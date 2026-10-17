//! What a project opens again: the tabs it had, less what no longer exists.
//!
//! The editor's `recall` reads and writes the names; which of them come back
//! is decided here, from the names and the files there are now.

/// The files to open again, in the order their tabs were opened, then the one
/// that was in front, so it is in front again — each only if it still exists.
///
/// @implements REQ-NAV.session_recalled
/// @drt REQ-NAV.session_recalled
pub fn reopened(files: Vec<String>, shown: Option<String>, existing: Vec<String>) -> Vec<String> {
    files.into_iter().chain(shown).filter(|path| existing.contains(path)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(all: &[&str]) -> Vec<String> {
        all.iter().map(|s| s.to_string()).collect()
    }

    /// @tests REQ-NAV.session_recalled
    #[test]
    fn a_removed_file_is_left_out_and_the_front_one_comes_last() {
        let back = reopened(names(&["a", "gone", "b"]), Some("a".into()), names(&["a", "b"]));
        assert_eq!(back, names(&["a", "b", "a"]));
        assert_eq!(reopened(names(&["a"]), Some("gone".into()), names(&["a"])), names(&["a"]));
    }
}
