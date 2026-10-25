fn score(path: &str, query: &str) -> Option<i64> {
    let lower: Vec<char> = path.to_lowercase().chars().collect();
    let wanted: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if wanted.is_empty() {
        return Some(-(lower.len() as i64));
    }
    let name_starts = lower.iter().rposition(|c| *c == '/').map_or(0, |n| n + 1);
    let best = [0, name_starts]
        .into_iter()
        .filter_map(|from| letters_from(&lower, &wanted, from, name_starts))
        .max()?;
    let name = (lower.len() - name_starts) as i64;
    Some(best * 1_000_000 - name * 1000 - lower.len() as i64)
}

fn letters_from(lower: &[char], wanted: &[char], from: usize, name_starts: usize) -> Option<i64> {
    let mut total = 0i64;
    let mut at = from;
    let mut previous: Option<usize> = None;
    for letter in wanted {
        let found = (at..lower.len()).find(|i| lower[*i] == *letter)?;
        total += 1;
        if found >= name_starts {
            total += 4;
        }
        if previous.is_some_and(|p| p + 1 == found) {
            total += 6;
        }
        if found == name_starts || (found > 0 && matches!(lower[found - 1], '/' | '_' | '-' | '.')) {
            total += 3;
        }
        previous = Some(found);
        at = found + 1;
    }
    Some(total)
}

/// @implements REQ-NAV.quick_open
/// @drt REQ-NAV.quick_open
pub fn matches(files: Vec<String>, query: &str, limit: usize) -> Vec<String> {
    let mut scored: Vec<(i64, &String)> =
        files.iter().filter_map(|path| score(path, query).map(|s| (s, path))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(limit).map(|(_, path)| path.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<String> {
        ["src/surface/screen.rs", "src/surface/view.rs", "crates/core/src/lib.rs", "docs/screenshots.md", "README.md"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// @tests REQ-NAV.quick_open
    #[test]
    fn letters_in_order_find_a_file_and_the_name_counts_most() {
        assert_eq!(matches(files(), "screen", 2), vec!["src/surface/screen.rs", "docs/screenshots.md"]);
        assert_eq!(matches(files(), "vw", 5), vec!["src/surface/view.rs"]);
        assert_eq!(matches(files(), "LIB", 5), vec!["crates/core/src/lib.rs"]);
        assert!(matches(files(), "zzz", 5).is_empty());
        assert_eq!(matches(files(), "", 1), vec!["README.md"]);
    }
}
