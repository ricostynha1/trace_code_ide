//! `.tracelean/trace.lock.json` — the derived, committed view of the graph.
//!
//! Two properties matter more than the schema: it is **deterministic** (the
//! same repository state serializes to identical bytes, so a diff means
//! something changed), and it **preserves evidence** (the scanner may mark a
//! record stale but never rewrites or drops one, because the backends own
//! them).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::evidence::EvidenceRecord;
use super::{Link, TraceIndex};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    /// Requirement id → its clause keys and content hash.
    pub requirements: BTreeMap<String, LockRequirement>,
    /// Sorted for determinism.
    pub links: Vec<LockLink>,
    pub evidence: Vec<EvidenceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockRequirement {
    pub clauses: Vec<String>,
    pub content_hash: String,
    pub file: PathBuf,
    pub refines: Vec<String>,
    pub decomposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LockLink {
    pub role: String,
    pub req: String,
    pub clause: Option<String>,
    pub file: PathBuf,
    /// Anchor identity: symbol path, region bounds, or whole-file marker.
    pub anchor: String,
    /// Hash of the anchored body, comments excluded. Drives staleness.
    pub body_hash: String,
    /// Hash of the link's identity. Drives evidence validity.
    pub link_hash: String,
    pub qualifier: Option<String>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: VERSION,
            requirements: BTreeMap::new(),
            links: Vec::new(),
            evidence: Vec::new(),
        }
    }
}

pub fn path_for(root: &Path) -> PathBuf {
    root.join(".tracelean").join("trace.lock.json")
}

pub fn load(root: &Path) -> Option<Lockfile> {
    let text = std::fs::read_to_string(path_for(root)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Render an index as a lockfile. Pure: no IO, so determinism is testable.
pub fn render(index: &TraceIndex) -> Lockfile {
    let mut requirements = BTreeMap::new();
    for (id, req) in &index.requirements {
        requirements.insert(
            id.clone(),
            LockRequirement {
                clauses: req.clauses.keys().cloned().collect(),
                content_hash: req.content_hash.clone(),
                file: req.file.clone(),
                refines: {
                    let mut r = req.refines.clone();
                    r.sort();
                    r
                },
                decomposition: req.decomposition.as_str().to_string(),
            },
        );
    }

    let mut links: Vec<LockLink> = index.links.iter().map(lock_link).collect();
    links.sort();
    links.dedup();

    let mut evidence = index.evidence.clone();
    evidence.sort_by(|a, b| {
        (&a.key, a.detail.backend(), &a.at).cmp(&(&b.key, b.detail.backend(), &b.at))
    });

    Lockfile { version: VERSION, requirements, links, evidence }
}

fn lock_link(link: &Link) -> LockLink {
    LockLink {
        role: link.role.as_str().to_string(),
        req: link.req_id.clone(),
        clause: link.clause.clone(),
        file: link.anchor.file.clone(),
        anchor: link.anchor.ident(),
        body_hash: link.anchor.body_hash.clone(),
        link_hash: link.link_hash.clone(),
        qualifier: link.qualifier.as_ref().map(|q| q.as_str().to_string()),
    }
}

/// Serialize deterministically: sorted maps, sorted vectors, stable field
/// order, trailing newline.
pub fn to_string(lock: &Lockfile) -> Result<String, String> {
    serde_json::to_string_pretty(lock)
        .map(|s| s + "\n")
        .map_err(|e| format!("serializing lockfile: {e}"))
}

pub fn save(root: &Path, index: &TraceIndex) -> Result<PathBuf, String> {
    let lock = render(index);
    let text = to_string(&lock)?;
    let path = path_for(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// Append or replace one evidence record, keyed by (requirement, clause, bond,
/// backend). Backends call this; the scanner never does.
pub fn put_evidence(root: &Path, record: EvidenceRecord) -> Result<(), String> {
    let mut lock = load(root).unwrap_or_default();
    lock.evidence.retain(|r| {
        !(r.key == record.key && r.detail.backend() == record.detail.backend())
    });
    lock.evidence.push(record);
    lock.evidence.sort_by(|a, b| {
        (&a.key, a.detail.backend(), &a.at).cmp(&(&b.key, b.detail.backend(), &b.at))
    });
    let text = to_string(&lock)?;
    let path = path_for(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}
