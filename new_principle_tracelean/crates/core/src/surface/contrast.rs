//! @implements REQ-LOOK.contrast_sufficient

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    pub text: String,
    pub on: String,
    pub least: u32,
    #[serde(default)]
    pub waived: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub text: String,
    pub on: String,
    pub ratio: Option<u32>,
    pub least: u32,
    pub problem: String,
}

fn channels(colour: &str) -> Option<[u8; 3]> {
    let hex = colour.strip_prefix('#')?;
    if hex.chars().count() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn linear(n: u8) -> f64 {
    let c = f64::from(n) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance([r, g, b]: [u8; 3]) -> f64 {
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

pub fn ratio(a: &str, b: &str) -> Option<u32> {
    let (x, y) = (luminance(channels(a)?), luminance(channels(b)?));
    let (hi, lo) = if x < y { (y, x) } else { (x, y) };
    Some((((hi + 0.05) / (lo + 0.05)) * 100.0).floor() as u32)
}

/// @implements REQ-LOOK.contrast_sufficient
/// @drt REQ-LOOK.contrast_sufficient
pub fn findings(pairs: Vec<Pair>, colours: Vec<(String, String)>) -> Vec<Finding> {
    let colour = |key: &str| colours.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let mut out = Vec::new();
    for pair in pairs {
        let found = |ratio: Option<u32>, problem: &str| Finding {
            text: pair.text.clone(),
            on: pair.on.clone(),
            ratio,
            least: pair.least,
            problem: problem.to_string(),
        };
        let (Some(a), Some(b)) = (colour(&pair.text), colour(&pair.on)) else {
            out.push(found(None, "unknown colour"));
            continue;
        };
        match ratio(a, b) {
            None => out.push(found(None, "not a colour")),
            Some(r) if r < pair.least && pair.waived.is_none() => out.push(found(Some(r), "below its minimum")),
            Some(r) if r >= pair.least && pair.waived.is_some() => out.push(found(Some(r), "waiver unused")),
            Some(_) => {}
        }
    }
    out
}

pub fn of_theme(theme: &serde_json::Value) -> (Vec<Pair>, Vec<(String, String)>) {
    let mut colours = Vec::new();
    if let Some(sections) = theme.as_object() {
        for (section, entries) in sections {
            let Some(entries) = entries.as_object() else { continue };
            for (key, value) in entries {
                if let Some(value) = value.as_str() {
                    colours.push((format!("{section}.{key}"), value.to_string()));
                }
            }
        }
    }
    let pairs = theme
        .get("contrast")
        .cloned()
        .and_then(|p| serde_json::from_value(p).ok())
        .unwrap_or_default();
    (pairs, colours)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../assets/theme.json")).unwrap()
    }

    #[test]
    fn the_ratio_is_wcags() {
        assert_eq!(ratio("#000000", "#ffffff"), Some(2100));
        assert_eq!(ratio("#ffffff", "#ffffff"), Some(100));
        assert_eq!(ratio("#777777", "#ffffff"), Some(447));
        assert_eq!(ratio("#fff", "#000000"), None);
    }

    /// @tests REQ-LOOK.contrast_sufficient
    #[test]
    fn the_theme_in_use_is_readable_or_says_why_not() {
        let (pairs, colours) = of_theme(&theme());
        assert!(pairs.len() > 20, "the theme declares only {} pairs", pairs.len());
        let found = findings(pairs, colours);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// @tests REQ-LOOK.contrast_sufficient
    #[test]
    fn an_unreadable_pair_an_unknown_colour_and_a_stale_waiver_are_found() {
        let colours = vec![
            ("ui.document".to_string(), "#282c34".to_string()),
            ("ui.dim".to_string(), "#2a2e36".to_string()),
            ("ui.text".to_string(), "#ffffff".to_string()),
        ];
        let pair = |text: &str, waived: Option<&str>| Pair {
            text: text.into(),
            on: "ui.document".into(),
            least: 450,
            waived: waived.map(str::to_string),
        };
        let found = findings(
            vec![pair("ui.dim", None), pair("ui.missing", None), pair("ui.text", Some("old")), pair("ui.text", None)],
            colours,
        );
        let problems: Vec<&str> = found.iter().map(|f| f.problem.as_str()).collect();
        assert_eq!(problems, vec!["below its minimum", "unknown colour", "waiver unused"]);
    }
}
