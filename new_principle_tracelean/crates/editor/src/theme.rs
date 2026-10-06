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

    #[test]
    fn the_shipped_theme_names_every_role() {
        let theme: Value = serde_json::from_str(SHIPPED).unwrap();
        for role in ["plain", "path", "entry", "heading", "requirement", "level", "added", "removed"]
        {
            assert!(colour(&theme, "roles", role).is_some(), "no colour for `{role}`");
        }
        for grade in ["levelL1", "levelL2", "levelL3", "levelL4"] {
            assert!(colour(&theme, "roles", grade).is_some(), "no colour for `{grade}`");
        }
    }

    #[test]
    fn an_override_replaces_only_what_it_names() {
        let mut theme: Value = serde_json::from_str(SHIPPED).unwrap();
        merge(&mut theme, serde_json::json!({ "ui": { "accent": "#ff0000" } }));
        assert_eq!(colour(&theme, "ui", "accent"), Some("#ff0000"));
        assert!(colour(&theme, "ui", "sidebar").is_some(), "the rest of the section survived");
    }
}
