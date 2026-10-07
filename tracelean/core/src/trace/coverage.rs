//! Line coverage, attached to the code it is about.
//!
//! A percentage for a repository is close to useless: it averages the module
//! nobody tests with the one that is exhaustively tested, and the number that
//! comes out moves for reasons nobody can act on. Coverage becomes actionable
//! when it is attached to a *declaration* -- this function, the one annotated
//! as implementing a requirement clause, was executed by the tests or it was
//! not.
//!
//! That is also the only form in which it says anything about traceability. A
//! `@tests` annotation is a claim that a test exercises a clause; if the
//! implementation of that clause never runs, the claim is false and the line
//! data is what falsifies it.
//!
//! TraceLean does not measure coverage itself. It reads what the language's own
//! tool produced -- `cargo llvm-cov --json`, `coverage json` -- and normalizes
//! it into `.tracelean/coverage.json`, which is the only format the rest of the
//! system knows about. Adding a language means adding an importer here, not
//! teaching the graph about a new tool.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Which lines of one file ran, and which did not.
///
/// Lines nobody could execute -- blanks, comments, declarations -- are in
/// neither set. A line's absence therefore means "not executable", not
/// "uncovered", which is the distinction that makes a per-declaration fraction
/// honest: a twenty-line function with three statements is 3/3, not 3/20.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileCoverage {
    /// 1-indexed, as every coverage tool reports them.
    pub covered: BTreeSet<u32>,
    pub uncovered: BTreeSet<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LineCoverage {
    /// Project-relative paths.
    pub files: BTreeMap<PathBuf, FileCoverage>,
    /// What produced this, and when. Recorded because coverage read from a
    /// stale file is worse than no coverage at all.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub at: String,
}

/// Covered and executable line counts over a span, 0-indexed and inclusive as
/// anchors are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpanCoverage {
    pub covered: u32,
    pub executable: u32,
}

impl SpanCoverage {
    pub fn fraction(&self) -> f32 {
        if self.executable == 0 {
            return 1.0;
        }
        self.covered as f32 / self.executable as f32
    }
}

impl LineCoverage {
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(".tracelean").join("coverage.json")
    }

    /// Read `.tracelean/coverage.json`, or nothing.
    ///
    /// Absent is a normal state -- most projects will not have run a coverage
    /// tool -- so this returns an empty map rather than an error, and the graph
    /// shows no coverage badge instead of a zero. Reporting 0% for a file
    /// nobody measured is a lie a dashboard should never tell.
    pub fn load(root: &Path) -> LineCoverage {
        let Ok(text) = std::fs::read_to_string(Self::path_for(root)) else {
            return LineCoverage::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<PathBuf, String> {
        let path = Self::path_for(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| format!("serializing coverage: {e}"))?;
        std::fs::write(&path, text + "\n").map_err(|e| format!("writing {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Coverage over a declaration's span. `None` when the file was not
    /// measured at all.
    pub fn for_span(&self, file: &Path, start_line: u32, end_line: u32) -> Option<SpanCoverage> {
        let data = self.files.get(file)?;
        let first = start_line + 1;
        let last = end_line + 1;
        let covered = data.covered.range(first..=last).count() as u32;
        let uncovered = data.uncovered.range(first..=last).count() as u32;
        Some(SpanCoverage { covered, executable: covered + uncovered })
    }
}

/// Make a path relative to the project root, if it is under it.
fn relative(root: &Path, path: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(path);
    if let Ok(stripped) = candidate.strip_prefix(root) {
        return Some(stripped.to_path_buf());
    }
    if candidate.is_relative() {
        return Some(candidate);
    }
    // A container measured the code at a different absolute path than the host
    // reads it from. Falling back to the tail that exists under the root keeps
    // `make cover` output usable outside the container it was produced in.
    let mut components: Vec<_> = candidate.components().collect();
    while !components.is_empty() {
        components.remove(0);
        let tail: PathBuf = components.iter().collect();
        if !tail.as_os_str().is_empty() && root.join(&tail).exists() {
            return Some(tail);
        }
    }
    None
}

/// Import `cargo llvm-cov --json` output (the llvm-cov export format).
///
/// The segment array is the load-bearing part: each entry is
/// `[line, column, count, has_count, is_region_entry, is_gap]`, and a line is
/// executable when some segment on it has a count. Lines with no segment at all
/// are not executable and must stay out of both sets.
pub fn from_llvm_cov(root: &Path, json: &serde_json::Value) -> Result<LineCoverage, String> {
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|d| d.first())
        .ok_or("not llvm-cov JSON: no `data[0]`")?;
    let files = data
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or("not llvm-cov JSON: no `data[0].files`")?;

    let mut out = LineCoverage {
        source: "cargo llvm-cov".into(),
        ..LineCoverage::default()
    };
    for file in files {
        let Some(name) = file.get("filename").and_then(|n| n.as_str()) else { continue };
        let Some(path) = relative(root, name) else { continue };
        let mut entry = FileCoverage::default();
        if let Some(segments) = file.get("segments").and_then(|s| s.as_array()) {
            for segment in segments {
                let Some(parts) = segment.as_array() else { continue };
                let (Some(line), Some(count), Some(has_count)) = (
                    parts.first().and_then(|v| v.as_u64()),
                    parts.get(2).and_then(|v| v.as_u64()),
                    parts.get(3).and_then(|v| v.as_bool()),
                ) else {
                    continue;
                };
                if !has_count {
                    continue;
                }
                let line = line as u32;
                if count > 0 {
                    entry.uncovered.remove(&line);
                    entry.covered.insert(line);
                } else if !entry.covered.contains(&line) {
                    entry.uncovered.insert(line);
                }
            }
        }
        if !entry.covered.is_empty() || !entry.uncovered.is_empty() {
            out.files.insert(path, entry);
        }
    }
    Ok(out)
}

/// Import `coverage json` output from coverage.py.
pub fn from_coverage_py(root: &Path, json: &serde_json::Value) -> Result<LineCoverage, String> {
    let files = json
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or("not coverage.py JSON: no `files` object")?;

    let mut out = LineCoverage {
        source: "coverage.py".into(),
        ..LineCoverage::default()
    };
    for (name, file) in files {
        let Some(path) = relative(root, name) else { continue };
        let mut entry = FileCoverage::default();
        let numbers = |key: &str| -> Vec<u32> {
            file.get(key)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|n| n.as_u64()).map(|n| n as u32).collect())
                .unwrap_or_default()
        };
        entry.covered.extend(numbers("executed_lines"));
        entry.uncovered.extend(numbers("missing_lines"));
        if !entry.covered.is_empty() || !entry.uncovered.is_empty() {
            out.files.insert(path, entry);
        }
    }
    Ok(out)
}

/// Import whichever of the two formats this file is, merging into what is
/// already recorded.
///
/// Merging rather than replacing, because a project with a Rust implementation
/// and a Python one has two tools and two runs, and the second must not erase
/// the first.
pub fn import(root: &Path, path: &Path) -> Result<LineCoverage, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))?;

    let imported = if json.get("data").is_some() {
        from_llvm_cov(root, &json)?
    } else if json.get("files").is_some() {
        from_coverage_py(root, &json)?
    } else {
        return Err(format!(
            "{} is neither `cargo llvm-cov --json` nor `coverage json` output",
            path.display()
        ));
    };

    let mut existing = LineCoverage::load(root);
    let source = imported.source.clone();
    for (file, data) in imported.files {
        existing.files.insert(file, data);
    }
    existing.source = if existing.source.is_empty() || existing.source == source {
        source
    } else {
        format!("{}, {source}", existing.source)
    };
    Ok(existing)
}
