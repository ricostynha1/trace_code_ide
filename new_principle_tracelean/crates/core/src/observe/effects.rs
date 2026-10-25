//! @implements ARCH-EFFECT-LAW.law_stated
//! @implements ARCH-CORE-SHELL.decision_public

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::history::command::Workspace;

use super::policy::{classify, is_mirrored, Capability, Class};

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Violation {
    MissingFromCopy { path: String },
    CopyDiffers { path: String },
    ExtraInCopy { path: String },
    RealTreeChanged { path: String },
    Escaped { path: String },
    ContainmentUnreported,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyWitness {
    pub before: Workspace,
    pub workspace: Workspace,
    pub after: Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunWitness {
    pub copy: CopyWitness,
    pub writes: Vec<String>,
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

/// @implements ARCH-DETERMINISM.stable_ordering
fn both_paths<'a>(one: &'a Workspace, other: &'a Workspace) -> BTreeSet<&'a String> {
    one.files.keys().chain(other.files.keys()).collect()
}

/// @implements REQ-SBX.real_tree_untouched
pub fn tree_changes(before: &Workspace, after: &Workspace) -> Vec<Violation> {
    both_paths(before, after)
        .into_iter()
        .filter(|path| before.files.get(*path) != after.files.get(*path))
        .map(|path| Violation::RealTreeChanged { path: path.clone() })
        .collect()
}

pub fn copy_violations(witness: CopyWitness) -> Vec<Violation> {
    let mut out: Vec<Violation> = both_paths(&witness.before, &witness.workspace)
        .into_iter()
        .filter_map(|path| copy_check(&witness.before, &witness.workspace, path))
        .collect();
    out.extend(tree_changes(&witness.before, &witness.after));
    out
}

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

/// @implements REQ-SBX.escape_is_not_silent
pub fn escape_violations(writes: Vec<String>) -> Vec<Violation> {
    writes
        .into_iter()
        .filter(|path| classify(path.clone()) == Class::Outside)
        .map(|path| Violation::Escaped { path })
        .collect()
}

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
        let workspace = tree(&[("src/a.rs", "x"), (".git", "")]);
        let witness = CopyWitness { before: before.clone(), workspace, after: before };
        assert_eq!(copy_violations(witness), vec![]);
    }

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
