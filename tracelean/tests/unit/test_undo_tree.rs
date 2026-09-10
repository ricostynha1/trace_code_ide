//! Unit tests for the undo tree data structure

use tracelean_lib::command_file;
use tracelean_lib::commands::Command;
use tracelean_lib::undo_tree::{CommitPoint, UndoTree};
use chrono::Utc;
use std::path::PathBuf;

fn insert_cmd(text: &str) -> (Command, Command) {
    let cmd = Command::insert(PathBuf::from("test.rs"), 0, text.to_string());
    let inv = cmd.inverse();
    (cmd, inv)
}

#[test]
fn push_creates_node() {
    let mut tree = UndoTree::new();
    assert!(tree.is_empty());

    let (cmd, inv) = insert_cmd("hello");
    tree.push(cmd, inv);

    assert_eq!(tree.len(), 1);
    assert!(tree.current_node().is_some());
}

#[test]
fn push_and_undo() {
    let mut tree = UndoTree::new();
    let (cmd, inv) = insert_cmd("hello");
    tree.push(cmd, inv);

    let undo_cmd = tree.undo();
    assert!(undo_cmd.is_some());
    assert!(tree.current_node().is_none());
}

#[test]
fn undo_at_root_returns_none() {
    let mut tree = UndoTree::new();
    assert!(tree.undo().is_none());
}

#[test]
fn redo_at_leaf_returns_none() {
    let mut tree = UndoTree::new();
    let (cmd, inv) = insert_cmd("hello");
    tree.push(cmd, inv);

    assert!(tree.redo().is_none());
}

#[test]
fn undo_redo_roundtrip() {
    let mut tree = UndoTree::new();
    let (cmd, inv) = insert_cmd("hello");
    tree.push(cmd, inv);

    tree.undo();
    assert!(tree.current_node().is_none());

    tree.redo();
    assert!(tree.current_node().is_some());
}

#[test]
fn branching_on_undo_plus_edit() {
    let mut tree = UndoTree::new();

    let (a, a_inv) = insert_cmd("A");
    let (b, b_inv) = insert_cmd("B");
    tree.push(a, a_inv);
    let node_a = tree.current_node().unwrap().id;
    tree.push(b, b_inv);

    tree.undo();

    let (c, c_inv) = insert_cmd("C");
    tree.push(c, c_inv);

    let a_node = tree.nodes().iter().find(|n| n.id == node_a).unwrap();
    assert_eq!(a_node.children.len(), 2);
    assert_eq!(tree.len(), 3);
}

#[test]
fn redo_takes_last_branch() {
    let mut tree = UndoTree::new();

    let (a, a_inv) = insert_cmd("A");
    let (b, b_inv) = insert_cmd("B");
    let (c, c_inv) = insert_cmd("C");

    tree.push(a, a_inv);
    tree.push(b, b_inv);
    tree.undo();
    tree.push(c, c_inv);
    let c_node_id = tree.current_node().unwrap().id;
    tree.undo();

    let redo = tree.redo();
    assert!(redo.is_some());
    assert_eq!(tree.current_node().unwrap().id, c_node_id);
}

#[test]
fn commit_point_on_current_node() {
    let mut tree = UndoTree::new();
    let (cmd, inv) = insert_cmd("hello");
    tree.push(cmd, inv);

    tree.set_commit_point(CommitPoint {
        name: "initial".into(),
        timestamp: Utc::now(),
        coverage: Some(95.0),
        spec_conformance: Some(true),
    });

    let node = tree.current_node().unwrap();
    assert!(node.commit_point.is_some());
    assert_eq!(node.commit_point.as_ref().unwrap().name, "initial");
}

#[test]
fn jump_to_node() {
    let mut tree = UndoTree::new();

    let (a, a_inv) = insert_cmd("A");
    let (b, b_inv) = insert_cmd("B");
    let (c, c_inv) = insert_cmd("C");

    tree.push(a, a_inv);
    let node_a_id = tree.current_node().unwrap().id;
    tree.push(b, b_inv);
    tree.push(c, c_inv);

    let commands = tree.jump_to(node_a_id);
    assert!(commands.is_some());
    assert_eq!(tree.current_node().unwrap().id, node_a_id);
}

#[test]
fn push_file_base_is_reachable_but_does_not_move_current() {
    // Regression: a file's undo history in "file mode" used to start at
    // its first real edit — there was no node representing the file's
    // content when it was first opened, so there was nothing to jump back
    // to before that. `push_file_base` fixes this, but must not behave
    // like `push` (parenting under `current` and moving it) — opening a
    // file isn't "doing something at the current position", and other
    // files' in-progress edit chains must be unaffected by it.
    let mut tree = UndoTree::new();

    let (a, a_inv) = insert_cmd("A"); // some unrelated ongoing history
    tree.push(a, a_inv);
    let unrelated_current = tree.current_node().unwrap().id;

    let base = Command::replace(PathBuf::from("new.rs"), 0, "".into(), "hello".into());
    let base_id = tree.push_file_base(base.clone(), base);

    assert_eq!(tree.current_node().unwrap().id, unrelated_current, "opening a file must not move `current`");
    assert_eq!(tree.len(), 2);

    let base_node = tree.nodes().iter().find(|n| n.id == base_id).unwrap();
    assert_eq!(command_file(&base_node.command).as_deref(), Some("new.rs"), "the base node must carry the file's own path, unlike push_initial's untargeted empty Batch");
}

#[test]
fn multiple_branches_all_preserved() {
    let mut tree = UndoTree::new();

    let (a, a_inv) = insert_cmd("A");
    tree.push(a, a_inv);
    let root_id = tree.current_node().unwrap().id;

    let (b1, b1_inv) = insert_cmd("B1");
    tree.push(b1, b1_inv);
    tree.undo();

    let (b2, b2_inv) = insert_cmd("B2");
    tree.push(b2, b2_inv);
    tree.undo();

    let (b3, b3_inv) = insert_cmd("B3");
    tree.push(b3, b3_inv);

    let root_node = tree.nodes().iter().find(|n| n.id == root_id).unwrap();
    assert_eq!(root_node.children.len(), 3);
    assert_eq!(tree.len(), 4);
}
