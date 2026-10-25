/// @implements REQ-LOOK.setting_edited_in_place
pub fn is_colour(value: String) -> bool {
    let Some(digits) = value.strip_prefix('#') else { return false };
    matches!(digits.chars().count(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

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
