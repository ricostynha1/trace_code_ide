//! Shadow buffer diff → Replace command.
//!
//! Given original content and new (shadow) content, produce the minimal single
//! `Replace` command (char-indexed) that transforms one into the other by
//! trimming the common prefix and suffix.

use crate::commands::Command;
use std::path::PathBuf;

/// Diff original vs shadow buffer, emit at most one Replace command.
pub fn diff_to_commands(file: &PathBuf, original: &str, shadow: &str) -> Vec<Command> {
    if original == shadow {
        return Vec::new();
    }

    let orig_bytes = original.as_bytes();
    let shadow_bytes = shadow.as_bytes();

    // Common byte prefix, floored to a char boundary (boundaries coincide in
    // both strings since the bytes are equal up to prefix_len).
    let mut prefix_len = orig_bytes
        .iter()
        .zip(shadow_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();
    while !original.is_char_boundary(prefix_len) {
        prefix_len -= 1;
    }

    // Common byte suffix (not overlapping prefix), floored to a char boundary.
    let orig_rest = &orig_bytes[prefix_len..];
    let shadow_rest = &shadow_bytes[prefix_len..];
    let mut suffix_len = orig_rest
        .iter()
        .rev()
        .zip(shadow_rest.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !original.is_char_boundary(original.len() - suffix_len) {
        suffix_len -= 1;
    }

    let delete_end = original.len() - suffix_len;
    let insert_end = shadow.len() - suffix_len;

    let old = original[prefix_len..delete_end].to_string();
    let new = shadow[prefix_len..insert_end].to_string();
    let at = original[..prefix_len].chars().count();

    vec![Command::Replace {
        file: file.clone(),
        at,
        old,
        new,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_replace(cmds: &[Command]) -> (usize, &str, &str) {
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Replace { at, old, new, .. } => (*at, old.as_str(), new.as_str()),
            other => panic!("Expected Replace, got {:?}", other),
        }
    }

    #[test]
    fn test_no_change() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello", "hello");
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_pure_insert() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "ab", "aXb");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 1);
        assert_eq!(old, "");
        assert_eq!(new, "X");
    }

    #[test]
    fn test_pure_delete() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "aXb", "ab");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 1);
        assert_eq!(old, "X");
        assert_eq!(new, "");
    }

    #[test]
    fn test_replace_middle() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "aOLDb", "aNEWb");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 1);
        assert_eq!(old, "OLD");
        assert_eq!(new, "NEW");
    }

    #[test]
    fn test_append() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello", "hello world");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 5);
        assert_eq!(old, "");
        assert_eq!(new, " world");
    }

    #[test]
    fn test_prefix_delete() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello world", "world");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 0);
        assert_eq!(old, "hello ");
        assert_eq!(new, "");
    }

    #[test]
    fn test_multibyte_boundary() {
        // "é" and "è" share their first UTF-8 byte — the byte-prefix must be
        // floored back to a char boundary and `at` must be a char index.
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "日é本", "日è本");
        let (at, old, new) = expect_replace(&cmds);
        assert_eq!(at, 1);
        assert_eq!(old, "é");
        assert_eq!(new, "è");
    }

    #[test]
    fn test_multiline_change() {
        let orig = "fn hello() {\n    old();\n}\n";
        let shadow = "fn hello() {\n    new();\n}\n";
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), orig, shadow);
        let (_, old, new) = expect_replace(&cmds);
        assert_eq!(old, "old");
        assert_eq!(new, "new");
    }

    /// Verify the command actually reconstructs the shadow buffer.
    #[test]
    fn test_apply_command_reconstructs_shadow() {
        let cases = vec![
            ("hello", "world"),
            ("fn a() {}", "fn b() {}"),
            ("line1\nline2\n", "line1\nmodified\nline2\n"),
            ("abc", ""),
            ("", "new content"),
            ("日本語のテキスト", "日本語の新テキスト"),
            ("emoji 🎉 end", "emoji 🎊🎊 end"),
            ("fn x() {\n    old();\n}\n", "fn x() {\n    new_thing();\n    extra();\n}\n"),
        ];

        for (orig, shadow) in cases {
            let file = PathBuf::from("test.rs");
            let cmds = diff_to_commands(&file, orig, shadow);

            let mut state = crate::state::AppState::new();
            state.load_file(file.clone(), orig.to_string());
            for cmd in cmds {
                state.apply(cmd).expect("witness must match");
            }
            assert_eq!(
                state.get_content(&file).unwrap(),
                shadow,
                "Failed for orig={:?} shadow={:?}",
                orig,
                shadow
            );
        }
    }
}
