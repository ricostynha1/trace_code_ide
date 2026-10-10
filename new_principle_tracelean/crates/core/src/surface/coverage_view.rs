//! Which lines of a requirement's code tests run: what a coverage count on
//! the requirement page opens. Item by item — the function, the tests that
//! ran it and how often, and every executable line no test ran, each a link
//! to it — so a count that is short says where it is short.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::surface::requirement_view::{covered_text, link_text};
use crate::surface::sandbox_view::Lines;
use crate::surface::view::{Buffer, Role, TokenKind};
use crate::trace::lines::LineHits;

/// One item implementing the requirement, measured against its current text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    /// The item's first line, one-based.
    pub start: u32,
    pub symbol: Option<String>,
    /// The clause it implements; `None` for a requirement without clauses.
    pub clause: Option<String>,
    /// Its executable lines.
    pub lines: Vec<LineHits>,
    /// Its source, from `start` on, a line each.
    pub text: Vec<String>,
}

/// Lines run, executable lines and tests over `items`, each line of a file
/// counted once however many items span it.
///
/// @implements REQ-LINECOV.requirement_summary
pub fn total(items: &[Item]) -> (u64, u64, u64) {
    let mut lines: BTreeMap<(&str, u32), bool> = BTreeMap::new();
    let mut tests: BTreeSet<&str> = BTreeSet::new();
    for item in items {
        for line in &item.lines {
            *lines.entry((item.path.as_str(), line.line)).or_default() |= line.hits > 0;
            tests.extend(line.tests.iter().map(|(t, _)| t.as_str()));
        }
    }
    (lines.values().filter(|run| **run).count() as u64, lines.len() as u64, tests.len() as u64)
}

/// `total` over items as the model takes them: a path and its measured lines.
///
/// @drt REQ-LINECOV.requirement_summary
pub fn total_owned(items: Vec<(String, Vec<LineHits>)>) -> (u64, u64, u64) {
    let items: Vec<Item> = items
        .into_iter()
        .map(|(path, lines)| Item { path, start: 1, symbol: None, clause: None, lines, text: Vec::new() })
        .collect();
    total(&items)
}

/// `coverage_view` with its arguments owned, as a differential test calls it.
///
/// @drt REQ-LINECOV.lines_listed
pub fn coverage_view_owned(named: String, items: Vec<Item>, width: usize) -> Buffer {
    coverage_view(&named, &items, width)
}

/// The coverage of `named` (`REQ-X` or `REQ-X.clause`), as a record titled
/// `coverage <named>`.
///
/// @implements REQ-LINECOV.lines_listed
pub fn coverage_view(named: &str, items: &[Item], width: usize) -> Buffer {
    let mut out = Lines::new(width);
    out.line(&[("Coverage of ", Role::Heading, &[]), (named, Role::Requirement, &["trace.requirement"])]);
    if items.is_empty() {
        out.wrapped(
            "",
            "Nothing implementing it was measured against the text it has now. Run `tracelean-trace . --coverage`.",
            Role::Plain,
            &[],
        );
        return out.finish(&format!("coverage {named}"));
    }
    let shown = |(run, all, tests): (u64, u64, u64)| {
        (covered_text(run, all, tests), if run == all { Role::Added } else { Role::Removed })
    };
    let (said, role) = shown(total(items));
    out.line(&[(&said, role, &[])]);
    for item in items {
        out.blank();
        let link = link_text(&item.path, item.start);
        let mut pieces: Vec<(&str, Role, &[&str])> = Vec::new();
        if let Some(symbol) = &item.symbol {
            pieces.push((symbol, Role::Token { kind: TokenKind::Function }, &[]));
            pieces.push(("  ", Role::Plain, &[]));
        }
        pieces.push((&link, Role::Path, &["file.open"]));
        out.line(&pieces);
        let (said, role) = shown(total(std::slice::from_ref(item)));
        out.line(&[("  ", Role::Plain, &[]), (&said, role, &[])]);
        // Each test that ran any of its lines, with how many times in all.
        let mut by_test: BTreeMap<&str, u64> = BTreeMap::new();
        for line in &item.lines {
            for (test, n) in &line.tests {
                *by_test.entry(test.as_str()).or_default() += n;
            }
        }
        for (test, n) in &by_test {
            let times = format!(" ×{n}");
            out.line(&[("  ran by  ", Role::Plain, &[]), (test, Role::Token { kind: TokenKind::Function }, &[]), (&times, Role::Plain, &[])]);
        }
        let unrun: Vec<&LineHits> = item.lines.iter().filter(|l| l.hits == 0).collect();
        if !unrun.is_empty() {
            out.line(&[("  no test runs", Role::Removed, &[])]);
        }
        for line in unrun {
            let link = link_text(&item.path, line.line);
            // A line before the item's start has no source of it to show.
            let source = line
                .line
                .checked_sub(item.start)
                .and_then(|at| item.text.get(at as usize))
                .map(|s| s.trim())
                .unwrap_or_default();
            let source = format!("  {source}");
            out.line(&[("    ", Role::Plain, &[]), (&link, Role::Path, &["file.open"]), (&source, Role::Plain, &[])]);
        }
    }
    out.finish(&format!("coverage {named}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    fn hits(line: u32, tests: &[(&str, u64)]) -> LineHits {
        LineHits { line, hits: tests.iter().map(|(_, n)| n).sum(), tests: tests.iter().map(|(t, n)| (t.to_string(), *n)).collect() }
    }

    fn items() -> Vec<Item> {
        vec![
            Item {
                path: "src/a.rs".into(),
                start: 10,
                symbol: Some("describe".into()),
                clause: Some("one".into()),
                lines: vec![hits(10, &[("t1", 2)]), hits(11, &[]), hits(12, &[("t1", 1), ("t2", 1)])],
                text: vec!["fn describe() {".into(), "    \"steam\"".into(), "}".into()],
            },
            // A second clause's item over the same lines counts them once.
            Item {
                path: "src/a.rs".into(),
                start: 12,
                symbol: None,
                clause: Some("two".into()),
                lines: vec![hits(12, &[("t1", 1), ("t2", 1)])],
                text: vec!["}".into()],
            },
        ]
    }

    /// Over the whole requirement, a line two items span counts once.
    ///
    /// @tests REQ-LINECOV.requirement_summary
    #[test]
    fn a_line_is_counted_once_over_the_requirement() {
        assert_eq!(total(&items()), (2, 3, 2));
    }

    /// Each item says which tests ran it and how often, and each line no test
    /// ran is a link to it, with its source.
    ///
    /// @tests REQ-LINECOV.lines_listed
    #[test]
    fn the_tests_behind_an_item_and_its_unrun_lines_are_listed() {
        let shown = coverage_view("REQ-X", &items(), 80);
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(shown.id, "record:coverage REQ-X");
        assert!(shown.text.contains("2/3 lines run (66%), by 2 tests"), "{}", shown.text);
        assert!(shown.text.contains("ran by  t1 ×3\n  ran by  t2 ×1"), "{}", shown.text);
        let at = shown.text.find("src/a.rs:11").expect("the unrun line");
        assert_eq!(actions_at(shown.clone(), shown.text[..at].chars().count()), vec!["file.open".to_string()]);
        assert!(shown.text.contains("src/a.rs:11  \"steam\""), "{}", shown.text);
        assert!(coverage_view("REQ-X", &[], 80).text.contains("--coverage"));
    }
}
