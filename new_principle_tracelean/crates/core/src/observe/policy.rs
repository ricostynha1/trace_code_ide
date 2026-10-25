use serde::{Deserialize, Serialize};

/// @implements REQ-SBX.classification_total
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Class {
    /// @implements REQ-SBX.protected_never_mirrored
    Protected,
    Mirrored,
    /// @implements REQ-SBX.passthrough_not_mirrored
    PassThrough,
    /// @implements REQ-SBX.escape_is_not_silent
    Outside,
}

pub const PROTECTED: &[&str] = &[".git", ".tracelean"];

pub const PASS_THROUGH_ANYWHERE: &[&str] = &[
    "target", "node_modules", "__pycache__", ".lake", ".venv",
    ".cache", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".gradle", ".tox",
];

pub const PASS_THROUGH_AT_ROOT: &[&str] = &["build", "dist", "venv"];

/// @implements REQ-SBX.classification_total
/// @implements REQ-SBX.protected_never_mirrored
/// @implements REQ-SBX.passthrough_not_mirrored
/// @drt REQ-SBX.classification_total
/// @drt REQ-SBX.protected_never_mirrored
/// @drt REQ-SBX.passthrough_not_mirrored
/// @drt REQ-SBX.escape_is_not_silent
pub fn classify(path: String) -> Class {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Class::Outside;
    }
    if path.starts_with('\\') || (path.len() >= 2 && path.as_bytes()[1] == b':') {
        return Class::Outside;
    }

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

    let Some(first) = resolved.first().copied() else { return Class::Outside };

    if PROTECTED.contains(&first) {
        return Class::Protected;
    }
    if PASS_THROUGH_AT_ROOT.contains(&first)
        || resolved.iter().any(|segment| PASS_THROUGH_ANYWHERE.contains(segment))
    {
        return Class::PassThrough;
    }
    Class::Mirrored
}

/// @implements REQ-MIRROR.protected_excluded
pub fn is_mirrored(path: &str) -> bool {
    classify(path.to_string()) == Class::Mirrored
}

/// @implements REQ-SBX.capability_reported
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Capability {
    Contained { mechanism: String },
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
        assert_eq!(classify("tools/stage0/target/x".into()), Class::PassThrough);
        assert_eq!(classify("formal/.lake/build/x".into()), Class::PassThrough);
        assert_eq!(classify("build/out.o".into()), Class::PassThrough);
        assert!(!is_mirrored("node_modules/a/b.js"));
    }

    /// @tests REQ-SBX.passthrough_not_mirrored
    #[test]
    fn source_named_like_output_is_mirrored_and_caches_are_not() {
        assert_eq!(classify("src/build/mod.rs".into()), Class::Mirrored);
        assert_eq!(classify("web/dist/notes.md".into()), Class::Mirrored);
        assert_eq!(classify("lib/venv.py".into()), Class::Mirrored);
        for cache in [".cache/x", "py/.mypy_cache/x", ".pytest_cache/v", "app/.gradle/x", ".ruff_cache/a", ".tox/x"] {
            assert_eq!(classify(cache.into()), Class::PassThrough, "{cache}");
        }
    }

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
