//! The modal keymap.
//!
//! A mode machine is a total function from a mode and a key to one of four
//! outcomes. Stating that as a law is the difference between an interface that
//! occasionally swallows a key and one that cannot: "sometimes nothing happens"
//! is `step` being partial, and "escape left me somewhere unexpected" is the
//! parent relation not being a tree.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What a key does.
///
/// Exactly four possibilities, closed: a fifth would be a case the which-key
/// bar could not describe.
///
/// @implements REQ-MYTH.outcomes_closed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Move into a nested mode.
    Enter { mode: String },
    /// Run an action.
    Dispatch { action: String },
    /// Move one level towards the root.
    Leave { mode: String },
    /// The surface handles it — ordinary typing.
    PassThrough,
}

/// What a binding does when its key is pressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Binding {
    Enter { mode: String, description: String },
    Dispatch { action: String, description: String },
}

impl Binding {
    pub fn description(&self) -> &str {
        match self {
            Binding::Enter { description, .. } | Binding::Dispatch { description, .. } => {
                description
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    /// The mode one level closer to the root. `None` marks the root.
    #[serde(default)]
    pub parent: Option<String>,
    /// Ordered pairs rather than a map, matching the model's encoding.
    pub bindings: Vec<(String, Binding)>,
}

/// The keymap: data a user can edit.
///
/// @implements REQ-MYTH.keymap_is_data
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub root: String,
    pub modes: Vec<(String, Mode)>,
}

/// The key that leaves a mode.
pub const LEAVE: &str = "Escape";

/// The key that opens a leader menu.
///
/// A keymap is JSON a person edits, and a binding written on `" "` there is a
/// quoted space: indistinguishable from a typo and impossible to review. So the
/// keymap spells the key, and a frontend that received one asks for the
/// spelling rather than knowing it.
pub const SPACE: &str = "Space";

/// The keys a frontend reports by name rather than as the character typed.
///
/// Which of these a frontend can receive is its medium's business. What none of
/// them may do is invent a name: a name the keymap does not use is a binding
/// that silently does nothing, which is exactly what the space bar was — bound
/// as `Space`, reported by both frontends as `" "`, so `step` answered
/// `PassThrough` for a key the keymap bound and the leader menu was reachable
/// from neither. `actions_reachable` is a claim about the keymap; it is only
/// true of the running editor if the names on both sides of a key press are the
/// same names, so they are kept here rather than once per frontend.
///
/// @implements REQ-MYTH.actions_reachable
pub const NAMED: &[&str] =
    &["Backspace", "Down", "Enter", LEAVE, "Left", "Right", SPACE, "Tab", "Up"];

/// The keymap's name for a character a frontend received.
///
/// Almost every character names itself, and the one that cannot is the space.
/// A frontend deciding that for itself is a shell making a branch a function
/// could have made, so this is the one place that knows it.
///
/// @implements REQ-MYTH.actions_reachable
/// @implements ARCH-CORE-SHELL.shell_thin
pub fn typed(character: char) -> String {
    if character == ' ' {
        SPACE.to_string()
    } else {
        character.to_string()
    }
}

/// Every action the surface dispatches.
///
/// The registry a keymap is checked against: a binding naming something that is
/// not here is reported when the keymap loads, which is `actions_defined`, and
/// an entry here that no key sequence reaches is reported too, which is
/// `actions_reachable`. Keeping the list next to the mode machine is what makes
/// both checkable without the keymap layer knowing what any action does.
///
/// @implements REQ-MYTH.actions_defined
pub const ACTIONS: &[&str] = &[
    "drt.bindings",
    "drt.coverage",
    "drt.judge",
    "drt.run",
    "drt.shrink",
    "file.copy",
    "file.copy_line",
    "file.copy_path",
    "file.definition",
    "file.delete",
    "file.new",
    "file.open",
    "file.references",
    "file.rename",
    "file.save",
    "history.branch",
    "history.jump",
    "history.redo",
    "history.tree",
    "history.undo",
    "observe.accept",
    "observe.diff",
    "observe.accept_file",
    "observe.reject",
    "observe.reject_file",
    "observe.start",
    "sandbox.copy",
    "sandbox.end",
    "sandbox.new",
    "screen.close",
    "screen.focus.down",
    "screen.focus.left",
    "screen.focus.right",
    "screen.focus.up",
    "screen.grow",
    "screen.offers",
    "screen.show",
    "screen.shrink",
    "screen.split.across",
    "screen.split.down",
    "screen.station.design",
    "screen.station.history",
    "screen.station.project",
    "screen.station.requirements",
    "screen.station.sandbox",
    "screen.strip",
    "trace.approve",
    "trace.check",
    "trace.evidence",
    "trace.findings",
    "trace.judge",
    "trace.lock",
    "trace.new_requirement",
    "trace.requirement",
    "trace.rollup",
    "trace.stale",
];

/// The registry as a set.
pub fn actions() -> BTreeSet<String> {
    ACTIONS.iter().map(|name| name.to_string()).collect()
}

/// Why a keymap could not be loaded.
///
/// Unreadable text and a readable keymap that does not hold together are
/// different failures: the first is a typo in a file, the second is a keymap
/// that would work until somebody pressed the wrong key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The text is not a keymap at all.
    Unreadable { message: String },
    /// It is a keymap, and it does not hold together.
    Invalid { problems: Vec<Problem> },
}

/// Read a keymap from text, refusing one that does not hold together.
///
/// Both failures are reported here rather than at the key that triggers them:
/// a keymap that loads is one every key in it can be pressed against.
///
/// @implements REQ-MYTH.keymap_is_data
pub fn load(text: &str, actions: &BTreeSet<String>) -> Result<Keymap, LoadError> {
    let keymap: Keymap = serde_json::from_str(text)
        .map_err(|error| LoadError::Unreadable { message: error.to_string() })?;
    let problems = validate(&keymap, actions);
    if problems.is_empty() { Ok(keymap) } else { Err(LoadError::Invalid { problems }) }
}

impl Keymap {
    pub fn mode(&self, name: &str) -> Option<&Mode> {
        self.modes.iter().find(|(key, _)| key == name).map(|(_, mode)| mode)
    }

    pub fn mode_mut(&mut self, name: &str) -> Option<&mut Mode> {
        self.modes.iter_mut().find(|(key, _)| key == name).map(|(_, mode)| mode)
    }

    pub fn has_mode(&self, name: &str) -> bool {
        self.mode(name).is_some()
    }
}

impl Mode {
    pub fn binding(&self, key: &str) -> Option<&Binding> {
        self.bindings.iter().find(|(k, _)| k == key).map(|(_, binding)| binding)
    }

    /// Add or replace a binding, keeping the list ordered by key so that a
    /// keymap is a function of its content and not of its edit history.
    pub fn bind(&mut self, key: &str, binding: Binding) {
        self.bindings.retain(|(k, _)| k != key);
        self.bindings.push((key.to_string(), binding));
        self.bindings.sort_by(|a, b| a.0.cmp(&b.0));
    }

    pub fn unbind(&mut self, key: &str) {
        self.bindings.retain(|(k, _)| k != key);
    }
}

/// What a key does in a mode.
///
/// Total: every key in every mode has an outcome, and no key is swallowed. An
/// unbound key in the root mode is ordinary typing; an unbound key anywhere
/// else leaves the mode, so a mistyped leader never strands the user.
///
/// @implements REQ-MYTH.totality
/// @implements REQ-MYTH.escape_pops_one
/// @implements REQ-MYTH.outcomes_closed
/// @drt REQ-MYTH.totality
/// @drt REQ-MYTH.escape_pops_one
/// @drt REQ-MYTH.outcomes_closed
pub fn step(keymap: Keymap, mode: String, key: String) -> Outcome {
    let Some(current) = keymap.mode(&mode) else {
        // A mode that does not exist cannot bind anything, and pretending it
        // does would hide the misconfiguration. Falling back to the root is the
        // behaviour a user can recover from.
        return Outcome::PassThrough;
    };

    if let Some(binding) = current.binding(&key) {
        return match binding {
            Binding::Enter { mode, .. } => Outcome::Enter { mode: mode.clone() },
            Binding::Dispatch { action, .. } => Outcome::Dispatch { action: action.clone() },
        };
    }

    if key == LEAVE {
        return match &current.parent {
            Some(parent) => Outcome::Leave { mode: parent.clone() },
            None => Outcome::PassThrough,
        };
    }

    match &current.parent {
        None => Outcome::PassThrough,
        Some(_) => Outcome::Leave { mode: keymap.root.clone() },
    }
}

/// The mode a key leaves the machine in.
///
/// @implements REQ-MYTH.totality
pub fn next_mode(keymap: Keymap, mode: String, key: String) -> String {
    let root = keymap.root.clone();
    match step(keymap, mode.clone(), key) {
        Outcome::Enter { mode } => mode,
        Outcome::Leave { mode } => mode,
        Outcome::Dispatch { .. } => root,
        Outcome::PassThrough => mode,
    }
}

/// Keys available in a mode, with what they do.
///
/// Computed from the same data that dispatches the key, so the bar cannot
/// describe a binding that does not exist or omit one that does.
///
/// @implements REQ-MYTH.whichkey_is_a_query
pub fn which_key(keymap: &Keymap, mode: &str) -> Vec<(String, String)> {
    let Some(current) = keymap.mode(mode) else { return Vec::new() };
    let mut out: Vec<(String, String)> = current
        .bindings
        .iter()
        .map(|(key, binding)| (key.clone(), binding.description().to_string()))
        .collect();
    if current.parent.is_some() {
        out.push((LEAVE.to_string(), "leave this mode".to_string()));
    }
    out
}

/// What is wrong with a keymap.
///
/// Ordered by variant and then by content, so the report is a function of the
/// keymap. It was previously sorted by its own debug rendering, which made the
/// order an accident of a formatting implementation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Problem {
    /// A binding enters a mode that does not exist.
    UndefinedMode { mode: String, key: String, target: String },
    /// A binding names an action the registry does not have.
    UndefinedAction { mode: String, key: String, action: String },
    /// Leaving would not terminate.
    ParentCycle { mode: String },
    /// A mode no key sequence reaches from the root.
    UnreachableMode { mode: String },
    /// An action no key sequence reaches from the root.
    UnreachableAction { action: String },
    /// The declared root does not exist.
    NoRoot { root: String },
}

/// Check a keymap against the actions that exist.
///
/// Reported at load, not on the key that triggers it: a binding that fails when
/// pressed is a keymap that works until the moment somebody needs it.
///
/// @implements REQ-MYTH.modes_defined
/// @implements REQ-MYTH.actions_defined
/// @implements REQ-MYTH.escape_terminates
/// @implements REQ-MYTH.actions_reachable
/// @implements REQ-MYTH.keymap_is_data
pub fn validate(keymap: &Keymap, actions: &BTreeSet<String>) -> Vec<Problem> {
    let mut problems = Vec::new();

    if !keymap.has_mode(&keymap.root) {
        problems.push(Problem::NoRoot { root: keymap.root.clone() });
    }

    for (name, mode) in &keymap.modes {
        for (key, binding) in &mode.bindings {
            match binding {
                Binding::Enter { mode: target, .. } if !keymap.has_mode(target) => {
                    problems.push(Problem::UndefinedMode {
                        mode: name.clone(),
                        key: key.clone(),
                        target: target.clone(),
                    });
                }
                Binding::Dispatch { action, .. } if !actions.contains(action) => {
                    problems.push(Problem::UndefinedAction {
                        mode: name.clone(),
                        key: key.clone(),
                        action: action.clone(),
                    });
                }
                _ => {}
            }
        }

        // Leaving must reach the root in finitely many steps.
        let mut seen = BTreeSet::from([name.clone()]);
        let mut cursor = mode.parent.clone();
        while let Some(parent) = cursor {
            if !seen.insert(parent.clone()) {
                problems.push(Problem::ParentCycle { mode: name.clone() });
                break;
            }
            cursor = keymap.mode(&parent).and_then(|m| m.parent.clone());
        }
    }

    // Reachability from the root, over `Enter` edges.
    let mut reachable = BTreeSet::from([keymap.root.clone()]);
    let mut frontier = vec![keymap.root.clone()];
    while let Some(name) = frontier.pop() {
        let Some(mode) = keymap.mode(&name) else { continue };
        for (_, binding) in &mode.bindings {
            if let Binding::Enter { mode: target, .. } = binding {
                if reachable.insert(target.clone()) {
                    frontier.push(target.clone());
                }
            }
        }
    }
    for (name, _) in &keymap.modes {
        if !reachable.contains(name) {
            problems.push(Problem::UnreachableMode { mode: name.clone() });
        }
    }

    let mut dispatchable = BTreeSet::new();
    for name in &reachable {
        let Some(mode) = keymap.mode(name) else { continue };
        for (_, binding) in &mode.bindings {
            if let Binding::Dispatch { action, .. } = binding {
                dispatchable.insert(action.clone());
            }
        }
    }
    for action in actions {
        if !dispatchable.contains(action) {
            problems.push(Problem::UnreachableAction { action: action.clone() });
        }
    }

    problems.sort();
    problems
}

/// `validate`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-MYTH.modes_defined
/// @implements REQ-MYTH.actions_defined
/// @implements REQ-MYTH.escape_terminates
/// @implements REQ-MYTH.actions_reachable
/// @implements REQ-MYTH.keymap_is_data
/// @drt REQ-MYTH.modes_defined
/// @drt REQ-MYTH.actions_defined
/// @drt REQ-MYTH.escape_terminates
/// @drt REQ-MYTH.actions_reachable
/// @drt REQ-MYTH.keymap_is_data
pub fn validate_of(keymap: Keymap, actions: Vec<String>) -> Vec<Problem> {
    validate(&keymap, &actions.into_iter().collect())
}

/// `which_key`, in the same shape.
///
/// @implements REQ-MYTH.whichkey_is_a_query
/// @drt REQ-MYTH.whichkey_is_a_query
pub fn which_key_of(keymap: Keymap, mode: String) -> Vec<(String, String)> {
    which_key(&keymap, &mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter(mode: &str) -> Binding {
        Binding::Enter { mode: mode.into(), description: format!("enter {mode}") }
    }

    fn dispatch(action: &str) -> Binding {
        Binding::Dispatch { action: action.into(), description: action.into() }
    }

    fn mode(parent: Option<&str>, bindings: &[(&str, Binding)]) -> Mode {
        Mode {
            parent: parent.map(str::to_string),
            bindings: bindings.iter().map(|(k, b)| (k.to_string(), b.clone())).collect(),
        }
    }

    fn keymap() -> Keymap {
        Keymap {
            root: "Main".into(),
            modes: vec![
                ("Main".to_string(), mode(None, &[("C-.", enter("Options"))])),
                (
                    "Options".to_string(),
                    mode(Some("Main"), &[("f", enter("File")), ("u", dispatch("undo"))]),
                ),
                ("File".to_string(), mode(Some("Options"), &[("o", dispatch("open_file"))])),
            ],
        }
    }

    fn actions() -> BTreeSet<String> {
        ["undo", "open_file"].iter().map(|a| a.to_string()).collect()
    }

    /// No key is ever swallowed.
    ///
    /// @tests REQ-MYTH.totality
    #[test]
    fn every_key_in_every_mode_has_an_outcome() {
        let map = keymap();
        for (mode, _) in map.modes.clone() {
            for key in ["a", "z", "C-.", "u", "f", "o", "Escape", "F13", "€"] {
                let outcome = step(map.clone(), mode.clone(), key.to_string());
                // Exhaustive by construction — the point is that the call
                // returns rather than that it returns anything in particular.
                match outcome {
                    Outcome::Enter { .. }
                    | Outcome::Dispatch { .. }
                    | Outcome::Leave { .. }
                    | Outcome::PassThrough => {}
                }
            }
        }
    }

    /// @tests REQ-MYTH.escape_pops_one
    #[test]
    fn leaving_moves_exactly_one_level() {
        let map = keymap();
        assert_eq!(
            step(map.clone(), "File".into(), LEAVE.into()),
Outcome::Leave { mode: "Options".into() }
        );
        assert_eq!(
            step(map.clone(), "Options".into(), LEAVE.into()),
Outcome::Leave { mode: "Main".into() }
        );
        // At the root there is nothing to leave, so the key is ordinary.
        assert_eq!(step(map, "Main".into(), LEAVE.into()), Outcome::PassThrough);
    }

    /// @tests REQ-MYTH.escape_terminates
    #[test]
    fn repeated_leaving_reaches_the_root() {
        let map = keymap();
        let mut mode = "File".to_string();
        for _ in 0..10 {
            mode = next_mode(map.clone(), mode, LEAVE.into());
        }
        assert_eq!(mode, "Main");
    }

    /// A mistyped leader must not strand the user in a mode.
    #[test]
    fn an_unbound_key_in_a_nested_mode_returns_to_the_root() {
        let map = keymap();
        assert_eq!(
            step(map.clone(), "Options".into(), "q".into()),
Outcome::Leave { mode: "Main".into() }
        );
        // In the root, an unbound key is ordinary typing.
        assert_eq!(step(map, "Main".into(), "q".into()), Outcome::PassThrough);
    }

    /// @tests REQ-MYTH.whichkey_is_a_query
    #[test]
    fn the_help_is_computed_from_the_bindings() {
        let map = keymap();
        let keys: Vec<String> = which_key(&map, "Options").into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["f", "u", "Escape"]);
        // And it cannot describe a binding that does not exist.
        assert!(which_key(&map, "Nowhere").is_empty());
    }

    /// @tests REQ-MYTH.modes_defined
    /// @tests REQ-MYTH.actions_defined
    #[test]
    fn a_valid_keymap_reports_nothing() {
        assert_eq!(validate(&keymap(), &actions()), vec![]);
    }

    #[test]
    fn a_binding_to_a_missing_mode_is_reported_at_load() {
        let mut map = keymap();
        map.mode_mut("Main").unwrap().bind("x", enter("Ghost"));
        assert!(validate(&map, &actions())
            .iter()
            .any(|p| matches!(p, Problem::UndefinedMode { target, .. } if target == "Ghost")));
    }

    #[test]
    fn a_binding_to_a_missing_action_is_reported_at_load() {
        let mut map = keymap();
        map.mode_mut("Options").unwrap().bind("x", dispatch("nope"));
        assert!(validate(&map, &actions())
            .iter()
            .any(|p| matches!(p, Problem::UndefinedAction { action, .. } if action == "nope")));
    }

    /// @tests REQ-MYTH.escape_terminates
    #[test]
    fn a_parent_cycle_is_reported() {
        let mut map = keymap();
        map.mode_mut("Main").unwrap().parent = Some("File".into());
        assert!(validate(&map, &actions())
            .iter()
            .any(|p| matches!(p, Problem::ParentCycle { .. })));
    }

    /// @tests REQ-MYTH.actions_reachable
    #[test]
    fn an_action_no_key_sequence_reaches_is_reported() {
        let mut map = keymap();
        // Detach File from the graph: its action becomes unreachable.
        map.mode_mut("Options").unwrap().unbind("f");
        let problems = validate(&map, &actions());
        assert!(problems
            .iter()
            .any(|p| matches!(p, Problem::UnreachableMode { mode } if mode == "File")));
        assert!(problems
            .iter()
            .any(|p| matches!(p, Problem::UnreachableAction { action } if action == "open_file")));
    }

    #[test]
    fn a_missing_root_is_reported() {
        let map = Keymap { root: "Nowhere".into(), modes: Vec::new() };
        assert!(validate(&map, &BTreeSet::new())
            .iter()
            .any(|p| matches!(p, Problem::NoRoot { .. })));
    }
}
