//! Unit tests for commands module

use tracelean_lib::commands::Command;
use std::path::PathBuf;

#[test]
fn insert_inverse_is_delete() {
    let cmd = Command::Insert {
        file: PathBuf::from("test.rs"),
        offset: 0,
        text: "hello".into(),
    };
    let inv = cmd.inverse();
    match inv {
        Command::Delete {
            file,
            offset,
            len,
            deleted_text,
        } => {
            assert_eq!(file, PathBuf::from("test.rs"));
            assert_eq!(offset, 0);
            assert_eq!(len, 5);
            assert_eq!(deleted_text, "hello");
        }
        _ => panic!("Expected Delete"),
    }
}

#[test]
fn delete_inverse_is_insert() {
    let cmd = Command::Delete {
        file: PathBuf::from("test.rs"),
        offset: 3,
        len: 4,
        deleted_text: "world".into(),
    };
    let inv = cmd.inverse();
    match inv {
        Command::Insert { file, offset, text } => {
            assert_eq!(file, PathBuf::from("test.rs"));
            assert_eq!(offset, 3);
            assert_eq!(text, "world");
        }
        _ => panic!("Expected Insert"),
    }
}

#[test]
fn replace_helper_produces_batch_delete_insert() {
    let cmd = Command::replace(
        PathBuf::from("test.rs"),
        5,
        "foo".into(),
        "bar".into(),
    );
    match cmd {
        Command::Batch { commands } => {
            assert_eq!(commands.len(), 2);
            match &commands[0] {
                Command::Delete { offset, len, deleted_text, .. } => {
                    assert_eq!(*offset, 5);
                    assert_eq!(*len, 3);
                    assert_eq!(deleted_text, "foo");
                }
                _ => panic!("Expected Delete"),
            }
            match &commands[1] {
                Command::Insert { offset, text, .. } => {
                    assert_eq!(*offset, 5);
                    assert_eq!(text, "bar");
                }
                _ => panic!("Expected Insert"),
            }
        }
        _ => panic!("Expected Batch"),
    }
}

#[test]
fn replace_helper_inverse_is_batch_delete_insert_swapped() {
    let cmd = Command::replace(
        PathBuf::from("test.rs"),
        5,
        "foo".into(),
        "bar".into(),
    );
    let inv = cmd.inverse();
    // Inverse of Batch{Delete("foo"), Insert("bar")} = Batch{Delete("bar"), Insert("foo")}
    match inv {
        Command::Batch { commands } => {
            assert_eq!(commands.len(), 2);
            // Reversed order: inverse of Insert("bar") first, then inverse of Delete("foo")
            match &commands[0] {
                Command::Delete { offset, deleted_text, .. } => {
                    assert_eq!(*offset, 5);
                    assert_eq!(deleted_text, "bar");
                }
                _ => panic!("Expected Delete"),
            }
            match &commands[1] {
                Command::Insert { offset, text, .. } => {
                    assert_eq!(*offset, 5);
                    assert_eq!(text, "foo");
                }
                _ => panic!("Expected Insert"),
            }
        }
        _ => panic!("Expected Batch"),
    }
}

#[test]
fn rename_inverse_swaps_paths() {
    let cmd = Command::RenameFile {
        from: PathBuf::from("a.rs"),
        to: PathBuf::from("b.rs"),
    };
    let inv = cmd.inverse();
    match inv {
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
            Command::Insert {
                file: PathBuf::from("a.rs"),
                offset: 0,
                text: "a".into(),
            },
            Command::Insert {
                file: PathBuf::from("b.rs"),
                offset: 0,
                text: "b".into(),
            },
        ],
    };
    let inv = batch.inverse();
    match inv {
        Command::Batch { commands } => {
            assert_eq!(commands.len(), 2);
            match &commands[0] {
                Command::Delete { file, .. } => assert_eq!(file, &PathBuf::from("b.rs")),
                _ => panic!("Expected Delete"),
            }
            match &commands[1] {
                Command::Delete { file, .. } => assert_eq!(file, &PathBuf::from("a.rs")),
                _ => panic!("Expected Delete"),
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
    let inv = cmd.inverse();
    match inv {
        Command::DeleteFile { path, .. } => {
            assert_eq!(path, PathBuf::from("new.rs"));
        }
        _ => panic!("Expected DeleteFile"),
    }
}

#[test]
fn delete_file_inverse_is_create_file() {
    let cmd = Command::DeleteFile {
        path: PathBuf::from("old.rs"),
        content: "fn main() {}".into(),
    };
    let inv = cmd.inverse();
    match inv {
        Command::CreateFile { path } => {
            assert_eq!(path, PathBuf::from("old.rs"));
        }
        _ => panic!("Expected CreateFile"),
    }
}
