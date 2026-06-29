//! Unit tests for persistence (command log save/load, checkpoints)

use tracelean_lib::commands::Command;
use tracelean_lib::persistence;
use tracelean_lib::state::AppState;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn save_and_load_command_log() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let commands = vec![
        Command::Insert {
            file: PathBuf::from("test.rs"),
            offset: 0,
            text: "hello".into(),
        },
        Command::Insert {
            file: PathBuf::from("test.rs"),
            offset: 5,
            text: " world".into(),
        },
    ];

    persistence::save_command_log(&root, &commands).unwrap();
    let loaded = persistence::load_command_log(&root).unwrap();

    assert_eq!(loaded.len(), 2);
}

#[test]
fn load_empty_log_returns_empty_vec() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let loaded = persistence::load_command_log(&root).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn save_and_load_checkpoint() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "checkpoint content".into(),
    });

    persistence::save_checkpoint(&root, &state).unwrap();
    let loaded = persistence::load_checkpoint(&root).unwrap();

    assert!(loaded.is_some());
    let checkpoint = loaded.unwrap();
    assert_eq!(checkpoint.command_index, 1);
}

#[test]
fn load_checkpoint_when_none_exists() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let loaded = persistence::load_checkpoint(&root).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn restore_state_from_log() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let commands = vec![
        Command::CreateFile {
            path: PathBuf::from("test.rs"),
        },
        Command::Insert {
            file: PathBuf::from("test.rs"),
            offset: 0,
            text: "restored".into(),
        },
    ];

    persistence::save_command_log(&root, &commands).unwrap();
    let state = persistence::restore_state(&root).unwrap();

    assert_eq!(
        state.get_content(&PathBuf::from("test.rs")),
        Some("restored")
    );
}

#[test]
fn restore_state_from_checkpoint_plus_remaining() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());
    state.apply(Command::Insert {
        file: file.clone(),
        offset: 0,
        text: "first".into(),
    });
    persistence::save_checkpoint(&root, &state).unwrap();

    let commands = vec![
        Command::Insert {
            file: PathBuf::from("test.rs"),
            offset: 0,
            text: "first".into(),
        },
        Command::Insert {
            file: PathBuf::from("test.rs"),
            offset: 5,
            text: " second".into(),
        },
    ];
    persistence::save_command_log(&root, &commands).unwrap();

    let restored = persistence::restore_state(&root).unwrap();
    assert_eq!(
        restored.get_content(&PathBuf::from("test.rs")),
        Some("first second")
    );
}

#[test]
fn ensure_dirs_creates_structure() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    persistence::ensure_dirs(&root).unwrap();

    let commands_dir = root.join(".tracelean").join("commands");
    assert!(commands_dir.exists());
    assert!(commands_dir.is_dir());
}
