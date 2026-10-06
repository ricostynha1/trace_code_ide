//! Which paths a sandboxed workspace may write, and which of those come back.
//!
//! The classification is the whole security content of the feature, and it is
//! a total function from a path to one of four answers — which is why it is
//! here rather than inside the code that builds a container command line.
//! Stated once, it can be checked directly; buried in argument assembly, only
//! by running a container and hoping the test covered the case.

use serde::{Deserialize, Serialize};

/// What may happen to a path.
///
/// @implements REQ-SBX.classification_total
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Class {
    /// Writable inside the workspace, and never replayed onto the real tree.
    ///
    /// Tools legitimately read version control and the project's own state
    /// directory, and refusing breaks ordinary work — but nothing written
    /// there comes back.
    ///
    /// @implements REQ-SBX.protected_never_mirrored
    Protected,
    /// Writable, and mirrored back.
    Mirrored,
    /// Writable and regenerable: build output and caches, not worth mirroring.
    ///
    /// @implements REQ-SBX.passthrough_not_mirrored
    PassThrough,
    /// Not part of the project at all.
    ///
    /// @implements REQ-SBX.escape_is_not_silent
    Outside,
}

/// Paths whose changes are discarded even where they are writable.
pub const PROTECTED: &[&str] = &[".git", ".tracelean"];

/// Regenerable directories: writable, not mirrored.
pub const PASS_THROUGH: &[&str] =
    &["target", "node_modules", "dist", "build", ".venv", "venv", "__pycache__", ".lake"];

/// Classify a project-relative path.
///
/// Every path receives exactly one answer, including the paths that are trying
/// not to be answered: an absolute path, or one that climbs out of the project,
/// is `Outside` rather than being normalised into something that looks local.
///
/// @implements REQ-SBX.classification_total
/// @implements REQ-SBX.protected_never_mirrored
/// @implements REQ-SBX.passthrough_not_mirrored
/// @implements REQ-SBX.escape_is_not_silent
/// @drt REQ-SBX.classification_total
/// @drt REQ-SBX.protected_never_mirrored
/// @drt REQ-SBX.passthrough_not_mirrored
/// @drt REQ-SBX.escape_is_not_silent
pub fn classify(path: String) -> Class {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Class::Outside;
    }
    // Windows-style roots and drive letters are outside too, on any platform:
    // this decides about a string, and the string means the same thing
    // wherever it is read.
    if path.starts_with('\\') || (path.len() >= 2 && path.as_bytes()[1] == b':') {
        return Class::Outside;
    }

    // The path is resolved first, then classified by where it lands. A path is
    // what it reaches, not how it was spelled: `src/../.git/config` is the
    // version-control directory however it was written.
    let mut resolved: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                if resolved.pop().is_none() {
                    return Class::Outside;
                }
            }
            name => resolved.push(name),
        }
    }

    // A path that resolves to the project root itself is not a file in it.
    let Some(first) = resolved.first().copied() else { return Class::Outside };

    if PROTECTED.contains(&first) {
        return Class::Protected;
    }
    if resolved.iter().any(|segment| PASS_THROUGH.contains(segment)) {
        return Class::PassThrough;
    }
    Class::Mirrored
}

/// Whether a change at this path is replayed onto the real tree.
///
/// @implements REQ-MIRROR.protected_excluded
pub fn is_mirrored(path: &str) -> bool {
    classify(path.to_string()) == Class::Mirrored
}

/// Whether containment is available, as a named state rather than a silent
/// fallback.
///
/// A sandbox that silently is not one is the worst available failure: the user
/// needs to know before they run a tool, not after it has edited the real tree.
///
/// @implements REQ-SBX.capability_reported
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Capability {
    /// Full containment: the real tree is untouched while a tool runs.
    Contained { mechanism: String },
    /// No containment mechanism is available, and here is why.
    Unavailable { reason: String },
}

impl Capability {
    pub fn is_contained(&self) -> bool {
        matches!(self, Capability::Contained { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-SBX.classification_total
    #[test]
    fn ordinary_project_files_are_mirrored() {
        assert_eq!(classify("src/main.rs".into()), Class::Mirrored);
        assert_eq!(classify("a.rs".into()), Class::Mirrored);
        assert_eq!(classify("./src/main.rs".into()), Class::Mirrored);
        assert_eq!(classify("deep/nested/dir/file.txt".into()), Class::Mirrored);
    }

    /// @tests REQ-SBX.protected_never_mirrored
    #[test]
    fn version_control_and_state_are_protected() {
        assert_eq!(classify(".git/config".into()), Class::Protected);
        assert_eq!(classify(".git".into()), Class::Protected);
        assert_eq!(classify(".tracelean/drt.json".into()), Class::Protected);
        assert!(!is_mirrored(".git/HEAD"));
    }

    /// @tests REQ-SBX.passthrough_not_mirrored
    #[test]
    fn build_output_is_writable_and_not_mirrored() {
        assert_eq!(classify("target/debug/x".into()), Class::PassThrough);
        assert_eq!(classify("crates/core/target/x".into()), Class::PassThrough);
        assert_eq!(classify("formal/.lake/build/x".into()), Class::PassThrough);
        assert!(!is_mirrored("node_modules/a/b.js"));
    }

    /// A path trying not to be answered still gets exactly one answer.
    ///
    /// @tests REQ-SBX.escape_is_not_silent
    #[test]
    fn escaping_paths_are_outside_not_normalised() {
        assert_eq!(classify("../secrets".into()), Class::Outside);
        assert_eq!(classify("a/../../b".into()), Class::Outside);
        assert_eq!(classify("/etc/passwd".into()), Class::Outside);
        assert_eq!(classify("".into()), Class::Outside);
        assert_eq!(classify(".".into()), Class::Outside);
        assert_eq!(classify("C:\\Windows".into()), Class::Outside);
    }

    /// Climbing and coming back is still inside, and is classified by where it
    /// lands rather than by how it got there.
    #[test]
    fn a_path_that_climbs_and_returns_is_inside() {
        assert_eq!(classify("a/../b.rs".into()), Class::Mirrored);
        assert_eq!(classify("src/../.git/config".into()), Class::Protected);
    }

    /// @tests REQ-SBX.capability_reported
    #[test]
    fn missing_containment_is_a_named_state() {
        let unavailable = Capability::Unavailable { reason: "bwrap not found".into() };
        assert!(!unavailable.is_contained());
        assert!(Capability::Contained { mechanism: "bwrap+overlay".into() }.is_contained());
    }

    /// Totality: no path produces no answer.
    #[test]
    fn every_path_classifies() {
        for path in [
            "", "/", ".", "..", "a", "a/b", ".git", ".git/x", "target", "a/target/b",
            "../a", "a/../..", "\0", "x\0y", "C:/x", "\\\\server\\share",
        ] {
            let _ = classify(path.to_string());
        }
    }
}
