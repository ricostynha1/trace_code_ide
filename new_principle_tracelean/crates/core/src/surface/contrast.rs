//! Whether the colours the theme draws together can be read.
//!
//! The theme declares, beside its colours, which pairs are drawn together —
//! text on a background — and the least contrast each needs: 4.5 for text, 3
//! for large text and marks that are not text, as WCAG has them. The ratio is
//! WCAG's, from relative luminance, computed from `theme.json` itself, so an
//! edit that makes a role unreadable fails a test instead of a person's eyes.
//! A pair the theme keeps below its minimum says so with a reason; a reason on
//! a pair that no longer needs one is reported too.
//!
//! @implements REQ-LOOK.contrast_sufficient

use serde::{Deserialize, Serialize};

/// Two colours drawn together, by their keys in the theme (`ui.document`,
/// `roles.added`), and the least ratio allowed, in hundredths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    pub text: String,
    pub on: String,
    pub least: u32,
    /// Why the theme keeps this pair below its minimum.
    #[serde(default)]
    pub waived: Option<String>,
}

/// What is wrong with one pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub text: String,
    pub on: String,
    /// The ratio in hundredths, when both are colours.
    pub ratio: Option<u32>,
    pub least: u32,
    /// `unknown colour`, `not a colour`, `below its minimum` or `waiver unused`.
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

/// The contrast ratio of two `#rrggbb` colours in hundredths, rounded down.
pub fn ratio(a: &str, b: &str) -> Option<u32> {
    let (x, y) = (luminance(channels(a)?), luminance(channels(b)?));
    let (hi, lo) = if x < y { (y, x) } else { (x, y) };
    Some((((hi + 0.05) / (lo + 0.05)) * 100.0).floor() as u32)
}

/// Everything wrong with `pairs`, in their order, over `colours` (each key and
/// its value).
///
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

/// The theme's colours as `section.key` and value, and the pairs it declares.
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

    /// The theme in use declares the pairs its frontends draw, and every one
    /// meets its minimum or says why not.
    ///
    /// @tests REQ-LOOK.contrast_sufficient
    #[test]
    fn the_theme_in_use_is_readable_or_says_why_not() {
        let (pairs, colours) = of_theme(&theme());
        assert!(pairs.len() > 20, "the theme declares only {} pairs", pairs.len());
        let found = findings(pairs, colours);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// A pair made unreadable, an unknown key and a stale waiver are each found.
    ///
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
