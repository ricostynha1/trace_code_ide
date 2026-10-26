use std::collections::BTreeMap;
use std::path::Path;

use crate::trace::diff::{diff, Change, ClauseState};
use crate::trace::proofcheck::ProofReport;
use crate::trace::requirement::{parse_markdown, ParseOutcome, Requirement};

fn show(root: &Path, revision: &str, file: &str) -> Option<String> {
    let out = std::process::Command::new("git").arg("-C").arg(root).arg("show").arg(format!("{revision}:./{file}")).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// @implements REQ-DIFF.base_from_history
pub fn states_at(root: &Path, revision: &str) -> Result<Vec<ClauseState>, String> {
    let text = show(root, revision, ".tracelean/trace.lock")
        .ok_or_else(|| format!("no .tracelean/trace.lock at {revision}"))?;
    let lock = crate::trace::lockfile::parse(&text).map_err(|e| format!("the lock at {revision}: {e}"))?;
    let mut requirements: BTreeMap<String, Requirement> = BTreeMap::new();
    for (id, held) in &lock.requirements {
        let Some(source) = show(root, revision, &held.file) else { continue };
        if let ParseOutcome::Requirement(requirement, _) = parse_markdown(&held.file, &source) {
            requirements.insert(id.clone(), *requirement);
        }
    }
    let proofs: ProofReport = show(root, revision, ".tracelean/proofs.json")
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    Ok(crate::trace::diff::states(&lock, &requirements, &proofs))
}

pub fn changes_since(
    root: &Path,
    revision: &str,
    now: Vec<ClauseState>,
) -> Result<BTreeMap<(String, Option<String>), Vec<Change>>, String> {
    Ok(diff(states_at(root, revision)?, now).into_iter().map(|d| ((d.req_id, d.clause), d.changes)).collect())
}
