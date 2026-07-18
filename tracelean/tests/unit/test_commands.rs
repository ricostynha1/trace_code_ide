//! Unit tests for the Command primitive (char-indexed Replace).

use tracelean_lib::commands::{byte_index_at_char, Command};
use std::path::PathBuf;

#[test]
fn replace_inverse_swaps_old_new() {
    let cmd = Command::Replace {
        file: PathBuf::from("test.rs"),
        at: 5,
        old: "foo".into(),
        new: "bar".into(),
    };
    match cmd.inverse() {
        Command::Replace { file, at, old, new } => {
            assert_eq!(file, PathBuf::from("test.rs"));
            assert_eq!(at, 5);
            assert_eq!(old, "bar");
            assert_eq!(new, "foo");
        }
        other => panic!("Expected Replace, got {:?}", other),
    }
}

#[test]
fn insert_helper_is_replace_with_empty_old() {
    let cmd = Command::insert(PathBuf::from("test.rs"), 3, "hi".into());
    match &cmd {
        Command::Replace { at, old, new, .. } => {
            assert_eq!(*at, 3);
            assert_eq!(old, "");
            assert_eq!(new, "hi");
        }
        other => panic!("Expected Replace, got {:?}", other),
    }
    // Inverse of insert is delete of the same text
    match cmd.inverse() {
        Command::Replace { at, old, new, .. } => {
            assert_eq!(at, 3);
            assert_eq!(old, "hi");
            assert_eq!(new, "");
        }
        other => panic!("Expected Replace, got {:?}", other),
    }
}

#[test]
fn double_inverse_is_identity() {
    let cmd = Command::Replace {
        file: PathBuf::from("test.rs"),
        at: 7,
        old: "日本".into(),
        new: "🎉".into(),
    };
    match cmd.inverse().inverse() {
        Command::Replace { at, old, new, .. } => {
            assert_eq!(at, 7);
            assert_eq!(old, "日本");
            assert_eq!(new, "🎉");
        }
        other => panic!("Expected Replace, got {:?}", other),
    }
}

#[test]
fn rename_inverse_swaps_paths() {
    let cmd = Command::RenameFile {
        from: PathBuf::from("a.rs"),
        to: PathBuf::from("b.rs"),
    };
    match cmd.inverse() {
        Command::RenameFile { from, to } => {
            assert_eq!(from, PathBuf::from("b.rs"));
            assert_eq!(to, PathBuf::from("a.rs"));
        }
        _ => panic!("Expected RenameFile"),
    }
}

#[test]
fn batch_inverse_reverses_order() {
    let batch = Command::Batch {
        commands: vec![
            Command::insert(PathBuf::from("a.rs"), 0, "a".into()),
            Command::insert(PathBuf::from("b.rs"), 0, "b".into()),
        ],
    };
    match batch.inverse() {
        Command::Batch { commands } => {
            assert_eq!(commands.len(), 2);
            match &commands[0] {
                Command::Replace { file, old, new, .. } => {
                    assert_eq!(file, &PathBuf::from("b.rs"));
                    assert_eq!(old, "b");
                    assert_eq!(new, "");
                }
                _ => panic!("Expected Replace"),
            }
            match &commands[1] {
                Command::Replace { file, .. } => assert_eq!(file, &PathBuf::from("a.rs")),
                _ => panic!("Expected Replace"),
            }
        }
        _ => panic!("Expected Batch"),
    }
}

#[test]
fn create_file_inverse_is_delete_file() {
    let cmd = Command::CreateFile {
        path: PathBuf::from("new.rs"),
    };
    match cmd.inverse() {
        Command::DeleteFile { path, .. } => {
            assert_eq!(path, PathBuf::from("new.rs"));
        }
        _ => panic!("Expected DeleteFile"),
    }
}

#[test]
fn delete_file_inverse_restores_content() {
    let cmd = Command::DeleteFile {
        path: PathBuf::from("old.rs"),
        content: "fn main() {}".into(),
    };
    // Inverse recreates the file AND restores its content.
    match cmd.inverse() {
        Command::Batch { commands } => {
            assert!(matches!(&commands[0], Command::CreateFile { path } if path == &PathBuf::from("old.rs")));
            match &commands[1] {
                Command::Replace { at, old, new, .. } => {
                    assert_eq!(*at, 0);
                    assert_eq!(old, "");
                    assert_eq!(new, "fn main() {}");
                }
                _ => panic!("Expected Replace"),
            }
        }
        _ => panic!("Expected Batch"),
    }
}

#[test]
fn byte_index_at_char_handles_multibyte() {
    let s = "aé日🎉b";
    assert_eq!(byte_index_at_char(s, 0), 0);
    assert_eq!(byte_index_at_char(s, 1), 1); // after 'a'
    assert_eq!(byte_index_at_char(s, 2), 3); // after 'é' (2 bytes)
    assert_eq!(byte_index_at_char(s, 3), 6); // after '日' (3 bytes)
    assert_eq!(byte_index_at_char(s, 4), 10); // after '🎉' (4 bytes)
    assert_eq!(byte_index_at_char(s, 5), 11);
    assert_eq!(byte_index_at_char(s, 99), 11); // clamps to len
}

#[test]
fn cursor_after_lands_at_end_of_new_text() {
    let cmd = Command::Replace {
        file: PathBuf::from("test.rs"),
        at: 2,
        old: "xx".into(),
        new: "🎉🎉🎉".into(),
    };
    let hint = cmd.cursor_after().unwrap();
    assert_eq!(hint.char_pos, 5); // 2 + 3 chars (not bytes, not UTF-16 units)
}
