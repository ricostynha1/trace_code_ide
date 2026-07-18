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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum KeyResult {
    /// Enter another keymap state.
    Transition { state: String },
    /// Dispatch a registered action (state returns to the given state).
    Dispatch { action: String, next_state: String },
    /// The frontend should handle the key natively (e.g. typing in Main).
    Passthrough,
}

/// One entry for which-key display.
#[derive(Debug, Clone, Serialize)]
pub struct KeyBindingInfo {
    pub key: String,
    pub target: String,
    /// "transition" | "action" | "passthrough"
    pub kind: String,
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
            Some(t) => match transition_target(t) {
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
                    Some(t) => ("transition", t.to_string()),
                    None if target == PASSTHROUGH => ("passthrough", target.clone()),
                    None => ("action", target.clone()),
                };
                out.push(KeyBindingInfo {
                    key: key.clone(),
                    target: target_name,
                    kind: kind.to_string(),
                });
            }
        }
        out.sort_by(|a, b| a.key.cmp(&b.key));
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
                        if target != PASSTHROUGH && !registry.contains(target) {
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
        // Options NM = →Main: unknown key exits the mode
        assert_eq!(
            km.interpret("Options", "z"),
            KeyResult::Transition { state: "Main".into() }
        );
        // Unknown state: passthrough
        assert_eq!(km.interpret("Nope", "x"), KeyResult::Passthrough);
    }

    #[test]
    fn which_key_lists_bindings() {
        let km = sample();
        let b = km.bindings_for_state("Options");
        assert_eq!(b.len(), 3); // Escape, f, u (NM hidden)
        assert!(b.iter().any(|x| x.key == "u" && x.kind == "action"));
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
}
