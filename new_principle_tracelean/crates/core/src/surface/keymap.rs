use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// @implements REQ-MYTH.outcomes_closed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Enter { mode: String },
    Dispatch { action: String },
    Leave { mode: String },
    PassThrough,
}

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
    #[serde(default)]
    pub parent: Option<String>,
    pub bindings: Vec<(String, Binding)>,
}

/// @implements REQ-MYTH.keymap_is_data
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub root: String,
    pub modes: Vec<(String, Mode)>,
}

pub const LEAVE: &str = "Escape";

pub const SPACE: &str = "Space";

/// @implements REQ-MYTH.actions_reachable
pub const NAMED: &[&str] =
    &["Backspace", "Down", "Enter", LEAVE, "Left", "Right", SPACE, "Tab", "Up"];

/// @implements REQ-MYTH.actions_reachable
/// @implements ARCH-CORE-SHELL.shell_thin
pub fn typed(character: char) -> String {
    if character == ' ' {
        SPACE.to_string()
    } else {
        character.to_string()
    }
}

/// @implements REQ-MYTH.actions_defined
pub const ACTIONS: &[&str] = &[
    "context.copy",
    "context.toggle",
    "definitions.edit",
    "design.expand_all",
    "design.expand_everything",
    "design.fold_all",
    "design.open_only",
    "design.toggle",
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
    "file.find",
    "file.find_lines",
    "file.new",
    "file.open",
    "file.references",
    "file.rename",
    "file.save",
    "history.branch",
    "history.filter",
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
    "screen.station.definitions",
    "screen.station.design",
    "screen.station.findings",
    "screen.station.history",
    "screen.station.project",
    "screen.station.proof",
    "screen.station.sandbox",
    "screen.station.trace",
    "screen.strip",
    "trace.approve",
    "trace.check",
    "trace.context",
    "trace.coverage",
    "trace.evidence",
    "trace.findings",
    "trace.judge",
    "trace.lock",
    "trace.new_requirement",
    "trace.requirement",
    "trace.rollup",
    "trace.stale",
];

pub fn actions() -> BTreeSet<String> {
    ACTIONS.iter().map(|name| name.to_string()).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Unreadable { message: String },
    Invalid { problems: Vec<Problem> },
}

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

    pub fn bind(&mut self, key: &str, binding: Binding) {
        self.bindings.retain(|(k, _)| k != key);
        self.bindings.push((key.to_string(), binding));
        self.bindings.sort_by(|a, b| a.0.cmp(&b.0));
    }

    pub fn unbind(&mut self, key: &str) {
        self.bindings.retain(|(k, _)| k != key);
    }
}

/// @implements REQ-MYTH.totality
/// @implements REQ-MYTH.escape_pops_one
/// @implements REQ-MYTH.outcomes_closed
/// @drt REQ-MYTH.totality
/// @drt REQ-MYTH.escape_pops_one
/// @drt REQ-MYTH.outcomes_closed
pub fn step(keymap: Keymap, mode: String, key: String) -> Outcome {
    step_in(&keymap, &mode, &key)
}

fn step_in(keymap: &Keymap, mode: &str, key: &str) -> Outcome {
    let Some(current) = keymap.mode(mode) else {
        return Outcome::PassThrough;
    };

    if let Some(binding) = current.binding(key) {
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Problem {
    UndefinedMode { mode: String, key: String, target: String },
    UndefinedAction { mode: String, key: String, action: String },
    ParentCycle { mode: String },
    UnreachableMode { mode: String },
    UnreachableAction { action: String },
    NoRoot { root: String },
    UndefinedParent { mode: String, parent: String },
    Stranded { mode: String },
}

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

        if let Some(parent) = &mode.parent {
            if !keymap.has_mode(parent) {
                problems.push(Problem::UndefinedParent {
                    mode: name.clone(),
                    parent: parent.clone(),
                });
            }
        }

        let mut seen = BTreeSet::from([name.clone()]);
        let mut cursor = name.clone();
        loop {
            let target = match step_in(keymap, &cursor, LEAVE) {
                Outcome::Dispatch { .. } => break,
                Outcome::PassThrough => {
                    if cursor != keymap.root {
                        problems.push(Problem::Stranded { mode: name.clone() });
                    }
                    break;
                }
                Outcome::Enter { mode: target } | Outcome::Leave { mode: target } => target,
            };
            if !seen.insert(target.clone()) {
                problems.push(Problem::ParentCycle { mode: name.clone() });
                break;
            }
            cursor = target;
        }
    }

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

    /// @tests REQ-MYTH.totality
    #[test]
    fn every_key_in_every_mode_has_an_outcome() {
        let map = keymap();
        for (mode, _) in map.modes.clone() {
            for key in ["a", "z", "C-.", "u", "f", "o", "Escape", "F13", "€"] {
                let outcome = step(map.clone(), mode.clone(), key.to_string());
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

    #[test]
    fn an_unbound_key_in_a_nested_mode_returns_to_the_root() {
        let map = keymap();
        assert_eq!(
            step(map.clone(), "Options".into(), "q".into()),
Outcome::Leave { mode: "Main".into() }
        );
        assert_eq!(step(map, "Main".into(), "q".into()), Outcome::PassThrough);
    }

    /// @tests REQ-MYTH.whichkey_is_a_query
    #[test]
    fn the_help_is_computed_from_the_bindings() {
        let map = keymap();
        let keys: Vec<String> = which_key(&map, "Options").into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["f", "u", "Escape"]);
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

    /// @tests REQ-MYTH.escape_terminates
    /// @tests REQ-MYTH.modes_defined
    #[test]
    fn a_mode_whose_leaving_reaches_nowhere_is_reported() {
        let mut map = keymap();
        map.mode_mut("File").unwrap().parent = None;
        assert_eq!(validate(&map, &actions()), vec![Problem::Stranded { mode: "File".into() }]);

        let mut map = keymap();
        map.mode_mut("File").unwrap().parent = Some("Ghost".into());
        assert_eq!(
            validate(&map, &actions()),
            vec![
                Problem::UndefinedParent { mode: "File".into(), parent: "Ghost".into() },
                Problem::Stranded { mode: "File".into() },
            ]
        );

        let mut map = keymap();
        map.mode_mut("File").unwrap().parent = None;
        map.mode_mut("File").unwrap().bind(LEAVE, enter("Main"));
        assert_eq!(validate(&map, &actions()), vec![]);
    }

    /// @tests REQ-MYTH.actions_reachable
    #[test]
    fn an_action_no_key_sequence_reaches_is_reported() {
        let mut map = keymap();
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
