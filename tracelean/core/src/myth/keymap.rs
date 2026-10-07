//! Keymap: a mode machine over the action registry (Myth phase 3).
//!
//! The keymap is data (ui_settings/keymap.json), interpreted in core so the
//! GUI and TUI share one behavior:
//!
//! ```jsonc
//! {
//!   "Main":    { "C-Space": "→Options", "S-Up": "go_parent", "NM": "passthrough" },
//!   "Options": { "Escape": "→Main", "f": "→File", "u": "undo", "NM": "→Main" }
//! }
//! ```
//!
//! - `→State` (or `->State`) = transition
//! - bare name = action registry lookup
//! - `NM` = fallback for keys with no explicit match ("no match")
//! - `passthrough` = let the frontend handle the key natively
//!
//! Which-key is a query over this data (`bindings_for_state`), not a feature.

use std::collections::HashMap;

use serde::Serialize;

pub const START_STATE: &str = "Main";
const NO_MATCH: &str = "NM";
const PASSTHROUGH: &str = "passthrough";
const POP: &str = "pop";
const RESET: &str = "reset";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum KeyResult {
    /// Enter another keymap state.
    Transition { state: String },
    /// Dispatch a registered action (state returns to the given state).
    Dispatch { action: String, next_state: String },
    /// Leave the current mode, returning to whichever mode opened it.
    ///
    /// This is what `Escape` does. Before the mode stack existed every mode
    /// mapped `Escape` to `→Main`, so `File` — which is entered *from*
    /// `Options` — dropped two levels while the README promised one. The
    /// stack makes the documented behaviour the implemented one.
    Pop,
    /// Abandon the whole stack and return to `Main` (the `NM` fallback in
    /// transient modes: an unrecognised key cancels the menu outright).
    Reset,
    /// The frontend should handle the key natively (e.g. typing in Main).
    Passthrough,
}

/// One entry for which-key display.
#[derive(Debug, Clone, Serialize)]
pub struct KeyBindingInfo {
    pub key: String,
    pub target: String,
    /// "transition" | "action" | "passthrough" | "pop" | "reset"
    pub kind: String,
    /// Human label, when one is known. Which-key showing `goto_provenance`
    /// teaches nothing; "Who wrote this line" teaches the feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Menu grouping, for modes whose entries come from a provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Payload to pass through `ActionCtx::args` when this entry is chosen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Keymap {
    states: HashMap<String, HashMap<String, String>>,
}

fn transition_target(target: &str) -> Option<&str> {
    target
        .strip_prefix('→')
        .or_else(|| target.strip_prefix("->"))
}

impl Keymap {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let states: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(json).map_err(|e| format!("keymap parse error: {}", e))?;
        Ok(Self { states })
    }

    /// Load from ui_settings/keymap.json (empty keymap if absent).
    pub fn load() -> Self {
        super::bindings::read_ui_settings_file("ui_settings/keymap.json")
            .and_then(|content| Self::from_json(&content).ok())
            .unwrap_or_default()
    }

    pub fn has_state(&self, state: &str) -> bool {
        self.states.contains_key(state)
    }

    /// Interpret one key event in a state.
    pub fn interpret(&self, state: &str, key: &str) -> KeyResult {
        let Some(bindings) = self.states.get(state) else {
            return KeyResult::Passthrough;
        };
        let target = bindings
            .get(key)
            .or_else(|| bindings.get(NO_MATCH))
            .map(|s| s.as_str());
        match target {
            None | Some(PASSTHROUGH) => KeyResult::Passthrough,
            Some(POP) => KeyResult::Pop,
            Some(RESET) => KeyResult::Reset,
            Some(t) => match transition_target(t) {
                // `→Main` is kept as a spelling of `reset` rather than a push:
                // Main is the floor of the stack, never a level above it.
                Some(next) if next == START_STATE => KeyResult::Reset,
                Some(next) => KeyResult::Transition { state: next.to_string() },
                None => KeyResult::Dispatch {
                    action: t.to_string(),
                    // Dispatching always returns to Main (transient modes end
                    // on action; Main stays Main).
                    next_state: START_STATE.to_string(),
                },
            },
        }
    }

    /// Which-key data: the bindings available in a state, sorted by key.
    pub fn bindings_for_state(&self, state: &str) -> Vec<KeyBindingInfo> {
        let mut out = Vec::new();
        if let Some(bindings) = self.states.get(state) {
            for (key, target) in bindings {
                if key == NO_MATCH {
                    continue;
                }
                let (kind, target_name) = match transition_target(target) {
                    // `→Main` is a reset, not a push (see `interpret`); which-key
                    // must label it the same way the machine treats it.
                    Some(t) if t == START_STATE => ("reset", t.to_string()),
                    Some(t) => ("transition", t.to_string()),
                    None if target == PASSTHROUGH => ("passthrough", target.clone()),
                    None if target == POP => ("pop", "back".to_string()),
                    None if target == RESET => ("reset", START_STATE.to_string()),
                    None => ("action", target.clone()),
                };
                let title = (kind == "action").then(|| super::provider::title_for(&target_name));
                out.push(KeyBindingInfo {
                    key: key.clone(),
                    target: target_name,
                    kind: kind.to_string(),
                    title,
                    group: None,
                    args: None,
                });
            }
        }
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    }


    /// Every defined mode, `Main` first then alphabetical.
    pub fn modes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.states.keys().map(|s| s.as_str()).collect();
        out.sort_by_key(|s| (*s != START_STATE, *s));
        out
    }

    /// Render the whole keymap as a markdown table.
    ///
    /// The README used to describe the keymap in prose, and drifted: it
    /// promised `Escape` in `File` went "back to Options" while the machine
    /// jumped to `Main`. Generating the table from the same JSON the machine
    /// reads makes that class of documentation bug impossible, and the test
    /// that compares the two turns a stale README into a failing build.
    pub fn markdown_table(&self) -> String {
        let mut out = String::from("| Mode | Key | Effect |\n|---|---|---|\n");
        for mode in self.modes() {
            let Some(bindings) = self.states.get(mode) else { continue };
            let mut keys: Vec<&String> = bindings.keys().collect();
            // `NM` last: it is the catch-all, and reads as one.
            keys.sort_by_key(|k| (k.as_str() == NO_MATCH, k.as_str()));
            for key in keys {
                let target = &bindings[key];
                let effect = match transition_target(target) {
                    Some(t) if t == START_STATE => "return to `Main`".to_string(),
                    Some(t) => format!("enter `{}` mode", t),
                    None if target == PASSTHROUGH => "handled by the editor (normal typing)".to_string(),
                    None if target == POP => "leave this mode (back one level)".to_string(),
                    None if target == RESET => "return to `Main`".to_string(),
                    None => format!("{} (`{}`)", super::provider::title_for(target), target),
                };
                let key_label = if key == NO_MATCH {
                    "any other key".to_string()
                } else {
                    format!("`{}`", key)
                };
                out.push_str(&format!("| `{}` | {} | {} |\n", mode, key_label, effect));
            }
        }
        out
    }

    /// Startup validation: every non-transition, non-passthrough target must
    /// be a registered action; every transition target must be a defined
    /// state. Returns the list of problems (empty = valid).
    pub fn validate(&self, registry: &super::actions::ActionRegistry) -> Vec<String> {
        let mut problems = Vec::new();
        for (state, bindings) in &self.states {
            for (key, target) in bindings {
                match transition_target(target) {
                    Some(t) => {
                        if !self.states.contains_key(t) {
                            problems.push(format!(
                                "{}.{}: transition to undefined state `{}`",
                                state, key, t
                            ));
                        }
                    }
                    None => {
                        if target != PASSTHROUGH
                            && target != POP
                            && target != RESET
                            && !registry.contains(target)
                        {
                            problems.push(format!(
                                "{}.{}: unknown action `{}`",
                                state, key, target
                            ));
                        }
                    }
                }
            }
        }
        problems.sort();
        problems
    }
}

/// Keys handed out to a dynamic mode's entries.
///
/// Home-row-first rather than `1 2 3 …`: a quick-fix list is read and chosen
/// from in one motion, and digits put the common case under the weakest
/// fingers. `Escape` is excluded because every mode already uses it to pop.
const DYNAMIC_KEYS: &[&str] = &[
    "a", "s", "d", "f", "g", "h", "j", "k", "l", "q", "w", "e", "r", "t", "y", "u", "i", "o", "p",
    "z", "x", "c", "v", "b", "n", "m",
];

/// Turn provider-supplied actions into which-key bindings for a dynamic mode
/// (`CodeActions`). Entries past the available keys are dropped *and reported*
/// — a menu that silently truncates is a menu that hides the fix you wanted.
pub fn dynamic_bindings(actions: &[super::provider::Action]) -> (Vec<KeyBindingInfo>, usize) {
    let shown = actions.len().min(DYNAMIC_KEYS.len());
    let bindings = actions
        .iter()
        .zip(DYNAMIC_KEYS.iter())
        .map(|(action, key)| KeyBindingInfo {
            key: key.to_string(),
            target: action.name.clone(),
            kind: "action".to_string(),
            title: Some(action.title.clone()),
            group: Some(action.group.clone()),
            args: action.args.clone(),
        })
        .collect();
    (bindings, actions.len() - shown)
}


/// The mode stack.
///
/// Modes nest: `Main` → `Options` → `File`. Entering pushes, `Escape` pops one
/// level, dispatching an action or an unrecognised key resets to `Main`. The
/// floor is always `Main`, so popping an empty stack is a no-op rather than an
/// error state the UI has to handle.
#[derive(Debug, Clone, Serialize)]
pub struct ModeStack {
    stack: Vec<String>,
}

impl Default for ModeStack {
    fn default() -> Self {
        ModeStack { stack: vec![START_STATE.to_string()] }
    }
}

impl ModeStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// The mode the user is in right now — what which-key renders.
    pub fn current(&self) -> &str {
        self.stack.last().map(|s| s.as_str()).unwrap_or(START_STATE)
    }

    /// The path from `Main` to the current mode, for a breadcrumb
    /// ("Options › File") so a nested mode is legible without memorising how
    /// you got there.
    pub fn path(&self) -> &[String] {
        &self.stack
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn push(&mut self, state: &str) {
        if state == START_STATE {
            self.reset();
        } else {
            self.stack.push(state.to_string());
        }
    }

    pub fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    pub fn reset(&mut self) {
        self.stack.truncate(1);
        self.stack[0] = START_STATE.to_string();
    }

    /// Apply one interpreted key result, returning the new current mode.
    pub fn apply(&mut self, result: &KeyResult) -> &str {
        match result {
            KeyResult::Transition { state } => self.push(state),
            KeyResult::Pop => self.pop(),
            KeyResult::Reset => self.reset(),
            KeyResult::Dispatch { next_state, .. } => self.push(next_state),
            KeyResult::Passthrough => {}
        }
        self.current()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Keymap {
        Keymap::from_json(
            r#"{
                "Main": { "C-Space": "→Options", "S-ArrowUp": "go_parent", "NM": "passthrough" },
                "Options": { "Escape": "→Main", "f": "→File", "u": "undo", "NM": "→Main" },
                "File": { "o": "open_file", "NM": "→Main" }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn transition_and_dispatch() {
        let km = sample();
        assert_eq!(
            km.interpret("Main", "C-Space"),
            KeyResult::Transition { state: "Options".into() }
        );
        assert_eq!(
            km.interpret("Options", "u"),
            KeyResult::Dispatch { action: "undo".into(), next_state: "Main".into() }
        );
        assert_eq!(
            km.interpret("Options", "f"),
            KeyResult::Transition { state: "File".into() }
        );
    }

    #[test]
    fn fallback_and_passthrough() {
        let km = sample();
        // Main NM = passthrough: typing works
        assert_eq!(km.interpret("Main", "x"), KeyResult::Passthrough);
        // Options NM = →Main: unknown key abandons the mode entirely
        assert_eq!(km.interpret("Options", "z"), KeyResult::Reset);
        // Unknown state: passthrough
        assert_eq!(km.interpret("Nope", "x"), KeyResult::Passthrough);
    }

    #[test]
    fn which_key_lists_bindings() {
        let km = sample();
        let b = km.bindings_for_state("Options");
        assert_eq!(b.len(), 3); // Escape, f, u (NM hidden)
        assert!(b.iter().any(|x| x.key == "u" && x.kind == "action"));
        assert!(b.iter().any(|x| x.key == "Escape" && x.kind == "reset"));
        assert!(b.iter().any(|x| x.key == "f" && x.kind == "transition" && x.target == "File"));
    }

    #[test]
    fn validation_catches_unknowns() {
        let km = Keymap::from_json(
            r#"{ "Main": { "a": "no_such_action", "b": "→NoSuchState" } }"#,
        )
        .unwrap();
        let problems = km.validate(super::super::actions::registry());
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().any(|p| p.contains("no_such_action")));
        assert!(problems.iter().any(|p| p.contains("NoSuchState")));
    }

    #[test]
    fn shipped_keymap_is_valid() {
        let km = Keymap::load();
        assert!(km.has_state(START_STATE), "keymap.json must define Main");
        let problems = km.validate(super::super::actions::registry());
        assert!(problems.is_empty(), "keymap problems: {:?}", problems);
    }

    #[test]
    fn main_state_has_discoverable_bindings() {
        // Regression guard for the which-key discoverability bug
        // (new_features_work_plan.md #3): Main is the resting state and the
        // only one containing the leader key(s) into every other mode, so it
        // must never be the one state with nothing to show — the frontend
        // used to hide the which-key bar specifically because it treated an
        // empty/discarded Main binding set as the normal case.
        let km = Keymap::load();
        let bindings = km.bindings_for_state(START_STATE);
        assert!(
            !bindings.is_empty(),
            "Main must expose at least one discoverable binding (e.g. a leader key)"
        );
        assert!(
            bindings.iter().any(|b| b.kind == "transition"),
            "Main should offer at least one transition into another mode: {:?}",
            bindings
        );
    }

    /// Path of the README that documents the keymap.
    fn readme_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("README.md")
    }

    const BEGIN: &str = "<!-- keymap:begin (generated from tracelean/ui_settings/keymap.json) -->";
    const END: &str = "<!-- keymap:end -->";

    #[test]
    fn readme_keymap_table_matches_keymap_json() {
        // The README's keymap table is generated, not written. Run
        // `TRACELEAN_UPDATE_README=1 cargo test -p tracelean-core readme_keymap`
        // after editing keymap.json to regenerate it.
        let path = readme_path();
        let readme = std::fs::read_to_string(&path).expect("README.md must exist");
        let table = Keymap::load().markdown_table();
        let block = format!("{}\n\n{}\n{}", BEGIN, table, END);

        let start = readme.find(BEGIN).expect("README must contain the keymap:begin marker");
        let end = readme.find(END).expect("README must contain the keymap:end marker") + END.len();
        let current = &readme[start..end];

        if current == block {
            return;
        }
        if std::env::var("TRACELEAN_UPDATE_README").is_ok() {
            let updated = format!("{}{}{}", &readme[..start], block, &readme[end..]);
            std::fs::write(&path, updated).expect("README must be writable");
            return;
        }
        panic!(
            "README keymap table is stale. Regenerate with \
             TRACELEAN_UPDATE_README=1 cargo test -p tracelean-core readme_keymap"
        );
    }
}
