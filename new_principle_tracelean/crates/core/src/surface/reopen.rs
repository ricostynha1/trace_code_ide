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
