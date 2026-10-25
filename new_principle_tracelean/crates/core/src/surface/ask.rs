/// @implements REQ-NAV.name_is_asked_for
pub fn ask_step(answer: String, key: String) -> (String, String) {
    let blank = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
    match key.as_str() {
        super::keymap::LEAVE => ("cancelled".to_string(), answer),
        "Enter" => {
            let said = answer.trim_matches(blank).to_string();
            if said.is_empty() {
                ("cancelled".to_string(), answer)
            } else {
                ("done".to_string(), said)
            }
        }
        "Backspace" => {
            let mut kept = answer;
            kept.pop();
            ("typing".to_string(), kept)
        }
        super::keymap::SPACE => ("typing".to_string(), answer + " "),
        other if other.chars().count() == 1 => ("typing".to_string(), answer + other),
        _ => ("typing".to_string(), answer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(answer: &str, key: &str) -> (String, String) {
        ask_step(answer.into(), key.into())
    }

    /// @tests REQ-NAV.name_is_asked_for
    #[test]
    fn keys_make_the_answer_and_enter_hands_it_over() {
        assert_eq!(step("ab", "c"), ("typing".into(), "abc".into()));
        assert_eq!(step("ab", "Space"), ("typing".into(), "ab ".into()));
        assert_eq!(step("ab", "Backspace"), ("typing".into(), "a".into()));
        assert_eq!(step("", "Backspace"), ("typing".into(), "".into()));
        assert_eq!(step("ab", "Left"), ("typing".into(), "ab".into()));
        assert_eq!(step(" new.rs ", "Enter"), ("done".into(), "new.rs".into()));
    }

    /// @tests REQ-NAV.name_is_asked_for
    #[test]
    fn escape_and_an_empty_answer_do_nothing() {
        assert_eq!(step("ab", "Escape"), ("cancelled".into(), "ab".into()));
        assert_eq!(step("  ", "Enter"), ("cancelled".into(), "  ".into()));
    }
}
