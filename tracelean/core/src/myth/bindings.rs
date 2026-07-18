//! Binding maps: capture name → actions (Myth phase 1).
//!
//! `ui_settings/bindings.json` maps capture names (same names the highlight
//! queries produce, plus surface captures like `@file` / `@dir`) to the list
//! of actions available on nodes with that capture. Resolution uses the same
//! longest-prefix fallback as the theme map (`function.call` → `function`).
//! Merge rule (fable Q5): style = last capture wins; actions = union in
//! capture order.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Search the standard ui_settings candidate locations for a file.
pub(crate) fn read_ui_settings_file(filename: &str) -> Option<String> {
    let candidates = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(filename))),
        Some(PathBuf::from("/app").join(filename)),
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or(Path::new("."))
                .join(filename),
        ),
        Some(PathBuf::from(filename)),
    ];
    for candidate in candidates.iter().flatten() {
        if let Ok(content) = fs::read_to_string(candidate) {
            return Some(content);
        }
    }
    None
}

static BINDING_MAP: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

fn load_binding_map() -> HashMap<String, Vec<String>> {
    read_ui_settings_file("ui_settings/bindings.json")
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn get_binding_map() -> &'static HashMap<String, Vec<String>> {
    BINDING_MAP.get_or_init(load_binding_map)
}

/// Resolve a capture name to its actions with longest-prefix fallback.
pub fn actions_for_capture(capture_name: &str) -> Vec<String> {
    let map = get_binding_map();
    let mut name = capture_name.trim_start_matches('@');
    loop {
        if let Some(actions) = map.get(name) {
            return actions.clone();
        }
        match name.rfind('.') {
            Some(pos) => name = &name[..pos],
            None => return Vec::new(),
        }
    }
}

/// Union the actions of several captures, preserving capture order and
/// deduplicating (fable Q5: actions merge by union, query order = menu order).
pub fn union_actions(capture_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cap in capture_names {
        for action in actions_for_capture(cap) {
            if !out.contains(&action) {
                out.push(action);
            }
        }
    }
    out
}

/// Startup validation: every action named in the binding map must exist in
/// the registry. Returns the list of unknown names (empty = valid).
pub fn validate_against_registry(registry: &super::actions::ActionRegistry) -> Vec<String> {
    let mut unknown = Vec::new();
    for actions in get_binding_map().values() {
        for a in actions {
            if !registry.contains(a) && !unknown.contains(a) {
                unknown.push(a.clone());
            }
        }
    }
    unknown
}
