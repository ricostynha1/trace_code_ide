/// @implements REQ-LOOK.setting_edited_in_place
pub fn is_colour(value: String) -> bool {
    let Some(digits) = value.strip_prefix('#') else { return false };
    matches!(digits.chars().count(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit())
}

/// The values a setting that is not a colour takes, or nothing when it is one.
pub fn choices(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "layout.menuBar" => Some(&["shown", "hidden"]),
        "layout.stations" => Some(&["hidden", "left", "top"]),
        _ => None,
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

/// @implements REQ-LOOK.setting_edited_in_place
pub fn takes(name: String, value: String) -> bool {
    if name == "layout.zoom" {
        return zoomed(&value, 0).is_some_and(|kept| kept.to_string() == value);
    }
    match choices(&name) {
        Some(allowed) => allowed.contains(&value.as_str()),
        None => is_colour(value),
    }
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
        assert!(takes("layout.zoom".into(), "150".into()));
        assert!(!takes("layout.zoom".into(), "500".into()));
        assert!(!takes("layout.zoom".into(), "#fff".into()));
    }

    /// @tests REQ-LOOK.setting_edited_in_place
    #[test]
    fn a_layout_setting_takes_only_its_own_choices() {
        assert!(takes("layout.stations".into(), "top".into()));
        assert!(takes("layout.stations".into(), "hidden".into()));
        assert!(!takes("layout.stations".into(), "#ffffff".into()));
        assert!(!takes("layout.menuBar".into(), "top".into()));
        assert!(takes("ui.accent".into(), "#ffffff".into()));
        assert!(!takes("ui.accent".into(), "shown".into()));
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
