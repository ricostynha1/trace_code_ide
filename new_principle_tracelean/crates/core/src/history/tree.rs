use serde::{Deserialize, Serialize};

use super::command::{apply, inverse, Command, Outcome, Refusal, Workspace};

/// @implements ARCH-DETERMINISM.no_ambient_time
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub command: Command,
    pub inverse: Command,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub base: Workspace,
    nodes: Vec<Node>,
    current: Option<usize>,
    next_id: u64,
}

impl Tree {
    pub fn new(base: Workspace) -> Tree {
        Tree { base, nodes: Vec::new(), current: None, next_id: 0 }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn current(&self) -> Option<NodeId> {
        self.current.map(|i| self.nodes[i].id)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    pub fn state(&self) -> Result<Workspace, Refusal> {
        match self.current() {
            None => Ok(self.base.clone()),
            Some(id) => self.state_at(id),
        }
    }

    /// @implements REQ-UNDO.jump_equivalence
    pub fn state_at(&self, id: NodeId) -> Result<Workspace, Refusal> {
        let mut workspace = self.base.clone();
        for node_id in self.ancestry(id) {
            let node = self.node(node_id).expect("ancestry yields live nodes");
            workspace = apply(&workspace, &node.command)?;
        }
        Ok(workspace)
    }

    pub fn ancestry(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut cursor = Some(id);
        while let Some(node_id) = cursor {
            let Some(node) = self.node(node_id) else { break };
            path.push(node_id);
            cursor = node.parent;
        }
        path.reverse();
        path
    }

    /// @implements REQ-UNDO.no_loss_on_branch
    /// @implements REQ-UNDO.reachable
    pub fn push(&mut self, command: Command) -> Result<NodeId, Refusal> {
        let state = self.state()?;
        self.push_from(&state, command).map(|(id, _)| id)
    }

    pub fn push_from(&mut self, state: &Workspace, command: Command) -> Result<(NodeId, Workspace), Refusal> {
        let after = apply(state, &command)?;

        let id = NodeId(self.next_id);
        self.next_id += 1;
        let parent = self.current();
        let node = Node {
            id,
            inverse: inverse(&command),
            command,
            parent,
            children: Vec::new(),
        };
        if let Some(parent_index) = parent.and_then(|p| self.index_of(p)) {
            self.nodes[parent_index].children.push(id);
        }
        self.nodes.push(node);
        self.current = Some(self.nodes.len() - 1);
        Ok((id, after))
    }

    pub fn undo(&mut self) -> Option<NodeId> {
        let index = self.current?;
        let parent = self.nodes[index].parent;
        self.current = parent.and_then(|p| self.index_of(p));
        parent
    }

    pub fn redo(&mut self) -> Option<NodeId> {
        let children = match self.current {
            None => self.roots(),
            Some(index) => self.nodes[index].children.clone(),
        };
        let target = children.into_iter().max()?;
        self.current = self.index_of(target);
        Some(target)
    }

    /// @implements REQ-UNDO.reachable
    pub fn ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|n| n.id).collect()
    }

    pub fn roots(&self) -> Vec<NodeId> {
        self.nodes.iter().filter(|n| n.parent.is_none()).map(|n| n.id).collect()
    }

    /// @implements REQ-UNDO.path_via_ancestor
    pub fn jump_to(&mut self, target: NodeId) -> Result<Workspace, Refusal> {
        let from = self.current().map(|id| self.ancestry(id)).unwrap_or_default();
        let to = self.ancestry(target);

        let shared = from
            .iter()
            .zip(to.iter())
            .take_while(|(a, b)| a == b)
            .count();

        let mut workspace = self.state()?;
        for node_id in from[shared..].iter().rev() {
            let node = self.node(*node_id).expect("live");
            workspace = apply(&workspace, &node.inverse)?;
        }
        for node_id in &to[shared..] {
            let node = self.node(*node_id).expect("live");
            workspace = apply(&workspace, &node.command)?;
        }

        self.current = self.index_of(target);
        Ok(workspace)
    }

    /// @implements REQ-UNDO.preview_is_pure
    pub fn preview(&self, id: NodeId) -> Result<(Workspace, Workspace), Refusal> {
        let node = self.node(id).ok_or_else(|| Refusal::NoSuchFile { path: String::new() })?;
        let before = match node.parent {
            None => self.base.clone(),
            Some(parent) => self.state_at(parent)?,
        };
        let after = apply(&before, &node.command)?;
        Ok((before, after))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Step {
    Push { command: Command },
    Move { movement: Movement },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Movement {
    Undo,
    Redo,
    Jump { node: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub nodes: Vec<u64>,
    pub current: Option<u64>,
    pub travelled: Vec<Outcome>,
    pub replayed: Vec<Outcome>,
    pub previews: Vec<(Outcome, Outcome)>,
    /// @implements REQ-UNDO.preview_is_pure
    pub current_after_previews: Option<u64>,
}

/// @implements REQ-UNDO.jump_equivalence
/// @implements REQ-UNDO.path_via_ancestor
/// @implements REQ-UNDO.reachable
/// @implements REQ-UNDO.preview_is_pure
/// @drt REQ-UNDO.jump_equivalence
/// @drt REQ-UNDO.preview_is_pure
/// @drt REQ-UNDO.path_via_ancestor
/// @drt REQ-UNDO.reachable
/// @drt REQ-UNDO.no_loss_on_branch
pub fn run_script(base: Workspace, script: Vec<Step>) -> Report {
    let tree = from_script(base, script);

    let ids = tree.ids();
    let travelled = ids
        .iter()
        .map(|id| Outcome::from(tree.clone().jump_to(*id)))
        .collect();
    let replayed = ids.iter().map(|id| Outcome::from(tree.state_at(*id))).collect();

    let previewed = tree.clone();
    let previews = ids
        .iter()
        .map(|id| match previewed.preview(*id) {
            Ok((before, after)) => (Outcome::Ok { w: before }, Outcome::Ok { w: after }),
            Err(refusal) => {
                let kind = refusal.kind().to_string();
                (Outcome::Refused { kind: kind.clone() }, Outcome::Refused { kind })
            }
        })
        .collect();

    Report {
        nodes: ids.iter().map(|id| id.0).collect(),
        current: tree.current().map(|id| id.0),
        travelled,
        replayed,
        previews,
        current_after_previews: previewed.current().map(|id| id.0),
    }
}

pub fn from_script(base: Workspace, script: Vec<Step>) -> Tree {
    let mut tree = Tree::new(base);
    for step in script {
        match step {
            Step::Push { command } => {
                let _ = tree.push(command);
            }
            Step::Move { movement } => match movement {
                Movement::Undo => {
                    tree.undo();
                }
                Movement::Redo => {
                    tree.redo();
                }
                Movement::Jump { node } => {
                    let _ = tree.jump_to(NodeId(node));
                }
            },
        }
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(file: &str, offset: usize, text: &str) -> Command {
        Command::Insert { file: file.into(), offset, text: text.into() }
    }

    fn tree() -> Tree {
        Tree::new(Workspace::with(&[("a.rs", "")]))
    }

    #[test]
    fn push_from_the_held_state_is_push() {
        let (mut told, mut replayed) = (tree(), tree());
        for (offset, text) in [(0, "ab"), (1, "x"), (3, "yz")] {
            let state = told.state().unwrap();
            let (id, after) = told.push_from(&state, insert("a.rs", offset, text)).unwrap();
            assert_eq!(id, replayed.push(insert("a.rs", offset, text)).unwrap());
            assert_eq!(after, replayed.state().unwrap());
        }
        assert_eq!(told, replayed);
        let state = told.state().unwrap();
        assert!(told.push_from(&state, insert("a.rs", 99, "!")).is_err());
        assert_eq!(told, replayed);
    }

    /// @tests REQ-UNDO.no_loss_on_branch
    /// @tests REQ-UNDO.reachable
    #[test]
    fn undo_then_edit_branches_and_keeps_the_abandoned_path() {
        let mut t = tree();
        let first = t.push(insert("a.rs", 0, "one")).unwrap();
        t.undo();
        let second = t.push(insert("a.rs", 0, "two")).unwrap();

        assert_eq!(t.len(), 2);
        assert_eq!(t.state_at(first).unwrap().get("a.rs").unwrap(), "one");
        assert_eq!(t.state_at(second).unwrap().get("a.rs").unwrap(), "two");
    }

    /// @tests REQ-UNDO.jump_equivalence
    #[test]
    fn jumping_agrees_with_replaying_from_the_root() {
        let mut t = tree();
        let a = t.push(insert("a.rs", 0, "aaa")).unwrap();
        let b = t.push(insert("a.rs", 3, "bbb")).unwrap();
        t.jump_to(a).unwrap();
        let c = t.push(insert("a.rs", 0, "ccc")).unwrap();

        for target in [a, b, c] {
            let replayed = t.state_at(target).unwrap();
            let travelled = t.jump_to(target).unwrap();
            assert_eq!(replayed, travelled, "node {target:?}");
        }
    }

    #[test]
    fn travelling_between_branches_lands_correctly() {
        let mut t = tree();
        let root = t.push(insert("a.rs", 0, "R")).unwrap();
        let left = t.push(insert("a.rs", 1, "L")).unwrap();
        t.jump_to(root).unwrap();
        let right = t.push(insert("a.rs", 1, "X")).unwrap();

        assert_eq!(t.jump_to(left).unwrap().get("a.rs").unwrap(), "RL");
        assert_eq!(t.jump_to(right).unwrap().get("a.rs").unwrap(), "RX");
    }

    /// @tests REQ-CMD.total_or_refused
    #[test]
    fn a_refused_command_leaves_no_node() {
        let mut t = tree();
        assert!(t.push(insert("missing.rs", 0, "x")).is_err());
        assert!(t.is_empty());
    }

    /// @tests REQ-UNDO.preview_is_pure
    #[test]
    fn previewing_does_not_move_the_position() {
        let mut t = tree();
        let a = t.push(insert("a.rs", 0, "one")).unwrap();
        let b = t.push(insert("a.rs", 3, "two")).unwrap();
        let (before, after) = t.preview(a).unwrap();
        assert_eq!(before.get("a.rs").unwrap(), "");
        assert_eq!(after.get("a.rs").unwrap(), "one");
        assert_eq!(t.current(), Some(b));
    }

    #[test]
    fn undo_and_redo_are_inverse_movements() {
        let mut t = tree();
        let a = t.push(insert("a.rs", 0, "x")).unwrap();
        assert_eq!(t.undo(), None, "undoing the only node reaches the base");
        assert_eq!(t.state().unwrap().get("a.rs").unwrap(), "");
        assert_eq!(t.redo(), Some(a));
        assert_eq!(t.state().unwrap().get("a.rs").unwrap(), "x");
    }

    #[test]
    fn redo_follows_the_latest_branch() {
        let mut t = tree();
        t.push(insert("a.rs", 0, "one")).unwrap();
        t.undo();
        let second = t.push(insert("a.rs", 0, "two")).unwrap();
        t.undo();
        assert_eq!(t.redo(), Some(second));
    }
}
