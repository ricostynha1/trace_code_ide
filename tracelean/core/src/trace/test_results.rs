//! What the tests did, attached to the code they claim to exercise.
//!
//! A `@tests` annotation is a claim: this test exercises that clause. Counting
//! the claims is easy and says little -- three tests that all fail back a
//! clause exactly as badly as no tests do. So the count is drawn beside the
//! outcome, and the outcome comes from the test runner rather than from
//! TraceLean, which runs nothing itself.
//!
//! Same shape as line coverage: a normalized file the graph reads, and one
//! importer per tool. A language whose runner this does not understand can
//! write `.tracelean/test_results.json` directly -- the format is three fields.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Passed,
    Failed,
    /// Skipped, ignored, filtered out. Recorded rather than dropped: a test
    /// that did not run is not a test that passed, and a claim backed only by
    /// ignored tests should not look green.
    Skipped,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TestResults {
    /// Test name -> outcome. The name is whatever the runner prints, matched
    /// against annotation anchors by its last path segment.
    pub results: BTreeMap<String, Outcome>,
    #[serde(default)]
    pub source: String,
}

impl TestResults {
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(".tracelean").join("test_results.json")
    }

    /// Absent is normal, and reads as "nobody ran these through TraceLean" --
    /// which the graph shows as a bare count rather than as zero passing.
    pub fn load(root: &Path) -> TestResults {
        let Ok(text) = std::fs::read_to_string(Self::path_for(root)) else {
            return TestResults::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<PathBuf, String> {
        let path = Self::path_for(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serializing test results: {e}"))?;
        std::fs::write(&path, text + "\n").map_err(|e| format!("writing {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// The outcome for a test declaration, matched by its own name.
    ///
    /// Runners qualify names differently -- `unit::test_trace::name`,
    /// `checks.pricing_checks.Class.name` -- and an annotation anchors to a
    /// declaration, so the last segment is the only part both sides agree on.
    /// Ambiguity is possible and is resolved towards the worse outcome: two
    /// tests with one name, one failing, must not read as passing.
    pub fn outcome_for(&self, declaration: &str) -> Option<Outcome> {
        let mut found: Option<Outcome> = None;
        for (name, outcome) in &self.results {
            let last = name
                .rsplit(|c| c == ':' || c == '.' || c == '/')
                .next()
                .unwrap_or(name);
            if last != declaration {
                continue;
            }
            found = Some(match (found, outcome) {
                (Some(Outcome::Failed), _) | (_, Outcome::Failed) => Outcome::Failed,
                (Some(Outcome::Skipped), _) | (_, Outcome::Skipped) => Outcome::Skipped,
                _ => Outcome::Passed,
            });
        }
        found
    }
}

/// Import libtest's line-delimited JSON (`cargo test -- --format json`).
///
/// Every line is one event; the ones that matter are `{"type":"test",
/// "event":"ok"|"failed"|"ignored","name":...}`. Lines that are not JSON at all
/// are skipped rather than fatal, because cargo interleaves its own output with
/// the runner's and a hard failure there would make the importer unusable on
/// real output.
pub fn from_libtest_json(text: &str) -> TestResults {
    let mut out = TestResults { source: "libtest".into(), ..TestResults::default() };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
        if value.get("type").and_then(|t| t.as_str()) != Some("test") {
            continue;
        }
        let Some(name) = value.get("name").and_then(|n| n.as_str()) else { continue };
        let outcome = match value.get("event").and_then(|e| e.as_str()) {
            Some("ok") => Outcome::Passed,
            Some("failed") => Outcome::Failed,
            Some("ignored") => Outcome::Skipped,
            // `started` and anything else carries no verdict.
            _ => continue,
        };
        out.results.insert(name.to_string(), outcome);
    }
    out
}

/// Import the normalized form: `{"results": {"name": "passed"}}`, or the bare
/// map on its own.
pub fn from_normalized(json: &serde_json::Value) -> Result<TestResults, String> {
    if json.get("results").is_some() {
        return serde_json::from_value(json.clone())
            .map_err(|e| format!("not normalized test results: {e}"));
    }
    let map: BTreeMap<String, Outcome> = serde_json::from_value(json.clone())
        .map_err(|e| format!("not a name -> outcome map: {e}"))?;
    Ok(TestResults { results: map, source: "normalized".into() })
}

/// Import whichever form this file is, merging into what is already recorded.
pub fn import(root: &Path, path: &Path) -> Result<TestResults, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;

    let imported = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => from_normalized(&value)?,
        // Not one JSON document, so it is the line-delimited kind.
        Err(_) => {
            let parsed = from_libtest_json(&text);
            if parsed.is_empty() {
                return Err(format!(
                    "{} is neither normalized test results nor libtest JSON",
                    path.display()
                ));
            }
            parsed
        }
    };

    let mut existing = TestResults::load(root);
    existing.results.extend(imported.results);
    existing.source = if existing.source.is_empty() || existing.source == imported.source {
        imported.source
    } else {
        format!("{}, {}", existing.source, imported.source)
    };
    Ok(existing)
}
