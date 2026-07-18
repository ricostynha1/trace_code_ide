//! Unit tests for the undo tree data structure

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
