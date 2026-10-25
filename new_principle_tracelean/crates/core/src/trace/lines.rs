use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestLines {
    pub test: String,
    pub file: String,
    pub lines: Vec<(u32, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHits {
    pub line: u32,
    pub hits: u64,
    pub tests: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub files: BTreeMap<String, (String, Vec<LineHits>)>,
    #[serde(default)]
    pub sources: String,
}

pub fn sources_hash(files: &BTreeMap<String, String>) -> String {
    let relevant: String = files
        .iter()
        .filter(|(path, _)| path.ends_with(".rs") || path.ends_with("Cargo.toml") || path.ends_with("Cargo.lock"))
        .map(|(path, text)| format!("{path}\0{}\0", crate::trace::hash::text(text)))
        .collect();
    crate::trace::hash::text(&relevant)
}

/// @implements REQ-STALE.measured_not_retaken
pub fn retake(held: &Coverage, files: &BTreeMap<String, String>, again: bool) -> bool {
    again || held.files.is_empty() || held.sources != sources_hash(files)
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    pub run: u64,
    pub all: u64,
    pub tests: Vec<String>,
}

/// @implements REQ-LINECOV.clause_summary
/// @drt REQ-LINECOV.clause_summary
pub fn span_coverage(lines: Vec<LineHits>, start: u32, end: u32) -> Reach {
    let inside: Vec<&LineHits> = lines.iter().filter(|l| l.line >= start && l.line <= end).collect();
    let mut tests: Vec<String> = inside.iter().flat_map(|l| l.tests.iter().map(|(t, _)| t.clone())).collect();
    tests.sort();
    tests.dedup();
    Reach { run: inside.iter().filter(|l| l.hits > 0).count() as u64, all: inside.len() as u64, tests }
}

/// @implements REQ-LINECOV.stale_hidden
/// @drt REQ-LINECOV.stale_hidden
pub fn visible(measured: Option<(String, Vec<LineHits>)>, hash: String) -> Option<Vec<LineHits>> {
    measured.filter(|(at, _)| *at == hash).map(|(_, lines)| lines)
}

pub fn current<'a>(coverage: &'a Coverage, path: &str, hash: &str) -> Option<&'a Vec<LineHits>> {
    coverage.files.get(path).filter(|(measured, _)| measured == hash).map(|(_, lines)| lines)
}

/// @drt REQ-STALE.measured_not_retaken
pub fn retake_owned(measured: bool, held: String, files: Vec<(String, String)>, again: bool) -> bool {
    let mut coverage = Coverage { sources: held, ..Coverage::default() };
    if measured {
        coverage.files.insert(String::new(), (String::new(), Vec::new()));
    }
    retake(&coverage, &files.into_iter().collect(), again)
}

#[cfg(test)]
mod sources_tests {
    use super::*;

    /// @tests REQ-STALE.measured_not_retaken
    #[test]
    fn the_sources_hash_moves_with_rust_files_and_manifests_only() {
        let files = |extra: &[(&str, &str)]| -> BTreeMap<String, String> {
            [("src/a.rs", "fn a() {}"), ("Cargo.toml", "[package]"), ("README.md", "hi")]
                .iter()
                .chain(extra)
                .map(|(p, t)| (p.to_string(), t.to_string()))
                .collect()
        };
        let base = sources_hash(&files(&[]));
        assert_eq!(base, sources_hash(&files(&[("docs/x.md", "prose")])));
        assert_ne!(base, sources_hash(&files(&[("tests/t.rs", "#[test] fn t() {}")])));
        assert_ne!(base, sources_hash(&files(&[("Cargo.lock", "v")])));
    }
}

pub fn said(line: &LineHits) -> String {
    if line.tests.is_empty() {
        return "no test runs this line".into();
    }
    let who: Vec<String> = line.tests.iter().map(|(t, n)| format!("{t} ×{n}")).collect();
    format!("run {} times by {}", line.hits, who.join(", "))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub line: usize,
    pub hits: u64,
    pub said: String,
}

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

    /// @tests REQ-LINECOV.uncovered_shown
    #[test]
    fn markers_fall_on_the_rows_shown() {
        let hit = |line: u32, hits: u64| LineHits { line, hits, tests: Vec::new() };
        let shown = markers(vec![hit(0, 1), hit(3, 2), hit(5, 0), hit(9, 1)], 2, 4);
        assert_eq!(shown.iter().map(|m| (m.line, m.hits)).collect::<Vec<_>>(), vec![(2, 2), (4, 0)]);
        assert_eq!(shown[1].said, said(&hit(5, 0)));
    }

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
