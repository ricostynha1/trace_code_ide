//! Unit tests for AppState

use tracelean_lib::commands::Command;
use tracelean_lib::state::AppState;
use std::path::PathBuf;

#[test]
fn apply_insert_modifies_content() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "hello".into(),
    });

    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn apply_multiple_inserts() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "hello".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 5,
        text: " world".into(),
    });

    assert_eq!(state.get_content(&file), Some("hello world"));
}

#[test]
fn apply_delete_removes_text() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "hello world".into());

    state.apply(Command::Delete {
        file: file.clone(),
        offset: 5,
        len: 6,
        deleted_text: " world".into(),
    });

    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn apply_replace() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "hello world".into());

    state.apply(Command::replace(
        file.clone(),
        6,
        "world".into(),
        "rust".into(),
    ));

    assert_eq!(state.get_content(&file), Some("hello rust"));
}

#[test]
fn undo_reverts_insert() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "hello".into(),
    });
    assert_eq!(state.get_content(&file), Some("hello"));

    state.undo();
    assert_eq!(state.get_content(&file), Some(""));
}

#[test]
fn redo_reapplies() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "hello".into(),
    });
    state.undo();
    state.redo();
    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn multiple_undo_redo() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "A".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 1,
        text: "B".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 2,
        text: "C".into(),
    });

    assert_eq!(state.get_content(&file), Some("ABC"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("AB"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("A"));

    state.redo();
    assert_eq!(state.get_content(&file), Some("AB"));
}

#[test]
fn batch_atomic_undo() {
    let mut state = AppState::new();
    let file_a = PathBuf::from("a.rs");
    let file_b = PathBuf::from("b.rs");
    state.load_file(file_a.clone(), String::new());
    state.load_file(file_b.clone(), String::new());

    state.apply(Command::Batch {
        commands: vec![
            Command::Insert {
                file: file_a.clone(),
                offset: 0,
                text: "aaa".into(),
            },
            Command::Insert {
                file: file_b.clone(),
                offset: 0,
                text: "bbb".into(),
            },
        ],
    });

    assert_eq!(state.get_content(&file_a), Some("aaa"));
    assert_eq!(state.get_content(&file_b), Some("bbb"));

    state.undo();
    assert_eq!(state.get_content(&file_a), Some(""));
    assert_eq!(state.get_content(&file_b), Some(""));
}

#[test]
fn file_create_delete() {
    let mut state = AppState::new();
    let file = PathBuf::from("new.rs");

    state.apply(Command::CreateFile { path: file.clone() });
    assert!(state.get_buffer(&file).is_some());

    state.apply(Command::DeleteFile {
        path: file.clone(),
        content: String::new(),
    });
    assert!(state.get_buffer(&file).is_none());

    state.undo();
    assert!(state.get_buffer(&file).is_some());
}

#[test]
fn file_rename() {
    let mut state = AppState::new();
    let from = PathBuf::from("old.rs");
    let to = PathBuf::from("new.rs");
    state.load_file(from.clone(), "content".into());

    state.apply(Command::RenameFile {
        from: from.clone(),
        to: to.clone(),
    });

    assert!(state.get_buffer(&from).is_none());
    assert_eq!(state.get_content(&to), Some("content"));

    state.undo();
    assert_eq!(state.get_content(&from), Some("content"));
    assert!(state.get_buffer(&to).is_none());
}

#[test]
fn command_log_records_all() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "a".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 1,
        text: "b".into(),
    });

    assert_eq!(state.command_log().len(), 2);
}

// --- Additional undo edge-case tests ---

#[test]
fn undo_nothing_returns_false() {
    let mut state = AppState::new();
    assert!(!state.undo());
}

#[test]
fn redo_nothing_returns_false() {
    let mut state = AppState::new();
    assert!(!state.redo());
}

#[test]
fn undo_after_multiple_edits_reverts_last() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "start".into());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 5,
        text: "_1".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 7,
        text: "_2".into(),
    });

    assert_eq!(state.get_content(&file), Some("start_1_2"));

    // Undo last
    assert!(state.undo());
    assert_eq!(state.get_content(&file), Some("start_1"));

    // Undo first
    assert!(state.undo());
    assert_eq!(state.get_content(&file), Some("start"));

    // No more undo
    assert!(!state.undo());
    assert_eq!(state.get_content(&file), Some("start"));
}

#[test]
fn undo_branching_preserves_both_paths() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    // Path 1: insert A then B
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "A".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 1,
        text: "B".into(),
    });
    assert_eq!(state.get_content(&file), Some("AB"));

    // Undo B, back to "A"
    state.undo();
    assert_eq!(state.get_content(&file), Some("A"));

    // Branch: insert C instead
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 1,
        text: "C".into(),
    });
    assert_eq!(state.get_content(&file), Some("AC"));

    // Undo C, back to "A"
    state.undo();
    assert_eq!(state.get_content(&file), Some("A"));

    // Redo should go to C (last branch)
    state.redo();
    assert_eq!(state.get_content(&file), Some("AC"));
}

#[test]
fn undo_delete_restores_text() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "hello world".into());

    state.apply(Command::Delete {
        file: file.clone(),
        offset: 5,
        len: 6,
        deleted_text: " world".into(),
    });
    assert_eq!(state.get_content(&file), Some("hello"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("hello world"));
}

#[test]
fn undo_replace_restores_old_text() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "foo bar".into());

    state.apply(Command::replace(
        file.clone(),
        4,
        "bar".into(),
        "baz".into(),
    ));
    assert_eq!(state.get_content(&file), Some("foo baz"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("foo bar"));
}

#[test]
fn rapid_undo_redo_stability() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    // Apply 10 commands
    for i in 0..10 {
        state.apply(Command::Insert {
            file: file.clone(),
            offset: i,
            text: format!("{}", i % 10),
        });
    }
    assert_eq!(state.get_content(&file), Some("0123456789"));

    // Undo all
    for _ in 0..10 {
        assert!(state.undo());
    }
    assert_eq!(state.get_content(&file), Some(""));
    assert!(!state.undo()); // nothing left

    // Redo all
    for _ in 0..10 {
        assert!(state.redo());
    }
    assert_eq!(state.get_content(&file), Some("0123456789"));
    assert!(!state.redo()); // nothing left
}

#[test]
fn jump_to_node_works() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "A".into(),
    });
    // Get the node id after first insert
    let node_a_id = state.undo_tree().current_node().unwrap().id;

    state.apply(Command::Insert {
        file: file.clone(),
        offset: 1,
        text: "B".into(),
    });
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 2,
        text: "C".into(),
    });
    assert_eq!(state.get_content(&file), Some("ABC"));

    // Jump back to node A
    let commands = state.jump_to_node(node_a_id);
    assert!(commands.is_some());
    // Execute the traversal
    for cmd in commands.unwrap() {
        state.execute_raw(&cmd);
    }
    assert_eq!(state.get_content(&file), Some("A"));
}
