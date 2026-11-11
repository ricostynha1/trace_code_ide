use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tracelean_core::surface::setting::{self, LoadError, KINDS};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn read(name: &str) -> String {
    std::fs::read_to_string(root().join(name)).unwrap_or_else(|_| panic!("{name} is shipped"))
}

/// Every `section.key` with a string value in a shipped theme.
fn names(theme: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (section, entries) in theme.as_object().unwrap() {
        if let Value::Object(entries) = entries {
            for (key, value) in entries {
                if let Some(value) = value.as_str() {
                    out.push((format!("{section}.{key}"), value.to_string()));
                }
            }
        }
    }
    out
}

/// @tests REQ-LOOK.settings_declared
#[test]
fn the_shipped_grammar_holds_together_and_every_kind_it_names_is_implemented_and_used() {
    let settings = setting::load(&read("assets/settings.json")).unwrap_or_else(|e| panic!("{e:?}"));
    let named: BTreeSet<&str> = settings.settings.iter().map(|r| r.kind.as_str()).collect();
    let implemented: BTreeSet<&str> = KINDS.iter().copied().collect();
    assert_eq!(named, implemented, "a kind is implemented and never used, or used and not implemented");
}

/// @tests REQ-LOOK.settings_declared
#[test]
fn every_setting_a_shipped_theme_holds_is_declared_and_holds_a_value_its_rule_accepts_and_every_rule_is_used() {
    let settings = setting::shipped();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for file in ["assets/theme.json", "assets/theme-light.json"] {
        let theme: Value = serde_json::from_str(&read(file)).unwrap();
        for (name, value) in names(&theme) {
            let rule = settings.rule(&name).unwrap_or_else(|| panic!("{file}: `{name}` is not declared in assets/settings.json"));
            assert!(rule.accepts(&value), "{file}: `{name}` = `{value}` is not {}", rule.wanted());
            used.insert(rule.name.clone());
        }
    }
    for rule in &settings.settings {
        assert!(used.contains(&rule.name), "`{}` is declared and no shipped theme has a setting it covers", rule.name);
    }
}

/// @tests REQ-LOOK.settings_declared
#[test]
fn a_grammar_that_does_not_hold_together_is_refused_when_it_is_read() {
    let text = read("assets/settings.json");
    let refused = |bad: String, mention: &str| match setting::load(&bad) {
        Err(LoadError::Invalid { problems }) => {
            assert!(problems.iter().any(|p| p.contains(mention)), "{mention} not named: {problems:#?}")
        }
        other => panic!("a broken grammar loaded: {other:?}"),
    };
    refused(text.replace("\"percent\"", "\"percentage\""), "percentage");
    refused(text.replace("\"default\": \"shown\"", "\"default\": \"sideways\""), "sideways");
    refused(text.replace("\"layout.zoom\"", "\"layout.menuBar\""), "declared twice");
    refused(text.replace("\"ui.*\"", "\"ui\""), "is not `section.key`");
    match setting::load("{ not json") {
        Err(LoadError::Unreadable { .. }) => {}
        other => panic!("unreadable text loaded: {other:?}"),
    }
}

/// @tests REQ-LOOK.settings_declared
#[test]
fn the_zoom_the_grammar_declares_is_the_zoom_the_code_steps() {
    let settings = setting::shipped();
    let rule = settings.rule("layout.zoom").unwrap();
    let (least, most, step) = setting::ZOOM;
    assert_eq!((rule.min, rule.max, rule.step), (Some(least), Some(most), Some(step)));
}
