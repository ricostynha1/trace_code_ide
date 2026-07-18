//! Unit tests for AppState — the char-indexed Replace primitive, witness
//! verification, undo/redo, coalescing, and Unicode round-trips.

use proptest::prelude::*;
use tracelean_lib::commands::Command;
use tracelean_lib::state::AppState;
use std::path::PathBuf;

fn fresh(content: &str) -> (AppState, PathBuf) {
    let mut state = AppState::new();
    // Granular tests want one node per command.
    state.set_coalescing(false);
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), content.into());
    (state, file)
}

#[test]
fn apply_insert_modifies_content() {
    let (mut state, file) = fresh("");
    state
        .apply(Command::insert(file.clone(), 0, "hello".into()))
        .unwrap();
    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn apply_multiple_inserts() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "hello".into())).unwrap();
    state.apply(Command::insert(file.clone(), 5, " world".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("hello world"));
}

#[test]
fn apply_delete_removes_text() {
    let (mut state, file) = fresh("hello world");
    state
        .apply(Command::delete(file.clone(), 5, " world".into()))
        .unwrap();
    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn apply_replace() {
    let (mut state, file) = fresh("hello world");
    state
        .apply(Command::replace(file.clone(), 6, "world".into(), "rust".into()))
        .unwrap();
    assert_eq!(state.get_content(&file), Some("hello rust"));
}

#[test]
fn char_indices_not_bytes() {
    // '日' is 3 bytes but 1 char — an edit after it must use char positions.
    let (mut state, file) = fresh("日本語x");
    state
        .apply(Command::replace(file.clone(), 3, "x".into(), "y".into()))
        .unwrap();
    assert_eq!(state.get_content(&file), Some("日本語y"));
}

#[test]
fn witness_mismatch_is_rejected_without_mutation() {
    let (mut state, file) = fresh("hello world");
    let err = state
        .apply(Command::replace(file.clone(), 0, "goodbye".into(), "x".into()))
        .unwrap_err();
    assert!(err.contains("integrity"), "unexpected error: {}", err);
    // Buffer untouched, nothing recorded
    assert_eq!(state.get_content(&file), Some("hello world"));
    assert_eq!(state.command_log().len(), 0);
}

#[test]
fn failed_batch_rolls_back_partial_effects() {
    let (mut state, file) = fresh("abc");
    let err = state.apply(Command::Batch {
        commands: vec![
            Command::replace(file.clone(), 0, "a".into(), "X".into()), // ok
            Command::replace(file.clone(), 1, "WRONG".into(), "Y".into()), // witness fails
        ],
    });
    assert!(err.is_err());
    // First sub-command must have been rolled back
    assert_eq!(state.get_content(&file), Some("abc"));
    assert_eq!(state.command_log().len(), 0);
}

#[test]
fn undo_reverts_insert() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "hello".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("hello"));
    let out = state.undo();
    assert!(out.changed);
    assert_eq!(state.get_content(&file), Some(""));
}

#[test]
fn redo_reapplies() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "hello".into())).unwrap();
    state.undo();
    let out = state.redo();
    assert!(out.changed);
    assert_eq!(state.get_content(&file), Some("hello"));
}

#[test]
fn undo_redo_cursor_hints() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "hello".into())).unwrap();

    // Undo removes "hello": cursor lands at position 0 (end of re-inserted "")
    let out = state.undo();
    assert_eq!(out.cursor.unwrap().char_pos, 0);

    // Redo re-inserts "hello": cursor lands after it
    let out = state.redo();
    assert_eq!(out.cursor.unwrap().char_pos, 5);
}

#[test]
fn multiple_undo_redo() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "A".into())).unwrap();
    state.apply(Command::insert(file.clone(), 1, "B".into())).unwrap();
    state.apply(Command::insert(file.clone(), 2, "C".into())).unwrap();
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
    state.set_coalescing(false);
    let file_a = PathBuf::from("a.rs");
    let file_b = PathBuf::from("b.rs");
    state.load_file(file_a.clone(), String::new());
    state.load_file(file_b.clone(), String::new());

    state
        .apply(Command::Batch {
            commands: vec![
                Command::insert(file_a.clone(), 0, "aaa".into()),
                Command::insert(file_b.clone(), 0, "bbb".into()),
            ],
        })
        .unwrap();

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

    state.apply(Command::CreateFile { path: file.clone() }).unwrap();
    assert!(state.get_buffer(&file).is_some());

    state
        .apply(Command::DeleteFile {
            path: file.clone(),
            content: String::new(),
        })
        .unwrap();
    assert!(state.get_buffer(&file).is_none());

    state.undo();
    assert!(state.get_buffer(&file).is_some());
}

#[test]
fn delete_file_undo_restores_content() {
    let mut state = AppState::new();
    let file = PathBuf::from("keep.rs");
    state.load_file(file.clone(), "precious".into());

    state
        .apply(Command::DeleteFile {
            path: file.clone(),
            content: "precious".into(),
        })
        .unwrap();
    assert!(state.get_buffer(&file).is_none());

    state.undo();
    assert_eq!(state.get_content(&file), Some("precious"));
}

#[test]
fn file_rename() {
    let mut state = AppState::new();
    let from = PathBuf::from("old.rs");
    let to = PathBuf::from("new.rs");
    state.load_file(from.clone(), "content".into());

    state
        .apply(Command::RenameFile { from: from.clone(), to: to.clone() })
        .unwrap();
    assert!(state.get_buffer(&from).is_none());
    assert_eq!(state.get_content(&to), Some("content"));

    state.undo();
    assert_eq!(state.get_content(&from), Some("content"));
    assert!(state.get_buffer(&to).is_none());
}

#[test]
fn command_log_records_all() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "a".into())).unwrap();
    state.apply(Command::insert(file.clone(), 1, "b".into())).unwrap();
    assert_eq!(state.command_log().len(), 2);
}

#[test]
fn undo_nothing_returns_unchanged() {
    let mut state = AppState::new();
    assert!(!state.undo().changed);
    assert!(!state.redo().changed);
}

#[test]
fn undo_branching_preserves_both_paths() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "A".into())).unwrap();
    state.apply(Command::insert(file.clone(), 1, "B".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("AB"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("A"));

    // Branch: insert C instead
    state.apply(Command::insert(file.clone(), 1, "C".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("AC"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("A"));

    // Redo goes to C (last branch)
    state.redo();
    assert_eq!(state.get_content(&file), Some("AC"));
}

#[test]
fn backspace_at_document_start_middle_end() {
    // Regression for "backspace deletes the first character": with witness
    // verification a mispositioned delete is rejected, not misapplied.
    let (mut state, file) = fresh("abc");

    // Backspace at end (delete 'c' at index 2)
    state.apply(Command::delete(file.clone(), 2, "c".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("ab"));

    // Backspace in middle (delete 'a' at index 0)
    state.apply(Command::delete(file.clone(), 0, "a".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("b"));

    // A stale delete aimed at index 1 (now out of range content) is refused
    let err = state.apply(Command::delete(file.clone(), 1, "b".into()));
    assert!(err.is_err());
    assert_eq!(state.get_content(&file), Some("b"));
}

#[test]
fn select_and_type_over_multibyte() {
    let (mut state, file) = fresh("héllo wörld 🎉");
    // Replace "wörld" (chars 6..11) with "🌍"
    state
        .apply(Command::replace(file.clone(), 6, "wörld".into(), "🌍".into()))
        .unwrap();
    assert_eq!(state.get_content(&file), Some("héllo 🌍 🎉"));
    state.undo();
    assert_eq!(state.get_content(&file), Some("héllo wörld 🎉"));
}

#[test]
fn rapid_undo_redo_stability() {
    let (mut state, file) = fresh("");
    for i in 0..10 {
        state
            .apply(Command::insert(file.clone(), i, format!("{}", i % 10)))
            .unwrap();
    }
    assert_eq!(state.get_content(&file), Some("0123456789"));

    for _ in 0..10 {
        assert!(state.undo().changed);
    }
    assert_eq!(state.get_content(&file), Some(""));
    assert!(!state.undo().changed);

    for _ in 0..10 {
        assert!(state.redo().changed);
    }
    assert_eq!(state.get_content(&file), Some("0123456789"));
    assert!(!state.redo().changed);
}

#[test]
fn jump_to_node_works() {
    let (mut state, file) = fresh("");
    state.apply(Command::insert(file.clone(), 0, "A".into())).unwrap();
    let node_a_id = state.undo_tree().current_node().unwrap().id;

    state.apply(Command::insert(file.clone(), 1, "B".into())).unwrap();
    state.apply(Command::insert(file.clone(), 2, "C".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("ABC"));

    let commands = state.jump_to_node(node_a_id).unwrap();
    for cmd in commands {
        state.execute_raw(&cmd);
    }
    assert_eq!(state.get_content(&file), Some("A"));
}

// --- Typing-run coalescing ---

#[test]
fn typing_run_coalesces_into_one_node() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());
    state.record_file_open();

    state.apply(Command::insert(file.clone(), 0, "A".into())).unwrap();
    state.apply(Command::insert(file.clone(), 1, "B".into())).unwrap();
    state.apply(Command::insert(file.clone(), 2, "C".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("ABC"));

    // One undo removes the whole typing run
    state.undo();
    assert_eq!(state.get_content(&file), Some(""));
    // ... and one redo restores it
    state.redo();
    assert_eq!(state.get_content(&file), Some("ABC"));
}

#[test]
fn backspace_run_coalesces() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), "abcd".into());
    state.record_file_open();

    // Backspace three times from the end
    state.apply(Command::delete(file.clone(), 3, "d".into())).unwrap();
    state.apply(Command::delete(file.clone(), 2, "c".into())).unwrap();
    state.apply(Command::delete(file.clone(), 1, "b".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("a"));

    state.undo();
    assert_eq!(state.get_content(&file), Some("abcd"));
}

#[test]
fn coalescing_stops_after_undo() {
    let mut state = AppState::new();
    let file = PathBuf::from("test.rs");
    state.load_file(file.clone(), String::new());
    state.record_file_open();

    state.apply(Command::insert(file.clone(), 0, "A".into())).unwrap();
    state.undo();
    // New edit after an undo must never amend the undone node
    state.apply(Command::insert(file.clone(), 0, "X".into())).unwrap();
    assert_eq!(state.get_content(&file), Some("X"));
    state.undo();
    assert_eq!(state.get_content(&file), Some(""));
}

// --- Property tests: random Unicode edit scripts round-trip ---

/// A char from a set that exercises ASCII, CJK, emoji and combining marks.
fn arb_char() -> impl Strategy<Value = char> {
    prop_oneof![
        proptest::char::range('a', 'z'),
        Just('日'),
        Just('本'),
        Just('é'),
        Just('🎉'),
        Just('\u{0300}'), // combining grave accent
        Just('\n'),
    ]
}

fn arb_text(max_len: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(arb_char(), 0..max_len).prop_map(|v| v.into_iter().collect())
}

/// An edit against a document of char-length `n`: (position-fraction, delete-count, insert text)
fn arb_edit() -> impl Strategy<Value = (usize, usize, String)> {
    (0usize..1000, 0usize..8, arb_text(6))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_edit_script_round_trips(
        initial in arb_text(40),
        edits in proptest::collection::vec(arb_edit(), 1..25),
    ) {
        let mut state = AppState::new();
        state.set_coalescing(false);
        let file = PathBuf::from("prop.rs");
        state.load_file(file.clone(), initial.clone());
        state.record_file_open();

        let mut n_applied = 0usize;
        for (pos_frac, del, insert) in edits {
            let content = state.get_content(&file).unwrap().to_string();
            let chars: Vec<char> = content.chars().collect();
            let at = if chars.is_empty() { 0 } else { pos_frac % (chars.len() + 1) };
            let del = del.min(chars.len().saturating_sub(at));
            let old: String = chars[at..at + del].iter().collect();
            state.apply(Command::replace(file.clone(), at, old, insert)).unwrap();
            n_applied += 1;
        }
        let final_content = state.get_content(&file).unwrap().to_string();

        // Undo all ⇒ original
        for _ in 0..n_applied {
            prop_assert!(state.undo().changed);
        }
        prop_assert_eq!(state.get_content(&file).unwrap(), initial.as_str());

        // Redo all ⇒ final
        for _ in 0..n_applied {
            prop_assert!(state.redo().changed);
        }
        prop_assert_eq!(state.get_content(&file).unwrap(), final_content.as_str());
    }

    #[test]
    fn random_jumps_are_consistent(
        initial in arb_text(20),
        edits in proptest::collection::vec(arb_edit(), 1..15),
        jumps in proptest::collection::vec(0usize..1000, 1..8),
    ) {
        let mut state = AppState::new();
        state.set_coalescing(false);
        let file = PathBuf::from("prop.rs");
        state.load_file(file.clone(), initial.clone());
        state.record_file_open();

        // Record content at each node as we go
        let mut node_contents = vec![(state.undo_tree().current_node().unwrap().id, initial.clone())];
        for (pos_frac, del, insert) in edits {
            let content = state.get_content(&file).unwrap().to_string();
            let chars: Vec<char> = content.chars().collect();
            let at = if chars.is_empty() { 0 } else { pos_frac % (chars.len() + 1) };
            let del = del.min(chars.len().saturating_sub(at));
            let old: String = chars[at..at + del].iter().collect();
            state.apply(Command::replace(file.clone(), at, old, insert)).unwrap();
            node_contents.push((
                state.undo_tree().current_node().unwrap().id,
                state.get_content(&file).unwrap().to_string(),
            ));
        }

        // Jump to random recorded nodes; buffer must match the recorded content.
        for j in jumps {
            let (node_id, expected) = &node_contents[j % node_contents.len()];
            let cmds = state.jump_to_node(*node_id).unwrap();
            for cmd in cmds {
                state.execute_raw(&cmd);
            }
            prop_assert_eq!(state.get_content(&file).unwrap(), expected.as_str());
        }
    }
}
