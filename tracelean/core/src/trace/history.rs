//! Traceability progress over git history.
//!
//! A single coverage number tells you where the project is; the shape of that
//! number over time tells you whether the practice is working. Both matter, and
//! the second is the one that catches "we annotated everything in week one and
//! nothing since".
//!
//! Commits, not undo-tree nodes, are the unit. The undo tree is one developer's
//! history; a requirement's coverage is a property of the shared project, and a
//! chart that moved every time somebody typed would measure typing.
//!
//! Each commit is materialized into a temporary directory with `git archive`
//! and indexed there. That is slow — so every computed point is cached by
//! commit sha in `.tracelean/history.json`, and a sha is computed exactly once
//! for the life of the repository.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{Level, TraceIndex};

/// Coverage and assurance of the whole project at one commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryPoint {
    pub commit: String,
    /// Committer date, ISO-8601.
    pub at: String,
    pub subject: String,
    /// Mean coverage across root requirements, 0.0–1.0.
    pub coverage: f32,
    /// True when any root's coverage is only a lower bound (`decomposition:
    /// open` somewhere beneath it), so the point must render as "≥".
    pub coverage_is_lower_bound: bool,
    /// How many leaf clauses sit at each level. Keyed by the level's name so
    /// the cache file stays readable and survives a level being added.
    pub assurance_counts: BTreeMap<String, usize>,
    pub requirements: usize,
    /// Weakest level across all roots — what the project as a whole is worth.
    pub weakest: Option<Level>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryCache {
    /// Schema tag, so a change to how a point is computed invalidates the file
    /// rather than mixing old and new numbers on one chart.
    #[serde(default)]
    scheme: String,
    #[serde(default)]
    points: BTreeMap<String, HistoryPoint>,
}

const SCHEME: &str = "history/v1";

fn cache_path(root: &Path) -> PathBuf {
    root.join(".tracelean").join("history.json")
}

fn load_cache(root: &Path) -> HistoryCache {
    let cached: HistoryCache = std::fs::read_to_string(cache_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if cached.scheme == SCHEME {
        cached
    } else {
        HistoryCache::default()
    }
}

fn save_cache(root: &Path, cache: &HistoryCache) {
    let path = cache_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, json);
    }
}

/// Summarize an index into one point (the commit fields are filled by the caller).
pub fn summarize(index: &TraceIndex) -> (f32, bool, BTreeMap<String, usize>, Option<Level>) {
    let roots = index.roots();
    let mut total = 0.0f32;
    let mut lower_bound = false;
    let mut weakest: Option<Level> = None;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for root in &roots {
        let rollup = index.rollup(root);
        total += rollup.coverage;
        lower_bound |= rollup.coverage_is_lower_bound;
        weakest = Some(match weakest {
            Some(current) if current <= rollup.assurance => current,
            _ => rollup.assurance,
        });
    }

    // Level histogram over every clause of every requirement, so the chart can
    // show the *shape* of the evidence and not only its average.
    for req in index.requirements.values() {
        for clause in req.clauses.keys() {
            let level = index.assurance(&req.id, Some(clause));
            *counts.entry(format!("{:?}", level.weakest())).or_insert(0) += 1;
        }
    }

    let coverage = if roots.is_empty() { 0.0 } else { total / roots.len() as f32 };
    (coverage, lower_bound, counts, weakest)
}

/// Read the last `limit` commits touching the project.
fn git_log(root: &Path, limit: usize) -> Result<Vec<(String, String, String)>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg(format!("-n{}", limit))
        .arg("--format=%H%x1f%cI%x1f%s")
        .output()
        .map_err(|e| format!("git log failed: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\u{1f}');
            let sha = parts.next()?.to_string();
            let at = parts.next()?.to_string();
            let subject = parts.next().unwrap_or("").to_string();
            (!sha.is_empty()).then_some((sha, at, subject))
        })
        .collect())
}

/// Materialize one commit into `dest` and index it there.
fn index_at_commit(root: &Path, sha: &str, dest: &Path) -> Result<TraceIndex, String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let archive = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("archive")
        .arg("--format=tar")
        .arg(sha)
        .output()
        .map_err(|e| format!("git archive failed: {e}"))?;
    if !archive.status.success() {
        return Err(format!(
            "git archive {sha} failed: {}",
            String::from_utf8_lossy(&archive.stderr).trim()
        ));
    }
    let tar_path = dest.join("commit.tar");
    std::fs::write(&tar_path, &archive.stdout).map_err(|e| e.to_string())?;
    let extract = Command::new("tar")
        .arg("-xf")
        .arg(&tar_path)
        .arg("-C")
        .arg(dest)
        .output()
        .map_err(|e| format!("tar failed: {e}"))?;
    let _ = std::fs::remove_file(&tar_path);
    if !extract.status.success() {
        return Err(format!(
            "extracting {sha} failed: {}",
            String::from_utf8_lossy(&extract.stderr).trim()
        ));
    }
    Ok(super::build(dest))
}

/// Progress over the last `limit` commits, oldest first.
///
/// Commits that cannot be materialized are skipped with the reason recorded in
/// the returned errors rather than silently dropped — a chart with a hole in it
/// should say why there is a hole.
pub fn history(root: &Path, limit: usize) -> Result<(Vec<HistoryPoint>, Vec<String>), String> {
    let commits = git_log(root, limit)?;
    let mut cache = load_cache(root);
    if cache.scheme.is_empty() {
        cache.scheme = SCHEME.to_string();
    }

    let mut points = Vec::new();
    let mut problems = Vec::new();
    let mut computed_any = false;

    // Unique per call, not merely per process: two `history` calls running at
    // once would otherwise share a scratch directory and delete each other's
    // extracted trees on the way out.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp_base = std::env::temp_dir().join(format!(
        "tracelean-history-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));

    for (sha, at, subject) in commits.into_iter().rev() {
        if let Some(point) = cache.points.get(&sha) {
            points.push(point.clone());
            continue;
        }
        let dest = temp_base.join(&sha);
        let result = index_at_commit(root, &sha, &dest);
        let _ = std::fs::remove_dir_all(&dest);
        match result {
            Ok(index) => {
                let (coverage, lower_bound, counts, weakest) = summarize(&index);
                let point = HistoryPoint {
                    commit: sha.clone(),
                    at,
                    subject,
                    coverage,
                    coverage_is_lower_bound: lower_bound,
                    assurance_counts: counts,
                    requirements: index.requirements.len(),
                    weakest,
                };
                cache.points.insert(sha, point.clone());
                computed_any = true;
                points.push(point);
            }
            Err(e) => problems.push(format!("{}: {}", &sha[..sha.len().min(8)], e)),
        }
    }

    let _ = std::fs::remove_dir_all(&temp_base);
    if computed_any {
        save_cache(root, &cache);
    }
    Ok((points, problems))
}
