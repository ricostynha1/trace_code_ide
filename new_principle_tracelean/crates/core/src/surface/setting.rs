/// @implements REQ-LOOK.setting_edited_in_place
pub fn is_colour(value: String) -> bool {
    let Some(digits) = value.strip_prefix('#') else { return false };
    matches!(digits.chars().count(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit())
}

use serde::{Deserialize, Serialize};

/// The kinds of value a setting can be declared to hold. Each is a behaviour written in
/// Rust (`Rule::accepts`); `assets/settings.json` names one per setting, and a name that is
/// not here is refused when the file is read. A test holds the two lists to each other.
///
/// @implements REQ-LOOK.settings_declared
pub const KINDS: &[&str] = &["choice", "colour", "percent", "text"];

/// One declared setting, or a family of them (`ui.*`): what it is, what it may hold.
///
/// @implements REQ-LOOK.settings_declared
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    #[serde(default)]
    pub step: Option<i64>,
    #[serde(default)]
    pub default: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub settings: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Unreadable { message: String },
    Invalid { problems: Vec<String> },
}

/// The shipped grammar of the settings.
pub const SHIPPED: &str = include_str!("../../../../assets/settings.json");

/// Read a grammar of settings, refusing one that does not hold together.
///
/// @implements REQ-LOOK.settings_declared
pub fn load(text: &str) -> Result<Settings, LoadError> {
    let settings: Settings = serde_json::from_str(text)
        .map_err(|error| LoadError::Unreadable { message: error.to_string() })?;
    let problems = validate(&settings);
    if problems.is_empty() { Ok(settings) } else { Err(LoadError::Invalid { problems }) }
}

/// The shipped grammar, which a test holds to be valid.
pub fn shipped() -> Settings {
    load(SHIPPED).expect("the shipped settings grammar holds together")
}

fn validate(settings: &Settings) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for rule in &settings.settings {
        let name = &rule.name;
        let shape_ok = name
            .split_once('.')
            .is_some_and(|(section, key)| !section.is_empty() && !key.is_empty() && !section.contains('*'));
        if !shape_ok || (name.contains('*') && !name.ends_with(".*")) {
            problems.push(format!("`{name}` is not `section.key` or `section.*`"));
        }
        if !seen.insert(name.clone()) {
            problems.push(format!("`{name}` is declared twice"));
        }
        if !KINDS.contains(&rule.kind.as_str()) {
            problems.push(format!("`{name}` has the kind `{}`, which nothing implements", rule.kind));
            continue;
        }
        match rule.kind.as_str() {
            "choice" => {
                if rule.choices.is_empty() {
                    problems.push(format!("`{name}` is a choice with nothing to choose"));
                }
            }
            "percent" => match (rule.min, rule.max, rule.step) {
                (Some(min), Some(max), Some(step)) if min <= max && step > 0 => {}
                _ => problems.push(format!("`{name}` is a percent and needs `min` <= `max` and a `step` above 0")),
            },
            _ => {}
        }
        if let Some(default) = &rule.default {
            if name.ends_with(".*") {
                problems.push(format!("`{name}` is a family; a default belongs to one setting"));
            } else if !rule.accepts(default) {
                problems.push(format!("the default of `{name}`, `{default}`, is not a value it accepts"));
            }
        }
    }
    problems
}

impl Rule {
    /// Whether `value` is one this rule accepts: the behaviour of its kind.
    ///
    /// @implements REQ-LOOK.settings_declared
    pub fn accepts(&self, value: &str) -> bool {
        match self.kind.as_str() {
            "colour" => is_colour(value.to_string()),
            "choice" => self.choices.iter().any(|c| c == value),
            "percent" => match (value.parse::<i64>(), self.min, self.max, self.step) {
                (Ok(v), Some(min), Some(max), Some(step)) => {
                    v.to_string() == value && v >= min && v <= max && (v - min) % step == 0
                }
                _ => false,
            },
            "text" => !value.trim().is_empty(),
            _ => false,
        }
    }

    /// What a value of this rule looks like, for the refusal that names it.
    pub fn wanted(&self) -> String {
        match self.kind.as_str() {
            "colour" => "a colour like #1e90ff".to_string(),
            "choice" => format!("one of {}", self.choices.join(", ")),
            "percent" => format!(
                "a percentage from {} to {}, in {}s",
                self.min.unwrap_or(0),
                self.max.unwrap_or(0),
                self.step.unwrap_or(1)
            ),
            _ => "some text".to_string(),
        }
    }

    fn covers(&self, name: &str) -> bool {
        match self.name.strip_suffix(".*") {
            Some(section) => name.strip_prefix(section).is_some_and(|rest| rest.starts_with('.') && rest.len() > 1),
            None => self.name == name,
        }
    }
}

impl Settings {
    /// The rule that declares `name`: its own, else the family it belongs to.
    pub fn rule(&self, name: &str) -> Option<&Rule> {
        self.settings
            .iter()
            .find(|rule| rule.name == name)
            .or_else(|| self.settings.iter().find(|rule| rule.covers(name)))
    }

    /// Whether `name` is declared and `value` is one it accepts.
    ///
    /// @implements REQ-LOOK.setting_edited_in_place
    pub fn takes(&self, name: &str, value: &str) -> bool {
        self.rule(name).is_some_and(|rule| rule.accepts(value))
    }
}

pub const ZOOM: (i64, i64, i64) = (60, 200, 10);

/// The zoom, in percent, `steps` steps from `now`, kept within what can be read; no step
/// returns to 100 only when asked with nothing to step from.
///
/// @implements REQ-LOOK.zoom_steps
pub fn zoomed(now: &str, steps: i64) -> Option<i64> {
    let (least, most, step) = ZOOM;
    let now: i64 = now.parse().ok()?;
    Some((now + steps * step).clamp(least, most))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-LOOK.zoom_steps
    #[test]
    fn zoom_steps_by_ten_and_stops_at_its_ends() {
        assert_eq!(zoomed("100", 1), Some(110));
        assert_eq!(zoomed("100", -1), Some(90));
        assert_eq!(zoomed("200", 1), Some(200));
        assert_eq!(zoomed("60", -3), Some(60));
        assert_eq!(zoomed("big", 1), None);
        let settings = shipped();
        assert!(settings.takes("layout.zoom", "150"));
        assert!(!settings.takes("layout.zoom", "500"));
        assert!(!settings.takes("layout.zoom", "#fff"));
    }

    /// @tests REQ-LOOK.setting_edited_in_place
    #[test]
    fn a_layout_setting_takes_only_its_own_choices() {
        let settings = shipped();
        assert!(settings.takes("layout.stations", "top"));
        assert!(settings.takes("layout.stations", "hidden"));
        assert!(!settings.takes("layout.stations", "#ffffff"));
        assert!(!settings.takes("layout.menuBar", "top"));
        assert!(settings.takes("ui.accent", "#ffffff"));
        assert!(!settings.takes("ui.accent", "shown"));
        assert!(!settings.takes("nowhere.at_all", "#ffffff"), "an undeclared setting took a value");
    }

    /// @tests REQ-LOOK.setting_edited_in_place
    #[test]
    fn a_colour_is_a_hash_and_three_to_eight_hex_digits() {
        for good in ["#fff", "#ffff", "#1e90ff", "#1e90ff80", "#ABCdef"] {
            assert!(is_colour(good.into()), "{good}");
        }
        for bad in ["", "#", "fff", "#ff", "#fffff", "red", "#1e90fg", "#1e90ff8", "##fff", "#éee"] {
            assert!(!is_colour(bad.into()), "{bad}");
        }
    }
}
