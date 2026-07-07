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
}

impl UndoTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            current: None,
            id_to_index: std::collections::HashMap::new(),
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
}

impl Default for UndoTree {
    fn default() -> Self {
        Self::new()
    }
}


