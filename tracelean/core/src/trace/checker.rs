//! The checker: what is claimed, what is missing, and what has gone stale.
//!
//! Findings are named rather than lumped into one "traceability error",
//! because the name is what makes a report actionable in a diff. Most of them
//! are *progress*, not faults: a requirement with no model yet is information,
//! not a broken build. Only a small default set blocks.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::annotation::{AnnotationProblem, AnnotationProblemKind, Qualifier};
use super::evidence::Bond;
use super::requirement::{FrontmatterProblem, Requirement};
use super::{Link, Role, TraceIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// An annotation names a requirement or clause that does not exist.
    Dangling,
    /// `refines:` names a requirement that does not exist.
    DanglingRefines,
    /// The refinement DAG contains a cycle.
    RefinesCycle,
    /// Two requirement documents declare the same id.
    DuplicateId,
    /// Evidence exists but one of its inputs has changed.
    Stale,
    /// A clause has no `@models` link and is not exempt.
    Unmodeled,
    /// Modeled but nothing claims to implement it.
    Unimplemented,
    /// Model and implementation both exist, but no harness binds them — the
    /// most important finding in the system: a model nobody checks.
    Unbound,
    /// Implemented but no test.
    Untested,
    /// The judge found confirmed drift (derived from evidence records).
    JudgeDrift,
    /// The judge's claims about the model were falsified twice over. A fact
    /// about the judge, not about the requirement — reported so a misreading
    /// never masquerades as a requirement problem.
    JudgeUnreliable,
    /// Differential testing found a mismatch (derived from evidence records).
    Divergence,
    /// Two links exclusively claim the same clause.
    Contested,
    /// `@exempt` without a reason or approver, or past its expiry.
    UnsoundExemption,
    /// `@partial` without a reason.
    UnsoundQualifier,
    /// A word that looks like a role but is not one.
    UnknownRole,
    /// Frontmatter that could not be parsed.
    BadFrontmatter,
    /// A `begin` region that is never closed, or a stray `@end`.
    BadRegion,
    /// The file had no grammar, so the link is whole-file and capped at L1.
    NoAnchorPrecision,
    /// A policy `require` rule is not met.
    PolicyUnmet,
    /// A clause is proved, but nothing says the proved properties *determine*
    /// the model.
    ///
    /// L3 is qualified by a coverage floor; without this, L4 is qualified by
    /// nothing, and `discountCents s ≤ s` -- a theorem the constant-zero
    /// function also satisfies -- reads exactly like a proof that pins the
    /// model down. Info rather than a warning: an unanswered question, not a
    /// defect.
    UnpinnedProof,
}

impl FindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FindingKind::UnpinnedProof => "unpinned-proof",
            FindingKind::Dangling => "dangling",
            FindingKind::DanglingRefines => "dangling-refines",
            FindingKind::RefinesCycle => "refines-cycle",
            FindingKind::DuplicateId => "duplicate-id",
            FindingKind::Stale => "stale",
            FindingKind::Unmodeled => "unmodeled",
            FindingKind::Unimplemented => "unimplemented",
            FindingKind::Unbound => "unbound",
            FindingKind::Untested => "untested",
            FindingKind::JudgeDrift => "judge-drift",
            FindingKind::JudgeUnreliable => "judge-unreliable",
            FindingKind::Divergence => "divergence",
            FindingKind::Contested => "contested",
            FindingKind::UnsoundExemption => "unsound-exemption",
            FindingKind::UnsoundQualifier => "unsound-qualifier",
            FindingKind::UnknownRole => "unknown-role",
            FindingKind::BadFrontmatter => "bad-frontmatter",
            FindingKind::BadRegion => "bad-region",
            FindingKind::NoAnchorPrecision => "no-anchor-precision",
            FindingKind::PolicyUnmet => "policy-unmet",
        }
    }

    /// Severity before the project's policy has its say.
    pub fn default_severity(&self) -> Severity {
        use FindingKind::*;
        match self {
            Dangling | DanglingRefines | RefinesCycle | DuplicateId | UnsoundExemption
            | UnsoundQualifier | JudgeDrift | Divergence | PolicyUnmet | BadFrontmatter => {
                Severity::Error
            }
            Stale | Untested | Unbound | Contested | UnknownRole | BadRegion
            | JudgeUnreliable => Severity::Warn,
            Unmodeled | Unimplemented | NoAnchorPrecision | UnpinnedProof => Severity::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    pub message: String,
    pub file: PathBuf,
    pub line: u32,
    pub req_id: Option<String>,
    pub clause: Option<String>,
    /// True when this finding is on the policy's `block_on` list, i.e. it fails
    /// the CI gate.
    ///
    /// Carried on the finding rather than recomputed by each consumer so the
    /// panel's "errors" filter and the gate can never disagree about what an
    /// error is — the surest way to make a dashboard untrustworthy is to let it
    /// answer a slightly different question than the build does.
    #[serde(default)]
    pub blocking: bool,
}

impl Finding {
    fn new(kind: FindingKind, message: String, file: PathBuf, line: u32) -> Self {
        Self {
            kind,
            severity: kind.default_severity(),
            message,
            file,
            line,
            req_id: None,
            clause: None,
            blocking: false,
        }
    }

    fn about(mut self, req_id: &str, clause: Option<&str>) -> Self {
        self.req_id = Some(req_id.to_string());
        self.clause = clause.map(|c| c.to_string());
        self
    }

    pub(super) fn frontmatter(p: FrontmatterProblem) -> Self {
        Finding::new(FindingKind::BadFrontmatter, p.message, p.file, p.line)
    }

    pub(super) fn duplicate_id(previous: &Requirement) -> Self {
        Finding::new(
            FindingKind::DuplicateId,
            format!(
                "requirement id `{}` is declared by more than one document (previously {})",
                previous.id,
                previous.file.display()
            ),
            previous.file.clone(),
            0,
        )
        .about(&previous.id, None)
    }

    pub(super) fn annotation(p: AnnotationProblem) -> Self {
        let kind = match p.kind {
            AnnotationProblemKind::UnknownRole => FindingKind::UnknownRole,
            AnnotationProblemKind::UnclosedRegion | AnnotationProblemKind::StrayEnd => {
                FindingKind::BadRegion
            }
            AnnotationProblemKind::OrphanQualifier => FindingKind::UnsoundQualifier,
        };
        Finding::new(kind, p.message, p.file, p.line)
    }
}

/// Project policy: severity overrides, minimum-level rules, and what blocks CI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub severity: BTreeMap<String, Severity>,
    #[serde(default)]
    pub require: Vec<RequireRule>,
    #[serde(default = "default_block_on")]
    pub block_on: Vec<String>,
    /// Requirement-id globs whose judgements get a second opinion by
    /// backtranslation. Empty by default: the extra calls cost money, and most
    /// requirements do not warrant them.
    #[serde(default)]
    pub high_value: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequireRule {
    /// Glob over requirement ids, e.g. `REQ-SEC-*`.
    pub r#match: String,
    pub min_level: String,
    pub bond: String,
}

fn default_block_on() -> Vec<String> {
    vec![
        "dangling".into(),
        "broken-anchor".into(),
        "divergence".into(),
        "unsound-exemption".into(),
        "unsound-qualifier".into(),
    ]
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            severity: BTreeMap::new(),
            require: Vec::new(),
            block_on: default_block_on(),
            high_value: Vec::new(),
        }
    }
}

impl Policy {
    /// Load `.tracelean/trace_policy.json`, writing the default if absent so
    /// the knobs are discoverable rather than hidden in documentation.
    pub fn load_or_default(root: &Path) -> Policy {
        let path = root.join(".tracelean").join("trace_policy.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(policy) = serde_json::from_str(&text) {
                return policy;
            }
        }
        let policy = Policy::default();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&policy) {
            let _ = std::fs::write(&path, text + "\n");
        }
        policy
    }

    fn severity_for(&self, kind: FindingKind) -> Severity {
        self.severity
            .get(kind.as_str())
            .copied()
            .unwrap_or_else(|| kind.default_severity())
    }

    pub fn blocks(&self, kind: FindingKind) -> bool {
        self.block_on.iter().any(|k| k == kind.as_str())
    }
}

/// Run every check over a built index.
pub fn check(index: &TraceIndex, policy: &Policy) -> Vec<Finding> {
    let mut out = Vec::new();

    check_dangling(index, &mut out);
    check_refines(index, &mut out);
    check_qualifiers(index, &mut out);
    check_precision(index, &mut out);
    check_coverage(index, &mut out);
    check_contested(index, &mut out);
    check_evidence(index, &mut out);
    check_strength(index, &mut out);
    check_policy_rules(index, policy, &mut out);

    for finding in out.iter_mut() {
        finding.severity = policy.severity_for(finding.kind);
        finding.blocking = policy.blocks(finding.kind);
    }
    out
}

fn check_dangling(index: &TraceIndex, out: &mut Vec<Finding>) {
    for link in &index.links {
        let Some(req) = index.requirements.get(&link.req_id) else {
            out.push(
                Finding::new(
                    FindingKind::Dangling,
                    format!("`@{} {}` names no known requirement", link.role.as_str(), link.req_id),
                    link.anchor.file.clone(),
                    link.line,
                )
                .about(&link.req_id, link.clause.as_deref()),
            );
            continue;
        };
        if let Some(clause) = &link.clause {
            if !req.has_clause(clause) {
                out.push(
                    Finding::new(
                        FindingKind::Dangling,
                        format!("requirement `{}` has no clause `{clause}`", link.req_id),
                        link.anchor.file.clone(),
                        link.line,
                    )
                    .about(&link.req_id, Some(clause)),
                );
            }
        }
    }
}

fn check_refines(index: &TraceIndex, out: &mut Vec<Finding>) {
    for req in index.requirements.values() {
        for parent in &req.refines {
            if !index.requirements.contains_key(parent) {
                out.push(
                    Finding::new(
                        FindingKind::DanglingRefines,
                        format!("`{}` refines `{parent}`, which does not exist", req.id),
                        req.file.clone(),
                        0,
                    )
                    .about(&req.id, None),
                );
            }
        }
    }

    // Cycle detection over the refinement DAG.
    let mut state: BTreeMap<&str, u8> = BTreeMap::new(); // 0 unvisited, 1 on stack, 2 done
    for id in index.requirements.keys() {
        if let Some(cycle) = find_cycle(index, id, &mut state) {
            let req = &index.requirements[&cycle];
            out.push(
                Finding::new(
                    FindingKind::RefinesCycle,
                    format!("`{cycle}` is part of a cycle in the refinement graph"),
                    req.file.clone(),
                    0,
                )
                .about(&cycle, None),
            );
        }
    }
}

fn find_cycle<'a>(index: &'a TraceIndex, id: &str, state: &mut BTreeMap<&'a str, u8>) -> Option<String> {
    // Borrow-friendly iterative DFS over ids.
    let Some((key, req)) = index.requirements.get_key_value(id) else { return None };
    match state.get(key.as_str()) {
        Some(1) => return Some(id.to_string()),
        Some(2) => return None,
        _ => {}
    }
    state.insert(key.as_str(), 1);
    for parent in &req.refines {
        if let Some(found) = find_cycle(index, parent, state) {
            state.insert(key.as_str(), 2);
            return Some(found);
        }
    }
    state.insert(key.as_str(), 2);
    None
}

fn check_qualifiers(index: &TraceIndex, out: &mut Vec<Finding>) {
    let today = now_date();
    for link in &index.links {
        match &link.qualifier {
            Some(Qualifier::Partial { reason }) if reason.is_none() => out.push(
                Finding::new(
                    FindingKind::UnsoundQualifier,
                    "`@partial` must carry reason=\"…\"".into(),
                    link.anchor.file.clone(),
                    link.line,
                )
                .about(&link.req_id, link.clause.as_deref()),
            ),
            Some(Qualifier::Exempt { reason, by, until }) => {
                if reason.is_none() || by.is_none() {
                    out.push(
                        Finding::new(
                            FindingKind::UnsoundExemption,
                            "`@exempt` must carry reason=\"…\" and by=…".into(),
                            link.anchor.file.clone(),
                            link.line,
                        )
                        .about(&link.req_id, link.clause.as_deref()),
                    );
                }
                if let Some(until) = until {
                    if until.as_str() < today.as_str() {
                        out.push(
                            Finding::new(
                                FindingKind::UnsoundExemption,
                                format!("exemption expired on {until}"),
                                link.anchor.file.clone(),
                                link.line,
                            )
                            .about(&link.req_id, link.clause.as_deref()),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn check_precision(index: &TraceIndex, out: &mut Vec<Finding>) {
    for link in &index.links {
        if !link.anchor.precise {
            out.push(
                Finding::new(
                    FindingKind::NoAnchorPrecision,
                    "no grammar for this file: the link anchors to the whole file and is capped at L1"
                        .into(),
                    link.anchor.file.clone(),
                    link.line,
                )
                .about(&link.req_id, link.clause.as_deref()),
            );
        }
    }
}

/// Coverage findings. Exemption suppresses them; partial does not.
fn check_coverage(index: &TraceIndex, out: &mut Vec<Finding>) {
    for req in index.requirements.values() {
        for clause in req.clause_keys() {
            let c = clause.as_deref();
            let links = index.links_for_clause(&req.id, c);
            if links.iter().any(|l| l.is_exempt()) {
                continue;
            }

            let has_model = links.iter().any(|l| l.role == Role::Models);
            let has_impl = links.iter().any(|l| l.role == Role::Implements);
            let has_test = links.iter().any(|l| l.role == Role::Tests);
            // Either a `@drt` annotation on a hand-written adapter, or a
            // binding in `.tracelean/drt.json` -- which is the ordinary case,
            // because the shipped runner needs no adapter to annotate.
            let has_drt = links.iter().any(|l| l.role == Role::Drt)
                || index.drt_bindings.contains(&(req.id.clone(), clause.clone()))
                || index.drt_bindings.contains(&(req.id.clone(), None));

            let where_ = (req.file.clone(), 0u32);

            if !has_model {
                out.push(
                    Finding::new(
                        FindingKind::Unmodeled,
                        format!("{} has no `@models` link", label(&req.id, c)),
                        where_.0.clone(),
                        where_.1,
                    )
                    .about(&req.id, c),
                );
            }
            if has_model && !has_impl {
                out.push(
                    Finding::new(
                        FindingKind::Unimplemented,
                        format!("{} is modeled but nothing implements it", label(&req.id, c)),
                        where_.0.clone(),
                        where_.1,
                    )
                    .about(&req.id, c),
                );
            }
            if has_model && has_impl && !has_drt {
                out.push(
                    Finding::new(
                        FindingKind::Unbound,
                        format!(
                            "{} has a model and an implementation but nothing binding them — \
                             add a binding to .tracelean/drt.json",
                            label(&req.id, c)
                        ),
                        where_.0.clone(),
                        where_.1,
                    )
                    .about(&req.id, c),
                );
            }
            if has_impl && !has_test {
                out.push(
                    Finding::new(
                        FindingKind::Untested,
                        format!("{} is implemented but has no `@tests` link", label(&req.id, c)),
                        where_.0,
                        where_.1,
                    )
                    .about(&req.id, c),
                );
            }
        }
    }
}

/// A proof whose strength nobody has asked about.
///
/// The question is only worth asking where there is something to ask it of: a
/// clause with `@proves` links. Asking it of an unproved clause would bury the
/// real finding (there is no proof) under a second one about the proof there
/// isn't.
fn check_strength(index: &TraceIndex, out: &mut Vec<Finding>) {
    for req in index.requirements.values() {
        for clause in req.clause_keys() {
            let c = clause.as_deref();
            let links = index.links_for_clause(&req.id, c);
            if links.iter().any(|l| l.is_exempt()) {
                continue;
            }
            if !links.iter().any(|l| l.role == Role::Proves) {
                continue;
            }
            if !matches!(
                super::strength::declared(index, &req.id, c),
                super::strength::Strength::Open
            ) {
                continue;
            }
            out.push(
                Finding::new(
                    FindingKind::UnpinnedProof,
                    format!(
                        "{} is proved, but nothing says the proved properties determine the \
                         model — a theorem can hold of many different functions",
                        label(&req.id, c)
                    ),
                    req.file.clone(),
                    0,
                )
                .about(&req.id, c),
            );
        }
    }
}

/// Multiple implementers are legal and silent; two that both claim exclusivity
/// are not.
fn check_contested(index: &TraceIndex, out: &mut Vec<Finding>) {
    let mut seen: BTreeMap<(String, Option<String>), Vec<&Link>> = BTreeMap::new();
    for link in &index.links {
        if link.role == Role::Implements && link.is_exclusive() {
            seen.entry((link.req_id.clone(), link.clause.clone()))
                .or_default()
                .push(link);
        }
    }
    for ((req_id, clause), links) in seen {
        if links.len() > 1 {
            for link in &links {
                out.push(
                    Finding::new(
                        FindingKind::Contested,
                        format!(
                            "{} is exclusively claimed by {} anchors",
                            label(&req_id, clause.as_deref()),
                            links.len()
                        ),
                        link.anchor.file.clone(),
                        link.line,
                    )
                    .about(&req_id, clause.as_deref()),
                );
            }
        }
    }
}

/// Staleness and backend verdicts, derived from evidence records rather than
/// recomputed — the judge and the differential tester write them, the checker
/// only reads.
fn check_evidence(index: &TraceIndex, out: &mut Vec<Finding>) {
    for record in &index.evidence {
        let req_id = &record.key.req_id;
        let clause = record.key.clause.as_deref();
        let file = index
            .requirements
            .get(req_id)
            .map(|r| r.file.clone())
            .unwrap_or_default();

        let current = index.current_hashes(req_id, clause);
        let stale = record.stale_inputs(&current);
        if !stale.is_empty() {
            out.push(
                Finding::new(
                    FindingKind::Stale,
                    format!(
                        "{} evidence for {} is stale: {} changed since it was recorded",
                        record.key.bond.as_str(),
                        label(req_id, clause),
                        stale.join(", ")
                    ),
                    file.clone(),
                    0,
                )
                .about(req_id, clause),
            );
            continue;
        }

        match &record.detail {
            super::EvidenceDetail::Judge { verdict, witness_confirmed, degraded, .. }
                if verdict != "agrees" && *witness_confirmed == Some(true) && !*degraded =>
            {
                out.push(
                    Finding::new(
                        FindingKind::JudgeDrift,
                        format!(
                            "{}: model and requirement disagree ({verdict}), witness confirmed",
                            label(req_id, clause)
                        ),
                        file,
                        0,
                    )
                    .about(req_id, clause),
                );
            }
            super::EvidenceDetail::Drt { divergences, .. } if *divergences > 0 => {
                out.push(
                    Finding::new(
                        FindingKind::Divergence,
                        format!(
                            "{}: {divergences} divergence(s) between model and implementation",
                            label(req_id, clause)
                        ),
                        file,
                        0,
                    )
                    .about(req_id, clause),
                );
            }
            _ => {}
        }
    }
}

fn check_policy_rules(index: &TraceIndex, policy: &Policy, out: &mut Vec<Finding>) {
    for rule in &policy.require {
        let Some(bond) = parse_bond(&rule.bond) else { continue };
        let Some(min) = parse_level(&rule.min_level) else { continue };
        for req in index.requirements.values() {
            if !glob_match(&rule.r#match, &req.id) {
                continue;
            }
            for clause in req.clause_keys() {
                let assurance = index.assurance(&req.id, clause.as_deref());
                let actual = match bond {
                    Bond::RequirementModel => assurance.requirement_model,
                    Bond::ModelImpl => assurance.model_impl,
                    Bond::ModelProof => assurance.model_proof,
                };
                if actual.map(|l| l < min).unwrap_or(true) {
                    out.push(
                        Finding::new(
                            FindingKind::PolicyUnmet,
                            // The subject is the clause, not the glob: a rule
                            // matching a three-clause requirement used to emit
                            // three findings with byte-identical text, which
                            // reads as the checker stuttering rather than as
                            // three separate gaps.
                            format!(
                                "policy ({}) requires {}{} at {} on {}, found {}",
                                rule.r#match,
                                req.id,
                                clause.as_deref().map(|c| format!(".{c}")).unwrap_or_default(),
                                rule.min_level,
                                rule.bond,
                                actual.map(|l| l.as_str()).unwrap_or("no evidence")
                            ),
                            req.file.clone(),
                            0,
                        )
                        .about(&req.id, clause.as_deref()),
                    );
                }
            }
        }
    }
}

fn parse_bond(s: &str) -> Option<Bond> {
    Some(match s {
        "requirement-model" | "RequirementModel" => Bond::RequirementModel,
        "model-impl" | "ModelImpl" => Bond::ModelImpl,
        "model-proof" | "ModelProof" => Bond::ModelProof,
        _ => return None,
    })
}

fn parse_level(s: &str) -> Option<super::Level> {
    use super::Level::*;
    Some(match s {
        "L1" => L1,
        "L2" => L2,
        "L3" => L3,
        "L4" => L4,
        _ => return None,
    })
}

impl Policy {
    /// Whether a requirement warrants the extra backtranslation calls.
    pub fn is_high_value(&self, req_id: &str) -> bool {
        self.high_value.iter().any(|p| glob_match(p, req_id))
    }
}

/// Trailing-`*` glob, which is all the policy syntax promises.
fn glob_match(pattern: &str, value: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    }
}

fn label(req_id: &str, clause: Option<&str>) -> String {
    match clause {
        Some(c) => format!("{req_id}.{c}"),
        None => req_id.to_string(),
    }
}

fn now_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}
