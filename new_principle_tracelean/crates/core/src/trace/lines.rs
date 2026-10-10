//! Line coverage, test by test.
//!
//! Each test is run on its own under an instrumented build, so what comes back
//! is not only which lines ran but which tests ran them and how often. This is
//! the decision half: reading LLVM's LCOV text, folding the runs into one table
//! per file, and summarising the lines a clause's implementing items span.
//! Running anything is `drt::lines_run`'s.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One test's counts for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestLines {
    pub test: String,
    pub file: String,
    /// One-based line, and how often it ran. A line absent is not executable.
    pub lines: Vec<(u32, u64)>,
}

/// One executable line: how often it ran, and by which tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHits {
    pub line: u32,
    pub hits: u64,
    /// The tests that ran it, by name, with their counts. Empty: uncovered.
    pub tests: Vec<(String, u64)>,
}

/// What `tracelean-trace --coverage` keeps: per file, the text's hash when it
/// was measured and its lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub files: BTreeMap<String, (String, Vec<LineHits>)>,
}

/// LCOV text as `(source file, [(line, count)])`, in the order it names them.
pub fn lcov(text: &str) -> Vec<(String, Vec<(u32, u64)>)> {
    let mut out: Vec<(String, Vec<(u32, u64)>)> = Vec::new();
    for line in text.lines() {
        if let Some(file) = line.strip_prefix("SF:") {
            out.push((file.to_string(), Vec::new()));
        } else if let Some(data) = line.strip_prefix("DA:") {
            let mut parts = data.split(',');
            let (Some(at), Some(count)) = (parts.next(), parts.next()) else { continue };
            if let (Ok(at), Ok(count), Some(last)) = (at.parse(), count.parse(), out.last_mut()) {
                last.1.push((at, count));
            }
        }
    }
    out
}

/// Every test's counts for `file`, folded into its executable lines.
///
/// A line any run reports is executable; its hits are the sum over tests, and
/// the tests listed are those that ran it at least once, by name.
///
/// @implements REQ-LINECOV.per_test
/// @drt REQ-LINECOV.per_test
pub fn merged(runs: Vec<TestLines>, file: String) -> Vec<LineHits> {
    let mut table: BTreeMap<u32, BTreeMap<String, u64>> = BTreeMap::new();
    for run in runs.iter().filter(|r| r.file == file) {
        for (line, count) in &run.lines {
            let tests = table.entry(*line).or_default();
            if *count > 0 {
                *tests.entry(run.test.clone()).or_default() += count;
            }
        }
    }
    table
        .into_iter()
        .map(|(line, tests)| LineHits {
            line,
            hits: tests.values().sum(),
            tests: tests.into_iter().collect(),
        })
        .collect()
}

/// How much of a stretch of lines ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    /// Executable lines some test ran.
    pub run: u64,
    /// Executable lines.
    pub all: u64,
    /// Every test that ran any of them, by name.
    pub tests: Vec<String>,
}

/// How much of lines `start..=end` (one-based) ran.
///
/// @implements REQ-LINECOV.clause_summary
/// @drt REQ-LINECOV.clause_summary
pub fn span_coverage(lines: Vec<LineHits>, start: u32, end: u32) -> Reach {
    let inside: Vec<&LineHits> = lines.iter().filter(|l| l.line >= start && l.line <= end).collect();
    let mut tests: Vec<String> = inside.iter().flat_map(|l| l.tests.iter().map(|(t, _)| t.clone())).collect();
    tests.sort();
    tests.dedup();
    Reach { run: inside.iter().filter(|l| l.hits > 0).count() as u64, all: inside.len() as u64, tests }
}

/// What is shown of a file's measured lines: them when they were measured
/// against the text the file has now (`hash`), nothing when against other text.
///
/// @implements REQ-LINECOV.stale_hidden
/// @drt REQ-LINECOV.stale_hidden
pub fn visible(measured: Option<(String, Vec<LineHits>)>, hash: String) -> Option<Vec<LineHits>> {
    measured.filter(|(at, _)| *at == hash).map(|(_, lines)| lines)
}

/// A file's lines, by reference, when they were measured against the text it
/// has now: `visible` without the copy.
pub fn current<'a>(coverage: &'a Coverage, path: &str, hash: &str) -> Option<&'a Vec<LineHits>> {
    coverage.files.get(path).filter(|(measured, _)| measured == hash).map(|(_, lines)| lines)
}

/// What a line's marker says when pointed at.
pub fn said(line: &LineHits) -> String {
    if line.tests.is_empty() {
        return "no test runs this line".into();
    }
    let who: Vec<String> = line.tests.iter().map(|(t, n)| format!("{t} ×{n}")).collect();
    format!("run {} times by {}", line.hits, who.join(", "))
}

/// A measured line as the editor marks it: the zero-based buffer line, how
/// often tests ran it — none is the mark of a line no test ran — and what
/// pointing at it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub line: usize,
    pub hits: u64,
    pub said: String,
}

/// The markers of the measured lines in a window of `height` buffer lines from
/// `top`. A line numbered 0 is no line and has none.
///
/// @implements REQ-LINECOV.uncovered_shown
/// @drt REQ-LINECOV.uncovered_shown
pub fn markers(lines: Vec<LineHits>, top: usize, height: usize) -> Vec<Marker> {
    lines
        .iter()
        .filter_map(|l| Some((l.line.checked_sub(1)? as usize, l)))
        .filter(|(at, _)| (top..top + height).contains(at))
        .map(|(at, l)| Marker { line: at, hits: l.hits, said: said(l) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(test: &str, lines: &[(u32, u64)]) -> TestLines {
        TestLines { test: test.into(), file: "src/a.rs".into(), lines: lines.to_vec() }
    }

    #[test]
    fn lcov_is_read_file_by_file() {
        let text = "TN:\nSF:/p/src/a.rs\nFN:3,f\nDA:3,2\nDA:4,0\nend_of_record\nSF:/p/src/b.rs\nDA:1,1\nend_of_record\n";
        assert_eq!(
            lcov(text),
            vec![("/p/src/a.rs".to_string(), vec![(3, 2), (4, 0)]), ("/p/src/b.rs".to_string(), vec![(1, 1)])]
        );
    }

    /// @tests REQ-LINECOV.per_test
    #[test]
    fn each_line_knows_which_tests_ran_it_and_how_often() {
        let lines = merged(vec![run("t1", &[(3, 2), (4, 0)]), run("t2", &[(3, 1), (4, 0), (5, 0)])], "src/a.rs".into());
        assert_eq!(lines[0], LineHits { line: 3, hits: 3, tests: vec![("t1".into(), 2), ("t2".into(), 1)] });
        assert_eq!(lines[1], LineHits { line: 4, hits: 0, tests: vec![] });
        assert_eq!(said(&lines[0]), "run 3 times by t1 ×2, t2 ×1");
        assert_eq!(said(&lines[1]), "no test runs this line");
    }

    /// @tests REQ-LINECOV.clause_summary
    #[test]
    fn a_span_counts_its_executable_lines() {
        let lines = merged(vec![run("t1", &[(3, 2), (4, 0), (9, 1)])], "src/a.rs".into());
        assert_eq!(span_coverage(lines, 1, 5), Reach { run: 1, all: 2, tests: vec!["t1".to_string()] });
    }

    /// @tests REQ-LINECOV.stale_hidden
    #[test]
    fn coverage_of_other_text_is_not_shown() {
        let mut coverage = Coverage::default();
        coverage.files.insert("src/a.rs".into(), ("h1".into(), vec![]));
        assert!(current(&coverage, "src/a.rs", "h1").is_some());
        assert!(current(&coverage, "src/a.rs", "h2").is_none());
    }
}
