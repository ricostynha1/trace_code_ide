fn fold(c: char) -> char {
    c.to_ascii_lowercase()
}

fn shortest(pattern: &[char], text: &[char]) -> Option<usize> {
    match pattern.split_first() {
        None => Some(0),
        Some(('*', rest)) => (0..=text.len()).find_map(|skip| shortest(rest, &text[skip..]).map(|n| skip + n)),
        Some((wanted, rest)) => {
            let (here, tail) = text.split_first()?;
            if *wanted == '?' || wanted == here {
                shortest(rest, tail).map(|n| n + 1)
            } else {
                None
            }
        }
    }
}

/// @implements REQ-NAV.find_by_glob
pub fn glob_matches(text: String, pattern: String) -> Vec<(usize, usize)> {
    let mut pattern: Vec<char> = pattern.chars().map(fold).collect();
    pattern.dedup_by(|a, b| *a == '*' && *b == '*');
    let mut found = Vec::new();
    if pattern.is_empty() {
        return found;
    }
    let mut line_start = 0usize;
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().map(fold).collect();
        let mut at = 0usize;
        while at < chars.len() {
            match shortest(&pattern, &chars[at..]) {
                Some(length) if length > 0 => {
                    found.push((line_start + at, line_start + at + length));
                    at += length;
                }
                _ => at += 1,
            }
        }
        line_start += chars.len() + 1;
    }
    found
}

/// @implements REQ-NAV.find_lists_lines
pub fn matching_lines(text: String, pattern: String) -> Vec<(usize, String)> {
    text.split('\n')
        .enumerate()
        .filter(|(_, line)| !glob_matches(line.to_string(), pattern.clone()).is_empty())
        .map(|(n, line)| (n + 1, line.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str, pattern: &str) -> Vec<(usize, usize)> {
        glob_matches(text.into(), pattern.into())
    }

    /// @tests REQ-NAV.find_by_glob
    #[test]
    fn a_text_with_no_wildcard_is_found_as_itself_in_any_case() {
        assert_eq!(found("one Two two", "two"), vec![(4, 7), (8, 11)]);
        assert_eq!(found("one", "x"), vec![]);
    }

    /// @tests REQ-NAV.find_by_glob
    #[test]
    fn a_star_is_any_run_and_a_question_mark_any_one_within_a_line() {
        assert_eq!(found("REQ-ACT.one_path", "req-*.one"), vec![(0, 11)]);
        assert_eq!(found("cat cot cut", "c?t"), vec![(0, 3), (4, 7), (8, 11)]);
        assert_eq!(found("a\nb", "a*b"), vec![]);
        assert_eq!(found("abc", "*"), vec![]);
    }

    /// @tests REQ-NAV.find_by_glob
    #[test]
    fn offsets_count_characters_of_the_whole_text() {
        assert_eq!(found("λ x\nλ y", "?y"), vec![(5, 7)]);
    }

    /// @tests REQ-NAV.find_lists_lines
    #[test]
    fn the_lines_that_hold_a_match_are_listed_by_number() {
        let text = "draft one\napproved two\ndraft three";
        assert_eq!(
            matching_lines(text.into(), "dra*".into()),
            vec![(1, "draft one".to_string()), (3, "draft three".to_string())]
        );
    }
}
