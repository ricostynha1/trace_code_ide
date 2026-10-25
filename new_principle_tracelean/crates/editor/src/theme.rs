use std::path::Path;

use serde_json::Value;

pub const SHIPPED: &str = include_str!("../../../assets/theme.json");

pub const OVERRIDE: &str = ".tracelean/theme.json";

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

pub fn write_setting(root: &Path, section: &str, key: &str, value: &str) -> Result<(), String> {
    let path = root.join(OVERRIDE);
    let mut over: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    over[section][key] = Value::String(value.to_string());
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&over).map_err(|error| error.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|error| error.to_string())
}

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
