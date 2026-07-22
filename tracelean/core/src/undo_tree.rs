//! Undo-tree: tree-structured history where branches form on undo+edit.
//! Not a linear stack — preserves all history paths.

use crate::commands::Command;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type NodeId = Uuid;

/// Metadata attached to commit points (named snapshots).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitPoint {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub coverage: Option<f64>,
    pub spec_conformance: Option<bool>,
}

/// A single node in the undo tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoNode {
    pub id: NodeId,
    pub command: Command,
    pub inverse: Command,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub timestamp: DateTime<Utc>,
    pub commit_point: Option<CommitPoint>,
}

/// The full undo tree. Stores all nodes, tracks current position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoTree {
    nodes: Vec<UndoNode>,
    /// Index of current node (where we are in history)
    current: Option<usize>,
    /// Map from NodeId to index for O(1) lookup
    id_to_index: std::collections::HashMap<NodeId, usize>,
    /// Item 6: per-file single-undo redo slot — path -> the node that
    /// `rewind_single_file(path)` most recently undid away from. Consumed
    /// (removed) by `redo_single_file`. A fresh edit to that file removes
    /// the slot (see `push`) since replaying it afterward would apply a
    /// `Replace` whose witness no longer matches the buffer.
    #[serde(default)]
    redo_targets: std::collections::HashMap<String, NodeId>,
}

impl UndoTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            current: None,
            id_to_index: std::collections::HashMap::new(),
            redo_targets: std::collections::HashMap::new(),
        }
    }

    /// Push an initial (base-state) node for a newly opened file.
    /// Uses a no-op Batch as the command. Only call when tree is empty.
    pub fn push_initial(&mut self) -> NodeId {
        let id = Uuid::new_v4();
        let node = UndoNode {
            id,
            command: Command::Batch { commands: vec![] },
            inverse: Command::Batch { commands: vec![] },
            parent: None,
            children: Vec::new(),
            timestamp: Utc::now(),
            commit_point: None,
        };
        let new_index = self.nodes.len();
        self.nodes.push(node);
        self.id_to_index.insert(id, new_index);
        self.current = Some(new_index);
        id
    }

    /// Push a new command. Creates child of current node (or root if empty).
    /// Returns the new node's ID.
    pub fn push(&mut self, command: Command, inverse: Command) -> NodeId {
        // Item 6: a fresh edit to a file invalidates any pending single-file
        // redo slot for that file — same "editing clears redo" rule a plain
        // undo/redo stack follows.
        if let Some(file) = crate::command_file(&command) {
            self.redo_targets.remove(&file);
        }

        let id = Uuid::new_v4();
        let parent = self.current.map(|idx| self.nodes[idx].id);
        let timestamp = Utc::now();

        let node = UndoNode {
            id,
            command,
            inverse,
            parent,
            children: Vec::new(),
            timestamp,
            commit_point: None,
        };

        let new_index = self.nodes.len();
        self.nodes.push(node);
        self.id_to_index.insert(id, new_index);

        // Add as child of parent
        if let Some(parent_idx) = self.current {
            self.nodes[parent_idx].children.push(id);
        }

        self.current = Some(new_index);
        id
    }

    /// Undo: move to parent node. Returns the inverse command to apply.
    /// Returns None if at root (nothing to undo).
    pub fn undo(&mut self) -> Option<&Command> {
        let current_idx = self.current?;
        let inverse = &self.nodes[current_idx].inverse;
        let parent_id = self.nodes[current_idx].parent;

        self.current = parent_id.and_then(|pid| self.id_to_index.get(&pid).copied());
        Some(inverse)
    }

    /// Redo: move to the most recent child (last branch taken).
    /// Returns the forward command to apply, or None if at leaf.
    pub fn redo(&mut self) -> Option<&Command> {
        let current_idx = self.current;

        // If at root (no current), try first node
        let children = match current_idx {
            Some(idx) => &self.nodes[idx].children,
            None => {
                // If tree has nodes but current is None, redo to first root node
                if self.nodes.is_empty() {
                    return None;
                }
                // Find root nodes (no parent)
                let root_idx = self.nodes.iter().position(|n| n.parent.is_none())?;
                self.current = Some(root_idx);
                return Some(&self.nodes[root_idx].command);
            }
        };

        if children.is_empty() {
            return None;
        }

        // Take last child (most recent branch)
        let child_id = *children.last().unwrap();
        let child_idx = *self.id_to_index.get(&child_id)?;
        self.current = Some(child_idx);
        Some(&self.nodes[child_idx].command)
    }

    /// Get current node (if any)
    pub fn current_node(&self) -> Option<&UndoNode> {
        self.current.map(|idx| &self.nodes[idx])
    }

    /// Replace the current node's command in place (typing-run coalescing).
    /// Refused if the node has children, is a commit point, or is the initial node.
    /// Returns true if amended.
    pub fn amend_current(&mut self, command: Command, inverse: Command) -> bool {
        let Some(idx) = self.current else { return false };
        let node = &mut self.nodes[idx];
        if !node.children.is_empty() || node.commit_point.is_some() || node.parent.is_none() {
            return false;
        }
        node.command = command;
        node.inverse = inverse;
        node.timestamp = Utc::now();
        true
    }

    /// Total number of nodes
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Set a commit point on the current node
    pub fn set_commit_point(&mut self, commit: CommitPoint) {
        if let Some(idx) = self.current {
            self.nodes[idx].commit_point = Some(commit);
        }
    }

    /// Get all nodes (for serialization/visualization)
    pub fn nodes(&self) -> &[UndoNode] {
        &self.nodes
    }

    /// Get a specific node by ID
    pub fn get_node(&self, id: NodeId) -> Option<&UndoNode> {
        self.id_to_index.get(&id).map(|&idx| &self.nodes[idx])
    }

    /// Jump to a specific node by ID. Returns commands needed to get there.
    /// This is for time-travel: find path from current to target.
    pub fn jump_to(&mut self, target: NodeId) -> Option<Vec<Command>> {
        let target_idx = *self.id_to_index.get(&target)?;

        // Find path: current → LCA → target
        let current_path = self.path_to_root(self.current);
        let target_path = self.path_to_root(Some(target_idx));

        // Find LCA (lowest common ancestor)
        let current_set: std::collections::HashSet<usize> = current_path.iter().copied().collect();
        let lca_idx = target_path.iter().find(|idx| current_set.contains(idx)).copied();

        let mut commands = Vec::new();

        // Undo from current back to LCA
        for &idx in &current_path {
            if Some(idx) == lca_idx {
                break;
            }
            commands.push(self.nodes[idx].inverse.clone());
        }

        // Redo from LCA to target
        let mut redo_path: Vec<usize> = Vec::new();
        for &idx in &target_path {
            if Some(idx) == lca_idx {
                break;
            }
            redo_path.push(idx);
        }
        redo_path.reverse();
        for idx in redo_path {
            commands.push(self.nodes[idx].command.clone());
        }

        self.current = Some(target_idx);
        Some(commands)
    }

    /// Get path from a node index to root (as list of indices)
    fn path_to_root(&self, from: Option<usize>) -> Vec<usize> {
        let mut path = Vec::new();
        let mut current = from;
        while let Some(idx) = current {
            path.push(idx);
            current = self.nodes[idx]
                .parent
                .and_then(|pid| self.id_to_index.get(&pid).copied());
        }
        path
    }

    /// Item 6: rewind only the most recent edit to `path`. Walks from
    /// `current` (inclusive) up through `parent` links to find the nearest
    /// node whose own `command` touches `path` — call it `target` — undoes
    /// just that node, then replays every *other-file* command between it
    /// and `current` as a new branch off `target`'s parent. The old branch
    /// (through `target`) is left in the tree untouched, reachable via
    /// `jump_to` like any other branch. Returns the ordered Commands the
    /// caller must execute against live buffers, or `None` if `path` has no
    /// history on the current branch.
    pub fn rewind_single_file(&mut self, path: &str) -> Option<Vec<Command>> {
        let current_idx = self.current?;

        // Walk current -> root (inclusive of current), collecting the
        // forward (command, inverse) pairs of nodes that do NOT touch
        // `path`, until the nearest node that does — `target`.
        let mut other_file: Vec<(Command, Command)> = Vec::new();
        let mut idx = current_idx;
        let target_idx = loop {
            if crate::command_affects_file(&self.nodes[idx].command, path) {
                break idx;
            }
            other_file.push((self.nodes[idx].command.clone(), self.nodes[idx].inverse.clone()));
            match self.nodes[idx]
                .parent
                .and_then(|pid| self.id_to_index.get(&pid).copied())
            {
                Some(p) => idx = p,
                None => return None, // reached root without an edit to `path`
            }
        };
        other_file.reverse(); // leaf-to-root -> chronological (root-to-leaf)

        let target_id = self.nodes[target_idx].id;
        let target_parent = self.nodes[target_idx].parent;

        // Record the redo slot before moving away from `target`.
        self.redo_targets.insert(path.to_string(), target_id);

        let mut result = match target_parent {
            Some(pid) => self.jump_to(pid)?,
            None => {
                // `target` is the tree's root — "undoing" it means moving
                // to the virtual pre-root state, same as plain `undo()`
                // does when it undoes a root node.
                let inv = self.nodes[target_idx].inverse.clone();
                self.current = None;
                vec![inv]
            }
        };

        // Replay the other-file commands as a new branch off target's parent.
        for (cmd, inv) in other_file {
            result.push(cmd.clone());
            self.push(cmd, inv);
        }

        Some(result)
    }

    /// Item 6: redo the most recent `rewind_single_file(path)` undo — jump
    /// back to the node that was undone, then replay the other-file edits
    /// made on the branch taken since then, as a new branch off it.
    /// Symmetric to `rewind_single_file` (same jump_to + push shape,
    /// reversed). Returns `None` if there's no pending redo slot for `path`.
    pub fn redo_single_file(&mut self, path: &str) -> Option<Vec<Command>> {
        let target_id = *self.redo_targets.get(path)?;
        let target_idx = *self.id_to_index.get(&target_id)?;
        let target_parent = self.nodes[target_idx].parent;
        let target_parent_idx = target_parent.and_then(|pid| self.id_to_index.get(&pid).copied());

        // Walk current -> target's parent (exclusive of it), collecting the
        // other-file commands applied on the branch taken since the
        // divergence, in chronological order once reversed.
        let mut other_file: Vec<(Command, Command)> = Vec::new();
        let mut idx = self.current;
        while let Some(i) = idx {
            if Some(i) == target_parent_idx {
                break;
            }
            other_file.push((self.nodes[i].command.clone(), self.nodes[i].inverse.clone()));
            idx = self.nodes[i]
                .parent
                .and_then(|pid| self.id_to_index.get(&pid).copied());
        }
        other_file.reverse();

        // Consume the slot — a normal redo stack is single-shot per edit.
        self.redo_targets.remove(path);

        let mut result = self.jump_to(target_id)?;

        for (cmd, inv) in other_file {
            result.push(cmd.clone());
            self.push(cmd, inv);
        }

        Some(result)
    }
}

impl Default for UndoTree {
    fn default() -> Self {
        Self::new()
    }
}


