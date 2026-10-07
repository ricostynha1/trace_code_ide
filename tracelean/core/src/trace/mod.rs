//! Traceability: requirements linked to models, code and tests by annotations
//! written in comments — never by filename, directory layout or any other
//! convention imposed on the project being traced.
//!
//! The pipeline is: scan requirement documents, scan annotations across source
//! files, resolve each annotation to a stable anchor, join with evidence
//! carried in the lockfile, then run the checker. Nothing here runs a prover
//! or a test; it decides what is claimed, what is backed, and what has gone
//! stale.

pub mod annotation;
pub mod anchor;
pub mod checker;
pub mod evidence;
pub mod hash;
pub mod history;
pub mod lockfile;
pub mod graph;
pub mod map;
pub mod requirement;
pub mod coverage;
pub mod test_results;
pub mod strength;
pub mod rollup;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use anchor::{Anchor, AnchorKind};
pub use history::{history, HistoryPoint};
pub use annotation::{AnnotationProblem, AnnotationProblemKind, Qualifier, Role};
pub use checker::{Finding, FindingKind, Policy, Severity};
pub use evidence::{Assurance, Bond, EvidenceDetail, EvidenceKey, EvidenceRecord, Level};
pub use requirement::{Decomposition, ParseOutcome, ReqStatus, Requirement};
pub use map::{CoverageMap, MapEntry};
pub use rollup::{RollUp, TreeNode};

/// A resolved link: an annotation that found its anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    pub qualifier: Option<Qualifier>,
    pub attrs: BTreeMap<String, String>,
    pub anchor: Anchor,
    /// Identity hash — changes when the link is retargeted, so evidence keyed
    /// on it cannot be silently inherited by a different claim.
    pub link_hash: String,
    pub line: u32,
}

impl Link {
    pub fn is_exempt(&self) -> bool {
        matches!(self.qualifier, Some(Qualifier::Exempt { .. }))
    }

    pub fn is_partial(&self) -> bool {
        matches!(self.qualifier, Some(Qualifier::Partial { .. }))
    }

    pub fn is_exclusive(&self) -> bool {
        self.attrs.contains_key("exclusive")
    }
}

/// Everything the traceability system knows about a project.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TraceIndex {
    pub requirements: BTreeMap<String, Requirement>,
    pub links: Vec<Link>,
    pub findings: Vec<Finding>,
    pub evidence: Vec<EvidenceRecord>,
    /// Files that were scanned, for incremental rebuilds later.
    pub scanned_files: BTreeSet<PathBuf>,
    /// `(requirement, clause)` pairs that `.tracelean/drt.json` binds to a
    /// differential test.
    ///
    /// The binding, not a comment, is what binds a model to an implementation:
    /// with TraceLean's shipped runner calling the function directly there is
    /// no adapter file to carry a `@drt` annotation, and inventing one purely
    /// so a comment exists somewhere would be a convention of exactly the kind
    /// this project refuses.
    #[serde(default)]
    pub drt_bindings: BTreeSet<(String, Option<String>)>,
}

/// Directories never worth walking.
const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", ".venv", "venv", "__pycache__",
    ".tracelean", "build", ".next", ".cache", "vendor", ".lake",
];

/// Extensions that can carry annotations. Markdown is scanned as a requirement
/// document instead, so it is not here.
fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "rs" | "py" | "lean" | "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "js" | "mjs"
                | "cjs" | "ts" | "tsx" | "jsx" | "go" | "java" | "rb" | "sh" | "toml" | "yaml"
                | "yml"
        )
    )
}

/// Build the whole index from the filesystem.
pub fn build(root: &Path) -> TraceIndex {
    let mut index = TraceIndex::default();

    // Evidence is authored by backends and only ever invalidated here, so it
    // is loaded first and carried through untouched.
    index.evidence = lockfile::load(root).map(|l| l.evidence).unwrap_or_default();

    let mut files = Vec::new();
    collect_files(root, root, &mut files);

    for path in files {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();

        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            match requirement::parse_markdown(&path, root, &content) {
                ParseOutcome::Requirement(req, problems) => {
                    for p in problems {
                        index.findings.push(Finding::frontmatter(p));
                    }
                    if let Some(previous) = index.requirements.insert(req.id.clone(), *req) {
                        index.findings.push(Finding::duplicate_id(&previous));
                    }
                }
                ParseOutcome::NotARequirement => {}
            }
            index.scanned_files.insert(rel);
            continue;
        }

        if !is_source(&path) {
            continue;
        }

        // Recorded whether or not it carries annotations: an untraced file is
        // exactly what the coverage map needs to show, and a scan that only
        // remembered annotated files would flatter the project.
        index.scanned_files.insert(rel);

        let scan = annotation::scan_file(&path, root, &content);
        if scan.annotations.is_empty() && scan.problems.is_empty() {
            continue;
        }
        for p in scan.problems.iter() {
            index.findings.push(Finding::annotation(p.clone()));
        }
        for (ann, anchor) in anchor::resolve_all(&path, root, &content, &scan) {
            let link_hash = hash::hash_link(
                ann.role.as_str(),
                &ann.req_id,
                ann.clause.as_deref(),
                &ann.attrs,
                &anchor.ident(),
            );
            index.links.push(Link {
                role: ann.role,
                req_id: ann.req_id,
                clause: ann.clause,
                qualifier: ann.qualifier,
                attrs: ann.attrs,
                anchor,
                link_hash,
                line: ann.line,
            });
        }
    }

    index.drt_bindings = crate::drt::DrtConfig::load(root)
        .map(|config| {
            config
                .bindings
                .iter()
                .map(|b| (b.req_id.clone(), b.clause.clone()))
                .collect()
        })
        .unwrap_or_default();

    let policy = Policy::load_or_default(root);
    let mut findings = checker::check(&index, &policy);
    index.findings.append(&mut findings);
    index.findings.sort_by(|a, b| {
        (a.severity, &a.file, a.line, a.kind).cmp(&(b.severity, &b.file, b.line, b.kind))
    });

    index
}

fn collect_files(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.') && name != ".tracelean" {
                continue;
            }
            collect_files(&path, root, out);
        } else {
            out.push(path);
        }
    }
}

impl TraceIndex {
    /// All links touching a requirement (any clause).
    pub fn links_for(&self, req_id: &str) -> Vec<&Link> {
        self.links.iter().filter(|l| l.req_id == req_id).collect()
    }

    /// Links for one clause. A link written without a clause applies to every
    /// clause of its requirement, so a requirement-level `@implements` still
    /// counts for each clause rather than for none.
    pub fn links_for_clause(&self, req_id: &str, clause: Option<&str>) -> Vec<&Link> {
        self.links
            .iter()
            .filter(|l| l.req_id == req_id)
            .filter(|l| match (&l.clause, clause) {
                (Some(a), Some(b)) => a == b,
                (None, _) => true,
                (Some(_), None) => false,
            })
            .collect()
    }

    pub fn has_role(&self, req_id: &str, clause: Option<&str>, role: Role) -> bool {
        self.links_for_clause(req_id, clause)
            .iter()
            .any(|l| l.role == role && !l.is_exempt())
    }

    /// Current hashes of every input a piece of evidence about this clause
    /// could depend on, for staleness comparison.
    pub fn current_hashes(&self, req_id: &str, clause: Option<&str>) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if let Some(req) = self.requirements.get(req_id) {
            let clause_text = clause
                .and_then(|c| req.clauses.get(c))
                .cloned()
                .unwrap_or_else(|| req.body.clone());
            out.insert("clause".into(), hash::hash_clause(&clause_text));
        }
        for link in self.links_for_clause(req_id, clause) {
            let slot = match link.role {
                Role::Models => "model",
                Role::Implements => "impl",
                Role::Drt => "adapter",
                Role::Tests => "test",
                Role::Proves => "proof",
                Role::Pins => "pins",
            };
            // Several links can fill one slot; combine deterministically.
            out.entry(slot.to_string())
                .and_modify(|existing| {
                    *existing = hash::hash_body(&format!("{existing}+{}", link.anchor.body_hash))
                })
                .or_insert_with(|| link.anchor.body_hash.clone());
        }
        out
    }

    /// Assurance for one clause, derived from evidence and staleness.
    pub fn assurance(&self, req_id: &str, clause: Option<&str>) -> Assurance {
        let mut out = Assurance::default();
        let current = self.current_hashes(req_id, clause);
        for record in &self.evidence {
            if record.key.req_id != req_id || record.key.clause.as_deref() != clause {
                continue;
            }
            if record.stale_inputs(&current).is_empty() {
                out.set(record.key.bond, record.level);
            } else {
                out.stale.push(record.key.bond);
            }
        }
        out
    }
}

// --- Views for the UI ---------------------------------------------------

/// One clause, with everything known about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClauseView {
    pub key: Option<String>,
    pub text: String,
    pub assurance: Assurance,
    /// Weakest bond that has evidence — what the badge shows.
    pub weakest: Level,
    pub exempt: bool,
    pub partial: bool,
    /// Whether the proved properties are known to determine the model.
    ///
    /// This is the *declared* state, which costs nothing to compute. Turning
    /// `attempted` into `pinned` needs the kernel's opinion, so it is asked for
    /// separately rather than on every rescan.
    pub strength: strength::Strength,
    pub links: Vec<Link>,
}

/// A requirement in full.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementView {
    pub requirement: Requirement,
    pub clauses: Vec<ClauseView>,
    /// Fraction of non-exempt clauses carrying any evidence above L1.
    pub coverage: f32,
    /// True when `decomposition: open`, in which case coverage must render as
    /// "≥ x%" — the denominator is not known to be complete.
    pub coverage_is_lower_bound: bool,
    pub findings: Vec<Finding>,
    pub children: Vec<String>,
}

/// Row in the requirements panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementSummary {
    pub id: String,
    pub title: String,
    pub file: PathBuf,
    pub refines: Vec<String>,
    pub children: Vec<String>,
    pub decomposition: Decomposition,
    pub status: ReqStatus,
    pub clause_count: usize,
    pub coverage: f32,
    pub coverage_is_lower_bound: bool,
    pub weakest: Level,
    pub stale_count: usize,
    pub error_count: usize,
    pub warn_count: usize,
}

impl TraceIndex {
    /// Direct children in the refinement DAG.
    pub fn children_of(&self, req_id: &str) -> Vec<String> {
        self.requirements
            .values()
            .filter(|r| r.refines.iter().any(|p| p == req_id))
            .map(|r| r.id.clone())
            .collect()
    }

    pub fn requirement_view(&self, req_id: &str) -> Option<RequirementView> {
        let req = self.requirements.get(req_id)?;
        let mut clauses = Vec::new();

        for key in req.clause_keys() {
            let k = key.as_deref();
            let links: Vec<Link> = self.links_for_clause(req_id, k).into_iter().cloned().collect();
            let assurance = self.assurance(req_id, k);
            clauses.push(ClauseView {
                text: key
                    .as_ref()
                    .and_then(|c| req.clauses.get(c))
                    .cloned()
                    .unwrap_or_else(|| req.body.clone()),
                weakest: assurance.weakest(),
                exempt: links.iter().any(|l| l.is_exempt()),
                partial: links.iter().any(|l| l.is_partial()),
                strength: strength::declared(self, req_id, k),
                assurance,
                links,
                key,
            });
        }

        let (coverage, lower_bound) = coverage_of(&clauses, req.decomposition);

        Some(RequirementView {
            requirement: req.clone(),
            clauses,
            coverage,
            coverage_is_lower_bound: lower_bound,
            findings: self
                .findings
                .iter()
                .filter(|f| f.req_id.as_deref() == Some(req_id))
                .cloned()
                .collect(),
            children: self.children_of(req_id),
        })
    }

    pub fn overview(&self) -> Vec<RequirementSummary> {
        self.requirements
            .values()
            .map(|req| {
                let view = self.requirement_view(&req.id);
                let (coverage, lower_bound, weakest, clause_count) = match &view {
                    Some(v) => (
                        v.coverage,
                        v.coverage_is_lower_bound,
                        v.clauses
                            .iter()
                            .filter(|c| !c.exempt)
                            .map(|c| c.weakest)
                            .min()
                            .unwrap_or(Level::L1),
                        v.clauses.len(),
                    ),
                    None => (0.0, true, Level::L1, 0),
                };
                let findings: Vec<&Finding> = self
                    .findings
                    .iter()
                    .filter(|f| f.req_id.as_deref() == Some(req.id.as_str()))
                    .collect();
                RequirementSummary {
                    id: req.id.clone(),
                    title: req.title.clone(),
                    file: req.file.clone(),
                    refines: req.refines.clone(),
                    children: self.children_of(&req.id),
                    decomposition: req.decomposition,
                    status: req.status,
                    clause_count,
                    coverage,
                    coverage_is_lower_bound: lower_bound,
                    weakest,
                    stale_count: findings
                        .iter()
                        .filter(|f| f.kind == FindingKind::Stale)
                        .count(),
                    error_count: findings
                        .iter()
                        .filter(|f| f.severity == Severity::Error)
                        .count(),
                    warn_count: findings
                        .iter()
                        .filter(|f| f.severity == Severity::Warn)
                        .count(),
                }
            })
            .collect()
    }
}

/// Coverage over non-exempt clauses, plus whether it is only a lower bound.
///
/// An `open` decomposition means the children are not claimed to exhaust the
/// parent, so the denominator is unknown and the number must be rendered as
/// "≥ x%". Requiring authors to claim `complete` before an exact percentage is
/// shown is what keeps the metric honest.
fn coverage_of(clauses: &[ClauseView], decomposition: Decomposition) -> (f32, bool) {
    let counted: Vec<&ClauseView> = clauses.iter().filter(|c| !c.exempt).collect();
    if counted.is_empty() {
        return (1.0, decomposition == Decomposition::Open);
    }
    let total: f32 = counted
        .iter()
        .map(|c| {
            let has_evidence = c.assurance.requirement_model.is_some()
                || c.assurance.model_impl.is_some()
                || c.assurance.model_proof.is_some();
            let claimed = c.links.iter().any(|l| l.role == Role::Implements);
            // Partial claims cannot reach a full clause.
            let value: f32 = if has_evidence {
                1.0
            } else if claimed {
                0.5
            } else {
                0.0
            };
            if c.partial { value.min(0.5) } else { value }
        })
        .sum();
    (
        total / counted.len() as f32,
        decomposition == Decomposition::Open,
    )
}
