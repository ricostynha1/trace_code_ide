//! Shadow buffer diff → Insert/Delete command sequence.
//!
//! Given original content and new (shadow) content, produce a minimal
//! sequence of Insert and Delete commands that transforms one into the other.
//! No Replace. Ever.

use crate::commands::Command;
use std::path::PathBuf;

/// Diff original vs shadow buffer, emit Insert/Delete commands.
/// Commands are ordered so applying them sequentially on original yields shadow.
/// Offsets are adjusted for prior ops (each op shifts subsequent offsets).
pub fn diff_to_commands(file: &PathBuf, original: &str, shadow: &str) -> Vec<Command> {
    if original == shadow {
        return Vec::new();
    }

    let orig_bytes = original.as_bytes();
    let shadow_bytes = shadow.as_bytes();

    // Find common prefix
    let prefix_len = orig_bytes
        .iter()
        .zip(shadow_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Find common suffix (not overlapping prefix)
    let orig_rest = &orig_bytes[prefix_len..];
    let shadow_rest = &shadow_bytes[prefix_len..];
    let suffix_len = orig_rest
        .iter()
        .rev()
        .zip(shadow_rest.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let delete_end = original.len() - suffix_len;
    let insert_end = shadow.len() - suffix_len;

    let deleted_text = &original[prefix_len..delete_end];
    let inserted_text = &shadow[prefix_len..insert_end];

    let mut commands = Vec::new();

    // Delete first (at prefix_len), then insert at same position
    if !deleted_text.is_empty() {
        commands.push(Command::Delete {
            file: file.clone(),
            offset: prefix_len,
            len: deleted_text.len(),
            deleted_text: deleted_text.to_string(),
        });
    }

    if !inserted_text.is_empty() {
        commands.push(Command::Insert {
            file: file.clone(),
            offset: prefix_len,
            text: inserted_text.to_string(),
        });
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_change() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello", "hello");
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_pure_insert() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "ab", "aXb");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Insert { offset, text, .. } => {
                assert_eq!(*offset, 1);
                assert_eq!(text, "X");
            }
            _ => panic!("Expected Insert"),
        }
    }

    #[test]
    fn test_pure_delete() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "aXb", "ab");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Delete { offset, len, deleted_text, .. } => {
                assert_eq!(*offset, 1);
                assert_eq!(*len, 1);
                assert_eq!(deleted_text, "X");
            }
            _ => panic!("Expected Delete"),
        }
    }

    #[test]
    fn test_replace_becomes_delete_then_insert() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "aOLDb", "aNEWb");
        assert_eq!(cmds.len(), 2);
        match &cmds[0] {
            Command::Delete { offset, len, deleted_text, .. } => {
                assert_eq!(*offset, 1);
                assert_eq!(*len, 3);
                assert_eq!(deleted_text, "OLD");
            }
            _ => panic!("Expected Delete"),
        }
        match &cmds[1] {
            Command::Insert { offset, text, .. } => {
                assert_eq!(*offset, 1);
                assert_eq!(text, "NEW");
            }
            _ => panic!("Expected Insert"),
        }
    }

    #[test]
    fn test_append() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello", "hello world");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Insert { offset, text, .. } => {
                assert_eq!(*offset, 5);
                assert_eq!(text, " world");
            }
            _ => panic!("Expected Insert"),
        }
    }

    #[test]
    fn test_prefix_delete() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "hello world", "world");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Delete { offset, len, deleted_text, .. } => {
                assert_eq!(*offset, 0);
                assert_eq!(*len, 6);
                assert_eq!(deleted_text, "hello ");
            }
            _ => panic!("Expected Delete"),
        }
    }

    #[test]
    fn test_complete_rewrite() {
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), "aaa", "bbb");
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], Command::Delete { .. }));
        assert!(matches!(&cmds[1], Command::Insert { .. }));
    }

    #[test]
    fn test_multiline_change() {
        let orig = "fn hello() {\n    old();\n}\n";
        let shadow = "fn hello() {\n    new();\n}\n";
        let cmds = diff_to_commands(&PathBuf::from("f.rs"), orig, shadow);
        // Should delete "old" and insert "new" in the middle
        assert_eq!(cmds.len(), 2);
        match &cmds[0] {
            Command::Delete { deleted_text, .. } => assert_eq!(deleted_text, "old"),
            _ => panic!("Expected Delete"),
        }
        match &cmds[1] {
            Command::Insert { text, .. } => assert_eq!(text, "new"),
            _ => panic!("Expected Insert"),
        }
    }

    /// Verify commands actually reconstruct the shadow buffer when applied to original.
    #[test]
    fn test_apply_commands_reconstructs_shadow() {
        let cases = vec![
            ("hello", "world"),
            ("fn a() {}", "fn b() {}"),
            ("line1\nline2\n", "line1\nmodified\nline2\n"),
            ("abc", ""),
            ("", "new content"),
            ("fn x() {\n    old();\n}\n", "fn x() {\n    new_thing();\n    extra();\n}\n"),
        ];

        for (orig, shadow) in cases {
            let file = PathBuf::from("test.rs");
            let cmds = diff_to_commands(&file, orig, shadow);

            // Apply commands to original
            let mut content = orig.to_string();
            for cmd in &cmds {
                match cmd {
                    Command::Delete { offset, len, .. } => {
                        content.drain(*offset..(*offset + *len));
                    }
                    Command::Insert { offset, text, .. } => {
                        content.insert_str(*offset, text);
                    }
                    _ => panic!("Only Insert/Delete allowed"),
                }
            }
            assert_eq!(content, shadow, "Failed for orig={:?} shadow={:?}", orig, shadow);
        }
    }
}
