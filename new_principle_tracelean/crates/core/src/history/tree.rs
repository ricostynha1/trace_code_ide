//! History as a tree.
//!
//! A linear stack destroys work: undo three steps, type one character, and the
//! three are gone. They were an alternative, not a mistake. A tree keeps them.

use serde::{Deserialize, Serialize};

use super::command::{apply, inverse, Command, Outcome, Refusal, Workspace};

/// Node identity. A counter rather than a random identifier, because a derived
/// artefact must not embed anything that varies between runs of the same input.
///
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

/// The tree, and where in it we are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    /// The state before any command. Every node's state is this plus its
    /// ancestry.
    pub base: Workspace,
    nodes: Vec<Node>,
    /// Index into `nodes`; `None` means "at the base".
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

    /// The state at the current position.
    pub fn state(&self) -> Result<Workspace, Refusal> {
        match self.current() {
            None => Ok(self.base.clone()),
            Some(id) => self.state_at(id),
        }
    }

    /// The state at a node, by replaying its ancestry from the base.
    ///
    /// This is the *definition* of a node's state. `jump_to` is an optimisation
    /// of it, and the two are required to agree.
    ///
    /// @implements REQ-UNDO.jump_equivalence
    pub fn state_at(&self, id: NodeId) -> Result<Workspace, Refusal> {
        let mut workspace = self.base.clone();
        for node_id in self.ancestry(id) {
            let node = self.node(node_id).expect("ancestry yields live nodes");
            workspace = apply(&workspace, &node.command)?;
        }
        Ok(workspace)
    }

    /// Node identifiers from the root down to `id`, inclusive.
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

    /// Record a command, applying it to the current state.
    ///
    /// A command recorded after an undo becomes a *sibling* of what was undone,
    /// so the abandoned path is still there.
    ///
    /// @implements REQ-UNDO.no_loss_on_branch
    /// @implements REQ-UNDO.reachable
    pub fn push(&mut self, command: Command) -> Result<NodeId, Refusal> {
        let state = self.state()?;
        self.push_from(&state, command).map(|(id, _)| id)
    }

    /// `push`, told the state the tree is at rather than replaying it from the
    /// base: the same node, and the state after it. An editor holding the
    /// state saves a replay of the whole history on every key, which grew with
    /// the session until typing lagged. `state` must be `self.state()`.
    pub fn push_from(&mut self, state: &Workspace, command: Command) -> Result<(NodeId, Workspace), Refusal> {
        // Applied before anything is recorded, so a refused command leaves no
        // node behind.
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

    /// Move to the parent, undoing the current command.
    pub fn undo(&mut self) -> Option<NodeId> {
        let index = self.current?;
        let parent = self.nodes[index].parent;
        self.current = parent.and_then(|p| self.index_of(p));
        parent
    }

    /// Move to the most recently created child.
    pub fn redo(&mut self) -> Option<NodeId> {
        let children = match self.current {
            None => self.roots(),
            Some(index) => self.nodes[index].children.clone(),
        };
        // The latest branch, so redo after undo-and-branch follows the newest
        // work rather than silently resurrecting the abandoned one.
        let target = children.into_iter().max()?;
        self.current = self.index_of(target);
        Some(target)
    }

    /// Every node ever created, in creation order.
    ///
    /// @implements REQ-UNDO.reachable
    pub fn ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|n| n.id).collect()
    }

    pub fn roots(&self) -> Vec<NodeId> {
        self.nodes.iter().filter(|n| n.parent.is_none()).map(|n| n.id).collect()
    }

    /// Travel to a node by way of the nearest common ancestor: invert on the
    /// way up, apply on the way down.
    ///
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

    /// The diff a node represents, as the states either side of it.
    ///
    /// Producing it does not move the current position.
    ///
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

/// One step of a script driving a tree.
///
/// A tree is stateful, and the property worth checking is about a *history* of
/// operations rather than any single one. A script is how that history gets
/// into a generator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Step {
    /// An edit.
    Push { command: Command },
    /// A move through the history, which creates nothing.
    Move { movement: Movement },
}

/// Moving through a history, as opposed to adding to it.
///
/// Split from `Step` because the two are different kinds of thing — one records
/// work, the other travels over work already recorded — and because a generator
/// over a flat four-way choice spends three quarters of a script navigating a
/// tree that was never built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Movement {
    Undo,
    Redo,
    Jump { node: u64 },
}

/// What running a script produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Every node the script created, in creation order.
    pub nodes: Vec<u64>,
    pub current: Option<u64>,
    /// For each node, the state jumping to it reaches.
    pub travelled: Vec<Outcome>,
    /// For each node, the state replaying its ancestry from the base reaches.
    pub replayed: Vec<Outcome>,
    /// For each node, the diff it represents: the state before its command and
    /// the state after.
    pub previews: Vec<(Outcome, Outcome)>,
    /// Where the tree sits after every node has been previewed.
    ///
    /// Equal to `current` or the preview moved the position, which is what
    /// `preview_is_pure` forbids. A diff view that navigated as a side effect
    /// of being drawn would leave the user somewhere they did not ask to be —
    /// and worse, the *next* command would branch from there.
    ///
    /// @implements REQ-UNDO.preview_is_pure
    pub current_after_previews: Option<u64>,
}

/// Run a script against a fresh tree, then ask every node the same question two
/// ways.
///
/// `state_at` is the definition of a node's state; `jump_to` is an optimisation
/// of it that travels by way of the nearest common ancestor. They are required
/// to agree, and the report puts both answers side by side so a disagreement is
/// a divergence rather than something a reader has to notice.
///
/// Each jump runs on a copy, so the answers do not depend on the order they are
/// asked in.
///
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

    // Previewing runs on the tree itself, not a copy: the question is whether
    // it moves the position, and a copy could not answer it.
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

/// The tree a script builds.
///
/// Separate from `run_script` because more than one question is asked of a
/// generated history — what each node's state is, and where a position came
/// from — and both need the same tree built the same way.
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

    /// Told the state it is at, the tree records what `push` would, and says
    /// the state after it; a refused command still leaves nothing.
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
        // The abandoned path is still reachable and still says what it said.
        assert_eq!(t.state_at(first).unwrap().get("a.rs").unwrap(), "one");
        assert_eq!(t.state_at(second).unwrap().get("a.rs").unwrap(), "two");
    }

    /// The law: travelling and replaying must agree.
    ///
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

    /// Travelling between branches must go via the common ancestor, not
    /// through the root, and must still land in the same place.
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

    /// Redo after branching follows the newest work, not the abandoned path.
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
