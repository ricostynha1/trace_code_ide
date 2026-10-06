//! The committed index.
//!
//! Committed so that a change to what a project claims, or to what backs those
//! claims, shows up in review like any other change. That works only if the
//! serialisation is deterministic — otherwise every diff carries noise and
//! people stop reading them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::index::Index;
use super::record::Evidence;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub requirements: BTreeMap<String, LockRequirement>,
    /// Sorted, for determinism.
    pub links: Vec<LockLink>,
    /// Carried through unchanged: backends own evidence, the scanner only ever
    /// invalidates it.
    ///
    /// @implements REQ-LOCK.evidence_preserved
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockRequirement {
    pub clauses: Vec<String>,
    pub content_hash: String,
    pub file: String,
    pub refines: Vec<String>,
    pub decomposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LockLink {
    pub role: String,
    pub req: String,
    pub clause: Option<String>,
    pub file: String,
    /// Anchor identity: symbol path, region bounds, or the file itself.
    pub anchor: String,
    /// Hash of the anchored body. Drives staleness.
    pub body_hash: String,
    /// Hash of the link's identity. Drives evidence validity.
    pub link_hash: String,
    pub qualifier: Option<String>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Lockfile {
            version: VERSION,
            requirements: BTreeMap::new(),
            links: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

/// Render an index. Pure: no filesystem, so determinism is testable by calling
/// it twice.
///
/// @implements REQ-LOCK.pure_render
/// @implements REQ-LOCK.deterministic_bytes
pub fn render(index: &Index, evidence: Vec<Evidence>) -> Lockfile {
    let requirements = index
        .requirements
        .iter()
        .map(|(id, req)| {
            (
                id.clone(),
                LockRequirement {
                    clauses: req.clauses.keys().cloned().collect(),
                    content_hash: req.content_hash.clone(),
                    file: req.file.clone(),
                    refines: req.refines.clone(),
                    decomposition: req.decomposition.as_str().to_string(),
                },
            )
        })
        .collect();

    let links: Vec<LockLink> = index
        .links
        .iter()
        .map(|link| LockLink {
            role: link.role.as_str().to_string(),
            req: link.req_id.clone(),
            clause: link.clause.clone(),
            file: link.anchor.file.clone(),
            anchor: link.anchor.ident(),
            body_hash: link.anchor.body_hash.clone(),
            link_hash: link.link_hash.clone(),
            qualifier: link.qualifier.as_ref().map(|q| q.as_str().to_string()),
        })
        .collect();
    let links = ordered_links(links);

    Lockfile { version: VERSION, requirements, links, evidence }
}

/// The order links are written in.
///
/// This is where `deterministic_bytes` actually lives. Sorted maps and a stable
/// field order come from the serialiser; the link list is the one part whose
/// order this project chooses, and an order that is not total would let two
/// runs over the same tree produce two different files — which is exactly the
/// diff noise that stops people reading the lockfile.
///
/// @implements REQ-LOCK.deterministic_bytes
/// @implements ARCH-DETERMINISM.stable_ordering
/// @drt REQ-LOCK.deterministic_bytes
/// @drt ARCH-DETERMINISM.stable_ordering
pub fn ordered_links(links: Vec<LockLink>) -> Vec<LockLink> {
    let mut links = links;
    links.sort();
    links
}

/// Serialise deterministically: sorted maps, stable field order, one trailing
/// newline.
///
/// @implements REQ-LOCK.deterministic_bytes
pub fn to_bytes(lockfile: &Lockfile) -> String {
    let mut text = serde_json::to_string_pretty(lockfile).unwrap_or_default();
    text.push('\n');
    text
}

/// What a build can do with a lockfile's version stamp.
///
/// A version it does not know is refused whole. Parsing the parts it recognises
/// would produce an index that is right about some claims and silently missing
/// others, which is worse than no index: nothing downstream could tell the
/// difference between a claim that was dropped and a claim that was never made.
///
/// @implements REQ-LOCK.version_stamped
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VersionVerdict {
    /// This build reads it.
    Readable,
    /// A version from another build. Refused, with both numbers, so the message
    /// can say which way to go.
    Unknown { found: u64, reads: u64 },
    /// No version at all, which is not a version zero.
    Unstamped,
}

/// Whether this build may read a lockfile carrying this version.
///
/// @implements REQ-LOCK.version_stamped
/// @drt REQ-LOCK.version_stamped
pub fn version_verdict(version: Option<u64>) -> VersionVerdict {
    match version {
        None => VersionVerdict::Unstamped,
        Some(v) if v == VERSION as u64 => VersionVerdict::Readable,
        Some(v) => VersionVerdict::Unknown { found: v, reads: VERSION as u64 },
    }
}

/// The evidence a rendered lockfile carries, given the evidence it was handed.
///
/// Rendering does not filter, reorder, summarise or re-judge evidence: a record
/// that went in comes out, byte for byte. Anything else would let the committed
/// artefact disagree with the backend that earned the record, and the artefact
/// is what people read.
///
/// @implements REQ-LOCK.evidence_preserved
/// @drt REQ-LOCK.evidence_preserved
pub fn carried_evidence(evidence: Vec<Evidence>) -> Vec<Evidence> {
    render(&Index::default(), evidence).evidence
}

/// Where a project keeps its lock file.
pub fn path_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("trace.lock")
}

/// Read the committed lock, if there is one.
///
/// Absent is not an error: a project that has never written one has no evidence
/// recorded, which is a state the reports have to be able to say out loud.
pub fn read(root: &Path) -> Option<Lockfile> {
    let text = std::fs::read_to_string(path_in(root)).ok()?;
    parse(&text).ok()
}

/// What a lock should carry, given the tree and what backends have earned.
///
/// Three steps, in this order and no other: take what the previous lock held,
/// fold in what the store has since collected, then drop whatever no longer
/// applies. Sweeping *last* is what makes a fresh record able to replace a stale
/// one — sweeping first would throw away the record the new run was written to
/// replace, and then both would be gone.
///
/// @implements REQ-LOCK.evidence_preserved
/// @implements REQ-STALE.change_invalidates
/// @implements REQ-STALE.no_silent_revalidation
pub fn collected(index: &Index, held: Vec<Evidence>, earned: Vec<Evidence>) -> Collected {
    let merged = super::earn::merge(held, earned);
    let (valid, dropped) = super::earn::still_standing(index, merged);
    Collected { lockfile: render(index, valid), dropped }
}

/// A lock and what was left out of it.
///
/// The dropped records come back rather than vanishing, because "three records
/// went stale" and "three records were never earned" call for different actions
/// and look identical in a lock file that just has fewer entries.
///
/// @implements REQ-STALE.stale_is_visible
#[derive(Debug, Clone, PartialEq)]
pub struct Collected {
    pub lockfile: Lockfile,
    pub dropped: Vec<Evidence>,
}

/// Write the lock where the project keeps it, and answer with its path.
///
/// The bytes come from `to_bytes`, which is a function of the index alone — so
/// writing twice without changing anything writes the same file.
///
/// @implements REQ-LOCK.deterministic_bytes
/// @implements REQ-LOCK.diff_is_meaningful
pub fn write(root: &Path, lockfile: &Lockfile) -> std::io::Result<PathBuf> {
    let path = path_in(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, to_bytes(lockfile))?;
    Ok(path)
}

/// Read a lockfile, refusing a version this build does not know.
///
/// @implements REQ-LOCK.version_stamped
pub fn parse(text: &str) -> Result<Lockfile, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not readable JSON: {e}"))?;
    match version_verdict(value.get("version").and_then(|v| v.as_u64())) {
        VersionVerdict::Readable => {}
        VersionVerdict::Unknown { found, reads } => {
            return Err(format!("lockfile version {found}, this build reads {reads}"))
        }
        VersionVerdict::Unstamped => return Err("lockfile carries no version".into()),
    }
    serde_json::from_value(value).map_err(|e| format!("not a lockfile: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::index::build;

    fn index_of(files: &[(&str, &str)]) -> (std::path::PathBuf, Index) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tracelean-lock-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, content) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let index = build(&dir);
        (dir, index)
    }

    const FILES: &[(&str, &str)] = &[
        ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
        ("src/i.rs", "// @implements REQ-A.one\npub fn f() {}\n"),
    ];

    /// @tests REQ-LOCK.deterministic_bytes
    #[test]
    fn the_same_tree_serialises_to_the_same_bytes() {
        let (dir, index) = index_of(FILES);
        let a = to_bytes(&render(&index, vec![]));
        let b = to_bytes(&render(&build(&dir), vec![]));
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// @tests REQ-LOCK.diff_is_meaningful
    #[test]
    fn a_body_change_moves_the_bytes() {
        let (dir, index) = index_of(FILES);
        let before = to_bytes(&render(&index, vec![]));
        std::fs::write(dir.join("src/i.rs"), "// @implements REQ-A.one\npub fn f() { 1 }\n")
            .unwrap();
        let after = to_bytes(&render(&build(&dir), vec![]));
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// @tests REQ-LOCK.version_stamped
    #[test]
    fn an_unknown_version_is_refused_not_partly_parsed() {
        let text = "{\"version\": 99, \"requirements\": {}, \"links\": [], \"evidence\": []}";
        assert!(parse(text).unwrap_err().contains("version 99"));
        assert!(parse("{\"requirements\": {}}").unwrap_err().contains("no version"));
    }

    #[test]
    fn a_rendered_lockfile_round_trips() {
        let (dir, index) = index_of(FILES);
        let lockfile = render(&index, vec![]);
        assert_eq!(parse(&to_bytes(&lockfile)).unwrap(), lockfile);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
