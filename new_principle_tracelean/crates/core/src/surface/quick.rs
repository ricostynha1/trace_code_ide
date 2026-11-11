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

/// The commands a query finds, each an action and what is said of it: with no query all
/// of them; otherwise those whose words hold every word asked for, in their own order,
/// then those holding the query's letters in order, nearest first, up to `limit` of them.
///
/// @implements REQ-NAV.commands_found_by_letters
pub fn commands(all: Vec<(String, String)>, query: &str, limit: usize) -> Vec<(String, String)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return all;
    }
    let (mut found, rest): (Vec<(String, String)>, Vec<(String, String)>) = all.into_iter().partition(|(_, said)| {
        let lower = said.to_lowercase();
        words.iter().all(|word| lower.contains(word.as_str()))
    });
    let mut near: Vec<(i64, (String, String))> = rest
        .into_iter()
        .filter_map(|(action, said)| {
            let lower: Vec<char> = said.to_lowercase().chars().collect();
            let wanted: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
            let run = letters_from(&lower, &wanted, 0, lower.len())?;
            let spread = span_of(&lower, &wanted)?;
            (spread <= wanted.len() * 4).then(|| (run * 1000 - spread as i64, (action, said)))
        })
        .collect();
    near.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.extend(near.into_iter().map(|(_, entry)| entry).take(limit.saturating_sub(found.len())));
    found
}

fn span_of(lower: &[char], wanted: &[char]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for start in (0..lower.len()).filter(|i| Some(&lower[*i]) == wanted.first()) {
        let mut at = start;
        let mut last = start;
        let mut all = true;
        for letter in wanted {
            match (at..lower.len()).find(|i| lower[*i] == *letter) {
                Some(found) => {
                    last = found;
                    at = found + 1;
                }
                None => {
                    all = false;
                    break;
                }
            }
        }
        if !all {
            break;
        }
        let spread = last + 1 - start;
        best = Some(best.map_or(spread, |b| b.min(spread)));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-NAV.commands_found_by_letters
    #[test]
    fn a_command_is_found_by_its_words_and_by_letters_in_order() {
        let all: Vec<(String, String)> = [
            ("trace.report", "trace: write the evidence report: a card per clause"),
            ("history.tree", "history: show the tree"),
            ("file.refresh", "file: refresh: reload the files from the disk and the sandbox"),
        ]
        .iter()
        .map(|(a, s)| (a.to_string(), s.to_string()))
        .collect();
        let named = |found: Vec<(String, String)>| found.into_iter().map(|(a, _)| a).collect::<Vec<_>>();
        assert_eq!(commands(all.clone(), "", 24).len(), 3);
        assert_eq!(named(commands(all.clone(), "evidence report", 24)), vec!["trace.report"]);
        assert_eq!(named(commands(all.clone(), "histre", 24)), vec!["history.tree"], "letters in order find nothing");
        assert_eq!(named(commands(all.clone(), "rfrsh", 24)), vec!["file.refresh"]);
        assert!(commands(all, "no such words at all", 24).is_empty());
    }

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
