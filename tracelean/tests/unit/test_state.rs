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
    // One more: the file's own base-state node (its content — "" — when
    // first opened). Content doesn't visibly change (it's a no-op), but
    // the tree position does move, so this still reports `changed`.
    assert!(state.undo().changed);
    assert!(!state.undo().changed);

    // Symmetric: redo past the base node first, then the 10 real edits.
    assert!(state.redo().changed);
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

fn find_base_node_id(state: &AppState, file: &PathBuf) -> tracelean_lib::undo_tree::NodeId {
    state
        .undo_tree()
        .nodes()
        .iter()
        .find(|n| tracelean_lib::command_file(&n.command).as_deref() == Some(&file.to_string_lossy() as &str))
        .expect("load_file should have created a base node for this file")
        .id
}

#[test]
fn opening_a_file_lets_undo_tree_jump_back_to_the_pre_edit_state() {
    // Regression: opening a file never gave it a base-state node, so a
    // file's undo history in "file mode" started at its first real edit —
    // there was nothing to jump back to before that (the file's own
    // beginning was unreachable). `load_file` now creates one itself, for
    // every caller (see its doc comment) — this exercises the ordinary
    // "user opens a file" path (`fresh` calls `load_file`, same as
    // `service::open_file` does).
    let original = "fn main() {}\n";
    let (mut state, file) = fresh(original);

    // load_file claims `current` here because nothing else was in
    // progress (a fresh AppState) — it must, or the session's first real
    // edit would *also* parent under a `None` current and create a second,
    // ambiguous "root" (see `UndoTree::push_file_base`'s doc comment for
    // why `redo()` breaks if that happens).
    let base_id = find_base_node_id(&state, &file);
    assert_eq!(state.undo_tree().current_node().unwrap().id, base_id);

    state.apply(Command::insert(file.clone(), 12, " // edit 1".into())).unwrap();
    state.apply(Command::insert(file.clone(), 0, "// edit 2\n".into())).unwrap();
    assert_ne!(state.get_content(&file), Some(original));

    let commands = state.jump_to_node(base_id).unwrap();
    for cmd in commands {
        state.execute_raw(&cmd);
    }
    assert_eq!(state.get_content(&file), Some(original), "jumping to the base node must restore the file to its content when first opened");
}

#[test]
fn agent_editing_an_unopened_file_still_gets_a_base_node() {
    // Regression (resurfaced after the first fix, in a different code
    // path): the user never had this file open — an agent's edit tool
    // seeds the buffer with the file's pre-edit content via `load_file`
    // (`ai::tool_executor`, `sandbox::mirror::apply_mutations`, `acp::server`
    // all do this), *then* applies the real edit. `load_file` now creates
    // the same base node regardless of who calls it or why — it's no
    // longer tied to the editor's own `open_file` path specifically.
    let mut state = AppState::new();
    let file = PathBuf::from("agent_touched.rs");
    let original = "fn untouched() {}\n";

    // Exactly the shape agent tooling uses: load pre-edit content into a
    // buffer the user never opened, then apply the real edit as a Replace.
    state.load_file(file.clone(), original.to_string());
    let base_id = find_base_node_id(&state, &file);
    // Nothing else was in progress (fresh state), so this base node claims
    // `current` — see `push_file_base`'s doc comment for why it must, or
    // the next edit would create a second ambiguous root.
    assert_eq!(state.undo_tree().current_node().unwrap().id, base_id);

    state
        .apply(Command::replace(file.clone(), 0, original.to_string(), "fn agent_wrote_this() {}\n".to_string()))
        .unwrap();
    assert_ne!(state.get_content(&file), Some(original));

    let commands = state.jump_to_node(base_id).unwrap();
    for cmd in commands {
        state.execute_raw(&cmd);
    }
    assert_eq!(state.get_content(&file), Some(original), "a file an agent edited without the user ever opening it must still have a reachable pre-edit state");
}

#[test]
fn first_file_in_a_real_session_chains_under_its_own_base_not_the_project_placeholder() {
    // Regression: reported as "3 initial states show up after opening one
    // file and making one edit" — `service::open_project` calls
    // `record_file_open()` (-> `push_initial`) on *every* fresh project,
    // before any file is ever opened, which leaves `current` pointing at
    // its own untargeted empty-`Batch` placeholder node. The first fix's
    // `push_file_base` only claimed `current` when it was `None` — but
    // here it's already `Some` (the placeholder), so the claim never
    // happened, and the file's first real edit parented under the
    // placeholder too, becoming a *sibling* of its own base node instead
    // of a child. In file mode, a node's parent being outside the
    // filtered set makes it render as its own root — so the base node
    // AND the first edit both showed up as disconnected "initial-looking"
    // circles for one file after one edit.
    let mut state = AppState::new();
    // Exactly service::open_project's baseline step.
    state.record_file_open();
    state.mark_commit_point("Initial snapshot".to_string());

    let file = PathBuf::from("src/main.rs");
    let original = "fn main() {}\n";
    // Exactly service::open_file's sequence.
    state.load_file(file.clone(), original.to_string());
    state.record_file_open(); // no-op now (tree isn't empty), same as open_file's real call

    let base_id = find_base_node_id(&state, &file);
    assert_eq!(
        state.undo_tree().current_node().unwrap().id,
        base_id,
        "the first file opened in a real session must be able to claim `current` away from the project's untargeted placeholder root"
    );

    state.apply(Command::insert(file.clone(), 13, "// edit".into())).unwrap();
    let edit_id = state.undo_tree().current_node().unwrap().id;
    let edit_node = state.undo_tree().nodes().iter().find(|n| n.id == edit_id).unwrap();
    assert_eq!(
        edit_node.parent,
        Some(base_id),
        "the file's first real edit must chain under its own base node, not become a sibling of it"
    );
}

#[test]
fn agent_editing_a_second_file_does_not_steal_current_from_the_first() {
    // The other half of the design: opening/loading a *second* file while
    // the user is mid-edit on a *first* one must not yank `current` away
    // from that in-progress chain (an agent editing file B shouldn't
    // disturb the user's undo/redo position in file A) — the base-node
    // claim in `push_file_base` only fires when `current` is `None`.
    let (mut state, file_a) = fresh("a original\n");
    state.apply(Command::insert(file_a.clone(), 0, "user edit to A\n".into())).unwrap();
    let current_after_a_edit = state.undo_tree().current_node().unwrap().id;

    let file_b = PathBuf::from("agent_touched_b.rs");
    let original_b = "fn b() {}\n";
    state.load_file(file_b.clone(), original_b.to_string());
    assert_eq!(
        state.undo_tree().current_node().unwrap().id,
        current_after_a_edit,
        "loading file B must not move `current` away from file A's in-progress edit"
    );

    let base_id_b = find_base_node_id(&state, &file_b);
    state
        .apply(Command::replace(file_b.clone(), 0, original_b.to_string(), "fn agent_wrote_b() {}\n".to_string()))
        .unwrap();

    // File A is untouched by any of this.
    assert_eq!(state.get_content(&file_a).unwrap(), "user edit to A\na original\n");

    // File B's own history is still independently reachable back to its
    // pre-edit state.
    let commands = state.jump_to_node(base_id_b).unwrap();
    for cmd in commands {
        state.execute_raw(&cmd);
    }
    assert_eq!(state.get_content(&file_b), Some(original_b));
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

// --- P5: structured node diff for hover previews ---

#[test]
fn node_diff_structured_reports_hunks() {
    let (mut state, file) = fresh("line1\nline2\nline3\n");
    state.record_file_open();
    let base = state.undo_tree().current_node().unwrap().id;

    // Scripted 3-edit history
    state
        .apply(Command::replace(file.clone(), 6, "line2".into(), "LINE-TWO".into()))
        .unwrap();
    state
        .apply(Command::insert(file.clone(), 14, "\nextra".into()))
        .unwrap();
    state
        .apply(Command::insert(file.clone(), 0, "header\n".into()))
        .unwrap();
    assert_eq!(
        state.get_content(&file),
        Some("header\nline1\nLINE-TWO\nextra\nline3\n")
    );

    // Diff current → base: what changes if we jump back to the start
    let diff = state.node_diff_structured(base).expect("diff");
    assert_eq!(diff.files.len(), 1);
    let fd = &diff.files[0];
    assert!(fd.path.ends_with("test.rs"));
    // Jumping back removes header/LINE-TWO/extra and restores line2
    assert_eq!(fd.removed, 3);
    assert_eq!(fd.added, 1);

    // First hunk: "header" at current line 1 is removed
    assert_eq!(fd.hunks[0].current_start_line, 1);
    assert_eq!(fd.hunks[0].removed_lines, vec!["header".to_string()]);
    assert!(fd.hunks[0].added_lines.is_empty());

    // Second hunk: LINE-TWO + extra (current lines 3-4) replaced by line2
    assert_eq!(fd.hunks[1].current_start_line, 3);
    assert_eq!(
        fd.hunks[1].removed_lines,
        vec!["LINE-TWO".to_string(), "extra".to_string()]
    );
    assert_eq!(fd.hunks[1].added_lines, vec!["line2".to_string()]);
}

#[test]
fn node_diff_structured_same_node_is_empty() {
    let (mut state, file) = fresh("abc");
    state.apply(Command::insert(file.clone(), 3, "d".into())).unwrap();
    let here = state.undo_tree().current_node().unwrap().id;
    let diff = state.node_diff_structured(here).expect("diff");
    assert!(diff.files.is_empty());
}

// bugs.md Bug 1: accept-edits diff review — a stale PendingDiff (buffer moved
// since it was staged) must be rejected by the witness check, not silently
// mis-applied. This exercises diff_pipeline + AppState together exactly as
// `apply_accepted_hunks` (gui_backend/src/ipc/ai_commands.rs) does, without
// any Tauri/CLI coupling.
mod ai_diff_review {
    use super::*;
    use tracelean_lib::ai::diff_pipeline::{accepted_hunks_to_commands, create_pending_diff};

    #[test]
    fn accepted_diff_applies_cleanly_when_buffer_unchanged() {
        let original = "fn main() {\n    old();\n}\n";
        let proposed = "fn main() {\n    new();\n}\n";
        let (mut state, file) = fresh(original);

        let mut diff = create_pending_diff(&file.to_string_lossy(), original, proposed, "agent", "applied");
        for h in &mut diff.hunks {
            h.accepted = true;
        }
        let commands = accepted_hunks_to_commands(&diff);
        for cmd in commands {
            state.apply(cmd).expect("buffer matches the diff's base — must apply");
        }
        assert_eq!(state.get_content(&file), Some(proposed));
    }

    #[test]
    fn stale_diff_is_rejected_without_mutating_the_buffer() {
        let original = "fn main() {\n    old();\n}\n";
        let proposed = "fn main() {\n    new();\n}\n";
        let (mut state, file) = fresh(original);

        let mut diff = create_pending_diff(&file.to_string_lossy(), original, proposed, "agent", "applied");
        for h in &mut diff.hunks {
            h.accepted = true;
        }

        // The user kept typing after the diff was staged — the live buffer no
        // longer matches `diff.original`.
        state
            .apply(Command::insert(file.clone(), 0, "// edited\n".into()))
            .unwrap();
        let drifted_content = state.get_content(&file).unwrap().to_string();

        let commands = accepted_hunks_to_commands(&diff);
        for cmd in commands {
            let err = state.apply(cmd);
            assert!(err.is_err(), "stale diff must be rejected, not silently mis-applied");
        }
        // Buffer is exactly what it was after the user's edit — untouched by
        // the rejected accept, not partially patched.
        assert_eq!(state.get_content(&file), Some(drifted_content.as_str()));
    }
}
