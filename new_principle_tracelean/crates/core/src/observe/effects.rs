//! The laws the effectful parts of observation must obey.
//!
//! Lean cannot copy a directory or probe a host for a container. It can say
//! what copying must achieve, and that is what this module is the other half
//! of: `formal/TraceLean/Effects.lean` states each law once as a function over
//! a witness, and these are the same functions.
//!
//! Nothing here performs an effect. The shell that does — `workspace.rs` —
//! records what it observed and hands the witness to `run_violations`, so the
//! law is checked against what really happened rather than against a
//! description of what was supposed to happen.
//!
//! @implements ARCH-EFFECT-LAW.law_stated
//! @implements ARCH-CORE-SHELL.decision_public

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::history::command::Workspace;

use super::policy::{classify, is_mirrored, Capability, Class};

/// What a run got wrong.
///
/// Named rather than boolean: a law that reports `false` cannot be reviewed,
/// and the person deciding whether to accept a run needs to know which file.
///
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Violation {
    /// A mirrored file, or a protected root, of the real tree is not in what
    /// the tool was given.
    MissingFromCopy { path: String },
    /// A mirrored file in the workspace differs from the tree it came from.
    CopyDiffers { path: String },
    /// A mirrored file in the workspace that the real tree does not have.
    ExtraInCopy { path: String },
    /// The real tree changed while the workspace was live.
    RealTreeChanged { path: String },
    /// Something outside the workspace was written.
    Escaped { path: String },
    /// Containment was unavailable and nothing said so.
    ContainmentUnreported,
}

/// Three snapshots: the real tree before, the workspace the tool was given, and
/// the real tree after the tool exited.
///
/// A snapshot holds every mirrored file with its content, and each protected
/// root that is there (`.git`, `.tracelean`) as one entry whose content is not
/// read: whether the tool can see version control is the copy's business, what
/// it does inside is its own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyWitness {
    pub before: Workspace,
    pub workspace: Workspace,
    pub after: Workspace,
}

/// Everything one observed run can be judged on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunWitness {
    pub copy: CopyWitness,
    /// Every path the run wrote to, as observed.
    pub writes: Vec<String>,
    /// What the host reported about containment.
    pub containment: Capability,
}

impl Default for RunWitness {
    fn default() -> RunWitness {
        RunWitness {
            copy: CopyWitness::default(),
            writes: Vec::new(),
            containment: Capability::Unavailable { reason: "not probed".to_string() },
        }
    }
}

/// Whether what the tool was given is a copy of the project at one path.
///
/// A mirrored path is in both with the same bytes, or in neither. A protected
/// path the project has must be there for the tool to read, and its content is
/// not compared: `.git` inside the workspace is *expected* to diverge from
/// `.git` outside it. Build output is not compared at all — it is regenerated.
///
/// @implements REQ-OBS.workspace_is_a_copy
pub fn copy_check(before: &Workspace, workspace: &Workspace, path: &str) -> Option<Violation> {
    let path_owned = || path.to_string();
    let (was, given) = (before.files.get(path), workspace.files.get(path));
    if is_mirrored(path) {
        return match (was, given) {
            (Some(_), None) => Some(Violation::MissingFromCopy { path: path_owned() }),
            (Some(a), Some(b)) if a == b => None,
            (Some(_), Some(_)) => Some(Violation::CopyDiffers { path: path_owned() }),
            (None, Some(_)) => Some(Violation::ExtraInCopy { path: path_owned() }),
            (None, None) => None,
        };
    }
    if classify(path.to_string()) == Class::Protected && was.is_some() && given.is_none() {
        return Some(Violation::MissingFromCopy { path: path_owned() });
    }
    None
}

/// Every path either snapshot names, in one total order. Chaining the two key
/// iterators instead reports in first-then-second order, which is an artefact
/// of which snapshot was taken first — differential testing found exactly
/// that, on a witness whose two trees named different files.
///
/// @implements ARCH-DETERMINISM.stable_ordering
fn both_paths<'a>(one: &'a Workspace, other: &'a Workspace) -> BTreeSet<&'a String> {
    one.files.keys().chain(other.files.keys()).collect()
}

/// Every path at which the real tree after the run differs from the tree
/// before it: written, created or removed, wherever it is.
///
/// @implements REQ-SBX.real_tree_untouched
pub fn tree_changes(before: &Workspace, after: &Workspace) -> Vec<Violation> {
    both_paths(before, after)
        .into_iter()
        .filter(|path| before.files.get(*path) != after.files.get(*path))
        .map(|path| Violation::RealTreeChanged { path: path.clone() })
        .collect()
}

/// The law of copying: the tool was given a copy of the project, and the real
/// tree is the same afterwards as before.
pub fn copy_violations(witness: CopyWitness) -> Vec<Violation> {
    let mut out: Vec<Violation> = both_paths(&witness.before, &witness.workspace)
        .into_iter()
        .filter_map(|path| copy_check(&witness.before, &witness.workspace, path))
        .collect();
    out.extend(tree_changes(&witness.before, &witness.after));
    out
}

/// Whether the reported capability is usable as a report.
///
/// An empty mechanism or an empty reason is the silent fallback wearing a name:
/// nothing downstream can tell the user what happened or why.
///
/// @implements REQ-SBX.capability_reported
pub fn capability_violations(reported: Capability) -> Vec<Violation> {
    let empty = match &reported {
        Capability::Contained { mechanism } => mechanism.is_empty(),
        Capability::Unavailable { reason } => reason.is_empty(),
    };
    if empty {
        vec![Violation::ContainmentUnreported]
    } else {
        Vec::new()
    }
}

/// Every observed write that did not land inside the workspace.
///
/// The claim is not that an escape cannot happen — that is the container's job
/// — but that one is named rather than absorbed.
///
/// @implements REQ-SBX.escape_is_not_silent
pub fn escape_violations(writes: Vec<String>) -> Vec<Violation> {
    writes
        .into_iter()
        .filter(|path| classify(path.clone()) == Class::Outside)
        .map(|path| Violation::Escaped { path })
        .collect()
}

/// Everything a single observed run got wrong: the copy, the real tree, the
/// escapes and the containment report, in that order.
///
/// @implements ARCH-EFFECT-LAW.law_checked
/// @drt REQ-OBS.workspace_is_a_copy
/// @drt REQ-SBX.real_tree_untouched
/// @drt REQ-SBX.capability_reported
/// @drt REQ-SBX.escape_is_not_silent
pub fn run_violations(witness: RunWitness) -> Vec<Violation> {
    let mut out = copy_violations(witness.copy);
    out.extend(escape_violations(witness.writes));
    out.extend(capability_violations(witness.containment));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(&str, &str)]) -> Workspace {
        let mut w = Workspace::new();
        for (path, content) in entries {
            w.files.insert(path.to_string(), content.to_string());
        }
        w
    }

    /// @tests REQ-OBS.workspace_is_a_copy
    #[test]
    fn a_faithful_copy_of_the_mirrored_files_is_clean() {
        let before = tree(&[("src/a.rs", "x"), (".git", ""), ("target/o", "bin")]);
        // The mirrored file was copied and version control can be seen; build
        // output was deliberately left out, and that is not a violation.
        let workspace = tree(&[("src/a.rs", "x"), (".git", "")]);
        let witness = CopyWitness { before: before.clone(), workspace, after: before };
        assert_eq!(copy_violations(witness), vec![]);
    }

    /// A copy with more in it than the project, or without the version control
    /// a tool reads, is not a copy. Both passed silently before: the law only
    /// looked from the project into the copy, and only at mirrored files.
    ///
    /// @tests REQ-OBS.workspace_is_a_copy
    #[test]
    fn an_extra_file_and_unseen_version_control_are_reported() {
        let before = tree(&[("src/a.rs", "x"), (".git", ""), (".tracelean", "")]);
        let workspace = tree(&[("src/a.rs", "x"), ("src/planted.rs", "y"), (".tracelean", "")]);
        let witness = CopyWitness { before: before.clone(), workspace, after: before };
        assert_eq!(
            copy_violations(witness),
            vec![
                Violation::MissingFromCopy { path: ".git".into() },
                Violation::ExtraInCopy { path: "src/planted.rs".into() },
            ]
        );
    }

    /// @tests REQ-OBS.workspace_is_a_copy
    #[test]
    fn a_mirrored_file_left_out_of_the_copy_is_reported() {
        let before = tree(&[("src/a.rs", "x")]);
        let witness =
            CopyWitness { before: before.clone(), workspace: tree(&[]), after: before };
        assert_eq!(
            copy_violations(witness),
            vec![Violation::MissingFromCopy { path: "src/a.rs".into() }]
        );
    }

    /// @tests REQ-SBX.real_tree_untouched
    #[test]
    fn a_write_to_the_real_tree_while_the_workspace_is_live_is_reported() {
        let before = tree(&[("src/a.rs", "x")]);
        let witness = CopyWitness {
            before: before.clone(),
            workspace: before.clone(),
            after: tree(&[("src/a.rs", "tampered")]),
        };
        assert_eq!(
            copy_violations(witness),
            vec![Violation::RealTreeChanged { path: "src/a.rs".into() }]
        );
        // Created and removed are changes too, in path order.
        assert_eq!(
            tree_changes(&before, &tree(&[("b.rs", "new")])),
            vec![
                Violation::RealTreeChanged { path: "b.rs".into() },
                Violation::RealTreeChanged { path: "src/a.rs".into() },
            ]
        );
    }

    /// @tests REQ-SBX.capability_reported
    #[test]
    fn an_unnamed_capability_is_itself_a_violation() {
        assert_eq!(
            capability_violations(Capability::Unavailable { reason: String::new() }),
            vec![Violation::ContainmentUnreported]
        );
        assert_eq!(
            capability_violations(Capability::Unavailable { reason: "no bwrap".into() }),
            vec![]
        );
    }

    /// @tests REQ-SBX.escape_is_not_silent
    #[test]
    fn a_write_outside_the_project_is_named() {
        let writes = vec!["src/a.rs".to_string(), "/etc/passwd".to_string(), "../x".to_string()];
        assert_eq!(
            escape_violations(writes),
            vec![
                Violation::Escaped { path: "/etc/passwd".into() },
                Violation::Escaped { path: "../x".into() },
            ]
        );
    }
}
