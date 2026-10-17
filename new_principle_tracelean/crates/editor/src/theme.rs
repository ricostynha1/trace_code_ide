//! How things look, read from a file rather than written into a frontend.
//!
//! A role says what a region *is* and the core decides it; what colour that is
//! belongs to whoever is looking. So the colours live in JSON: the shipped
//! `assets/theme.json`, with anything in the project's `.tracelean/theme.json`
//! laid over it key by key. Changing a colour is editing a file, and both
//! frontends read the same one, so the terminal and the window cannot drift.
//!
//! Nothing here decides what is on screen: a theme can only say how a role, a
//! token kind or a piece of chrome is painted.

use std::path::Path;

use serde_json::Value;

/// The theme the project ships, compiled in so a frontend always has one.
pub const SHIPPED: &str = include_str!("../../../assets/theme.json");

/// Where a project overrides it.
pub const OVERRIDE: &str = ".tracelean/theme.json";

/// The shipped theme with the project's override laid over it.
///
/// An override that is missing is the ordinary case; one that does not parse
/// is ignored rather than allowed to take the editor down with it, because a
/// typo in a colour should cost a colour and not the session.
///
/// @implements REQ-LOOK.roles_drawn_in_theme_colours
pub fn load(root: &Path) -> Value {
    let mut theme: Value = serde_json::from_str(SHIPPED).expect("the shipped theme parses");
    if let Ok(text) = std::fs::read_to_string(root.join(OVERRIDE)) {
        if let Ok(over) = serde_json::from_str::<Value>(&text) {
            merge(&mut theme, over);
        }
    }
    theme
}

/// Lay `over` onto `base`: objects merge key by key, anything else replaces.
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                merge(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, over) => *base = over,
    }
}

/// A colour from one section of the theme, as `#rrggbb`.
pub fn colour<'a>(theme: &'a Value, section: &str, key: &str) -> Option<&'a str> {
    theme.get(section)?.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-LOOK.roles_drawn_in_theme_colours
    #[test]
    fn the_shipped_theme_names_every_role() {
        let theme: Value = serde_json::from_str(SHIPPED).unwrap();
        for role in ["plain", "path", "entry", "heading", "requirement", "added", "removed"] {
            assert!(colour(&theme, "roles", role).is_some(), "no colour for `{role}`");
        }
        for mark in ["markAnnotated", "markJudged", "markTested", "markProved", "markMissing"] {
            assert!(colour(&theme, "roles", mark).is_some(), "no colour for `{mark}`");
        }
    }

    #[test]
    fn an_override_replaces_only_what_it_names() {
        let mut theme: Value = serde_json::from_str(SHIPPED).unwrap();
        merge(&mut theme, serde_json::json!({ "ui": { "accent": "#ff0000" } }));
        assert_eq!(colour(&theme, "ui", "accent"), Some("#ff0000"));
        assert!(colour(&theme, "ui", "sidebar").is_some(), "the rest of the section survived");
    }

    /// A project's override is laid over the shipped theme; one that does not
    /// parse costs nothing, and no override is the shipped theme.
    ///
    /// @tests REQ-LOOK.roles_drawn_in_theme_colours
    #[test]
    fn a_projects_override_is_read_and_a_broken_one_ignored() {
        let root = std::env::temp_dir().join(format!("tracelean-theme-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".tracelean")).unwrap();
        let shipped: Value = serde_json::from_str(SHIPPED).unwrap();
        assert_eq!(load(&root), shipped);
        std::fs::write(root.join(OVERRIDE), r##"{"ui":{"accent":"#123456"}}"##).unwrap();
        assert_eq!(colour(&load(&root), "ui", "accent"), Some("#123456"));
        std::fs::write(root.join(OVERRIDE), "{ not json").unwrap();
        assert_eq!(load(&root), shipped);
        let _ = std::fs::remove_dir_all(&root);
    }
}
