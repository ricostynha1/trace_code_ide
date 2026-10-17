//! Finding in a buffer: a text with `*` and `?`, and the lines that hold it.
//!
//! Pure functions of the text, so a window and a terminal find the same places
//! and a search of a file tree, of the requirements or of the findings — all of
//! them buffers — is the same search as one of a file.

/// One character folded for matching: the letters a to z only, so that a
/// character is one character on both sides and offsets stay offsets of the
/// text itself.
fn fold(c: char) -> char {
    c.to_ascii_lowercase()
}

/// How many characters of `text` the shortest match of `pattern` at its start
/// takes, if there is one.
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

/// Every place `pattern` matches in `text`, as character offsets of its start
/// and its end, left to right and without overlap. `*` is any run of
/// characters and `?` any one, within a line; the case of a to z is ignored. A match is
/// never empty, so `*` alone finds nothing rather than everywhere.
///
/// @implements REQ-NAV.find_by_glob
pub fn glob_matches(text: String, pattern: String) -> Vec<(usize, usize)> {
    let mut pattern: Vec<char> = pattern.chars().map(fold).collect();
    // `**` is `*`; left as written, a few of them on a long line is exponential.
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

/// The lines of `text` holding a match of `pattern`, each with its number
/// (from 1).
///
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
        // Not across a line.
        assert_eq!(found("a\nb", "a*b"), vec![]);
        // Never empty: a lone star finds nothing.
        assert_eq!(found("abc", "*"), vec![]);
    }

    /// @tests REQ-NAV.find_by_glob
    #[test]
    fn offsets_count_characters_of_the_whole_text() {
        // `λ` is two bytes and counts once.
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
