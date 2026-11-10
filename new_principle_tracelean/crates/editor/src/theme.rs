use std::path::Path;

use serde_json::Value;

pub const SHIPPED: &str = include_str!("../../../assets/theme.json");

pub const SHIPPED_LIGHT: &str = include_str!("../../../assets/theme-light.json");

pub const OVERRIDE: &str = ".tracelean/theme.json";

pub const BASE: &str = "base";

fn over(root: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(root.join(OVERRIDE)).ok()?).ok()
}

/// Which shipped theme the project's override builds on: `dark` unless it says `light`.
pub fn base(root: &Path) -> &'static str {
    match over(root).as_ref().and_then(|o| o.get(BASE)).and_then(Value::as_str) {
        Some("light") => "light",
        _ => "dark",
    }
}

/// @implements REQ-LOOK.theme_switched
pub fn switch_base(root: &Path) -> Result<&'static str, String> {
    let next = if base(root) == "light" { "dark" } else { "light" };
    let path = root.join(OVERRIDE);
    let mut over = over(root).filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    over[BASE] = Value::String(next.to_string());
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(&over).map_err(|error| error.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|error| error.to_string())?;
    Ok(next)
}

/// @implements REQ-LOOK.roles_drawn_in_theme_colours
pub fn load(root: &Path) -> Value {
    let shipped = if base(root) == "light" { SHIPPED_LIGHT } else { SHIPPED };
    let mut theme: Value = serde_json::from_str(shipped).expect("the shipped theme parses");
    if let Some(over) = over(root) {
        merge(&mut theme, over);
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

    /// @tests REQ-LOOK.theme_switched
    #[test]
    fn switching_flips_the_base_and_keeps_the_projects_own_colours() {
        let root = std::env::temp_dir().join(format!("tracelean-base-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".tracelean")).unwrap();
        std::fs::write(root.join(OVERRIDE), r##"{"ui":{"accent":"#123456"}}"##).unwrap();
        let dark: Value = serde_json::from_str(SHIPPED).unwrap();
        let light: Value = serde_json::from_str(SHIPPED_LIGHT).unwrap();
        assert_eq!(colour(&load(&root), "ui", "document"), colour(&dark, "ui", "document"));
        assert_eq!(switch_base(&root), Ok("light"));
        assert_eq!(colour(&load(&root), "ui", "document"), colour(&light, "ui", "document"));
        assert_eq!(colour(&load(&root), "ui", "accent"), Some("#123456"));
        assert_eq!(switch_base(&root), Ok("dark"));
        assert_eq!(colour(&load(&root), "ui", "document"), colour(&dark, "ui", "document"));
        let keys = |v: &Value, s: &str| v[s].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        for section in ["ui", "roles", "chips", "modes", "syntax", "fonts"] {
            assert_eq!(keys(&dark, section), keys(&light, section), "{section}");
        }
        let _ = std::fs::remove_dir_all(&root);
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
