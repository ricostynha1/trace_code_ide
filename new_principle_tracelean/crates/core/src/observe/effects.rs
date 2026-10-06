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
    /// A mirrored file in the real tree is missing from the workspace.
    MissingFromCopy { path: String },
    /// A mirrored file in the workspace differs from the tree it came from.
    CopyDiffers { path: String },
    /// The real tree changed while the workspace was live.
    RealTreeChanged { path: String },
    /// Something outside the workspace was written.
    Escaped { path: String },
    /// Containment was unavailable and nothing said so.
    ContainmentUnreported,
}

/// Three snapshots: the real tree before, the workspace the tool was given, and
/// the real tree after the tool exited.
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

/// One file of the real tree against the copy it should have produced.
///
/// A path that is not mirrored is not compared: `.git` inside the workspace is
/// *expected* to diverge from `.git` outside it, and build output is expected
/// to be regenerated.
fn copy_check(workspace: &Workspace, path: &str, content: &str) -> Option<Violation> {
    if !is_mirrored(path) {
        return None;
    }
    match workspace.files.get(path) {
        None => Some(Violation::MissingFromCopy { path: path.to_string() }),
        Some(copied) if copied == content => None,
        Some(_) => Some(Violation::CopyDiffers { path: path.to_string() }),
    }
}

/// The law of copying: the workspace holds every mirrored file of the tree it
/// came from, and the real tree is the same afterwards as before.
///
/// @implements REQ-OBS.workspace_is_a_copy
/// @implements REQ-SBX.real_tree_untouched
pub fn copy_violations(witness: CopyWitness) -> Vec<Violation> {
    let mut out: Vec<Violation> = witness
        .before
        .files
        .iter()
        .filter_map(|(path, content)| copy_check(&witness.workspace, path, content))
        .collect();

    // The union of both snapshots' paths, in one total order. Chaining the two
    // key iterators instead reports in before-then-after order, which is an
    // artefact of which snapshot was taken first — differential testing found
    // exactly that, on a witness whose two trees named different files.
    //
    // @implements ARCH-DETERMINISM.stable_ordering
    let paths: BTreeSet<&String> =
        witness.before.files.keys().chain(witness.after.files.keys()).collect();
    for path in paths {
        if witness.before.files.get(path) != witness.after.files.get(path) {
            out.push(Violation::RealTreeChanged { path: path.clone() });
        }
    }
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

/// Everything a single observed run got wrong.
///
/// @implements REQ-OBS.workspace_is_a_copy
/// @implements REQ-SBX.real_tree_untouched
/// @implements REQ-SBX.capability_reported
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
        let before = tree(&[("src/a.rs", "x"), (".git/HEAD", "ref"), ("target/o", "bin")]);
        // Only the mirrored file was copied; the protected and regenerable ones
        // were deliberately left out, and that is not a violation.
        let workspace = tree(&[("src/a.rs", "x")]);
        let witness = CopyWitness { before: before.clone(), workspace, after: before };
        assert_eq!(copy_violations(witness), vec![]);
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
            workspace: before,
            after: tree(&[("src/a.rs", "tampered")]),
        };
        assert_eq!(
            copy_violations(witness),
            vec![Violation::RealTreeChanged { path: "src/a.rs".into() }]
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
