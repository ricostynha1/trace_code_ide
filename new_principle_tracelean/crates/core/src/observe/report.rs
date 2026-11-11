use std::path::{Path, PathBuf};

use crate::trace;

pub struct Written {
    pub page: PathBuf,
    pub clauses: usize,
    pub held: [usize; 4],
    pub changed: Option<(usize, usize)>,
    pub findings: usize,
}

/// @implements REQ-REPORT.static_page
/// @implements REQ-REPORT.changes_marked
pub fn write(root: &Path, index: &trace::index::Index, out_dir: &Path, base: Option<&str>) -> Result<Written, String> {
    let files = super::workspace::snapshot(root).files;
    let lock = crate::drt::proofs::standing_lock(root, index);
    let proofs = crate::drt::proofs::load(root);
    let states = trace::diff::states(&lock, &index.requirements, &proofs);
    let changes = base
        .map(|b| super::revision::changes_since(root, b, states.clone()))
        .transpose()?
        .unwrap_or_default();
    let policy = trace::checker::Policy::default();
    let mut findings = trace::checker::check(index, &policy);
    findings.extend(trace::checker::refused(&proofs, &policy));
    let strengths = trace::proofcheck::strengths(index, &proofs, &files);
    let shared = crate::surface::report::Shared {
        index,
        lock: &lock,
        proofs: &proofs,
        strengths: &strengths,
        findings: &findings,
        files: &files,
        changes: &changes,
        base,
    };
    let page = crate::surface::report::page(&shared, &states);
    let target = out_dir.join("index.html");
    std::fs::create_dir_all(out_dir)
        .and_then(|_| std::fs::write(&target, page))
        .map_err(|why| format!("cannot write the report: {why}"))?;
    let count = |mark| states.iter().filter(|s| s.held.contains(&mark)).count();
    use crate::evidence::Mark;
    Ok(Written {
        page: target.canonicalize().unwrap_or(target),
        clauses: states.len(),
        held: [count(Mark::Annotated), count(Mark::Judged), count(Mark::Tested), count(Mark::Proved)],
        changed: base.map(|_| (changes.len(), changes.values().filter(|c| c.iter().any(trace::diff::Change::is_loss)).count())),
        findings: findings.len(),
    })
}
