//! Findings: what is claimed, what is missing, and what has gone stale.
//!
//! Naming is the feature. "Traceability error" is not actionable in a diff;
//! *this clause has a model and an implementation and nothing binds them* is a
//! task. Most findings are progress rather than faults, and a tool that reports
//! progress as failure gets its checks disabled in week one.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::annotation::{Qualifier, Role};
use super::index::Index;
use super::requirement::{self, Decomposition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

/// @implements REQ-CHECK.named_kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// An annotation names a requirement or clause that does not exist.
    Dangling,
    /// `refines:` names a requirement that does not exist.
    DanglingRefines,
    /// The refinement graph contains a cycle.
    RefinesCycle,
    /// Two documents declare the same identifier.
    DuplicateId,
    /// A clause has no model and is not exempt.
    Unmodeled,
    /// Modelled, but nothing claims to implement it.
    Unimplemented,
    /// Model and implementation both exist, and nothing binds them.
    Unbound,
    /// Implemented, but no test.
    Untested,
    /// Two links exclusively claim the same clause.
    Contested,
    /// An exemption without a reason or an approver.
    UnsoundExemption,
    /// A partial or nondeterministic qualifier without a reason.
    UnsoundQualifier,
    /// Something wrong in a document or an annotation.
    Malformed,
    /// A source file did not parse cleanly, so its annotations anchor to the
    /// whole file and are capped at the lowest evidence level.
    ///
    /// Not an error: the claims are still recorded, just not precisely placed.
    /// Blocking on it would make an unsupported language construct fail a build
    /// that is otherwise fine.
    Imprecise,
    /// A clause with more than one `@models` (ADR-0014).
    SeveralModels,
    /// A clause with more than one `@specifies`.
    SeveralSpecs,
    /// A clause with more than one `@pins`.
    SeveralPins,
}

impl Kind {
    /// Whether this names something broken, or work not yet done.
    ///
    /// @implements REQ-CHECK.progress_not_fault
    pub fn is_progress(self) -> bool {
        matches!(self, Kind::Unmodeled | Kind::Unimplemented | Kind::Untested)
    }

    pub fn severity(self) -> Severity {
        match self {
            Kind::Unmodeled | Kind::Unimplemented | Kind::Untested => Severity::Info,
            // The several-of-a-role kinds warn while the clauses carrying them
            // are sorted out (action plan §7); they become errors after.
            Kind::Unbound
            | Kind::UnsoundQualifier
            | Kind::Imprecise
            | Kind::SeveralModels
            | Kind::SeveralSpecs
            | Kind::SeveralPins => Severity::Warn,
            Kind::Dangling
            | Kind::DanglingRefines
            | Kind::RefinesCycle
            | Kind::DuplicateId
            | Kind::Contested
            | Kind::UnsoundExemption
            | Kind::Malformed => Severity::Error,
        }
    }
}

/// Which kinds fail a build. Deliberately small: a requirement with no model
/// yet is where the work is, not a broken build.
///
/// @implements REQ-CHECK.severity_policy
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub block_on: BTreeSet<Kind>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            block_on: [
                Kind::Dangling,
                Kind::DanglingRefines,
                Kind::RefinesCycle,
                Kind::DuplicateId,
                Kind::Contested,
                Kind::UnsoundExemption,
                Kind::Malformed,
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub req_id: Option<String>,
    pub clause: Option<String>,
    /// Carried rather than recomputed, so a report and a build gate can never
    /// answer slightly different questions about what an error is.
    pub blocking: bool,
}

impl Finding {
    fn new(kind: Kind, message: String, file: &str, line: u32, policy: &Policy) -> Finding {
        Finding {
            kind,
            severity: kind.severity(),
            message,
            file: file.to_string(),
            line,
            req_id: None,
            clause: None,
            blocking: policy.block_on.contains(&kind),
        }
    }

    fn about(mut self, req_id: &str, clause: Option<&str>) -> Finding {
        self.req_id = Some(req_id.to_string());
        self.clause = clause.map(str::to_string);
        self
    }
}

/// What is known about a kind, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindFacts {
    pub severity: Severity,
    /// Work not yet done, rather than something broken.
    pub progress: bool,
    /// Whether the default policy fails a build on it.
    pub blocks_by_default: bool,
}

/// Severity, progress and the default gate for one kind.
///
/// Together in one function because the requirement is that they agree: a kind
/// that describes progress must not be in the default blocking set, and a
/// reader must not have to check two places to find out whether something is a
/// fault.
///
/// @implements REQ-CHECK.named_kinds
/// @implements REQ-CHECK.progress_not_fault
/// @implements REQ-CHECK.severity_policy
/// @drt REQ-CHECK.named_kinds
/// @drt REQ-CHECK.progress_not_fault
/// @drt REQ-CHECK.severity_policy
pub fn facts(kind: Kind) -> KindFacts {
    KindFacts {
        severity: kind.severity(),
        progress: kind.is_progress(),
        blocks_by_default: Policy::default().block_on.contains(&kind),
    }
}

/// What a clause's claimed roles imply, in the order the findings are pushed.
///
/// This is the coverage decision on its own, over nothing but the roles: a
/// clause is unmodelled, or modelled and unimplemented, or both and unbound —
/// exactly one of the three, which is what `exactly_once` demands — and,
/// separately, implemented without a test.
///
/// An exempt clause yields nothing at all. That is the point of an exemption,
/// and why one has to carry a reason and an approver.
///
/// @implements REQ-CHECK.exactly_once
/// @implements REQ-CHECK.unbound_reported
/// @implements REQ-CHECK.structural_is_not_exempt
/// @drt REQ-CHECK.exactly_once
/// @drt REQ-CHECK.structural_is_not_exempt
/// @drt REQ-CHECK.unbound_reported
pub fn coverage_kinds(roles: Vec<Role>, exempt: bool, structural: bool) -> Vec<Kind> {
    if exempt {
        return Vec::new();
    }
    let have: BTreeSet<Role> = roles.into_iter().collect();
    let modeled = have.contains(&Role::Models);
    let implemented = have.contains(&Role::Implements);
    let bound = have.contains(&Role::Drt);
    let tested = have.contains(&Role::Tests);

    let mut out = Vec::new();
    // A structural clause is a property of the tree, so there is no
    // data-to-data function to model, no second implementation to compare
    // against, and nothing to point an `@implements` at — the tree realises it
    // by being the shape it is. What there is, and what is required, is a test
    // that reads the repository and says what it found. The clause stays in the
    // denominator, which an exemption would not.
    if structural {
        if !tested {
            out.push(Kind::Untested);
        }
        return out;
    }
    if !modeled {
        out.push(Kind::Unmodeled);
    } else if !implemented {
        out.push(Kind::Unimplemented);
    } else if !bound {
        out.push(Kind::Unbound);
    }
    if implemented && !tested {
        out.push(Kind::Untested);
    }
    out
}

/// What a qualifier on a link implies about the link itself.
///
/// An exemption removes a clause from the denominator, so an exemption without
/// a reason and an approver is a hole nobody signed for. A partial or
/// nondeterministic qualifier that says nothing is the same failure, one
/// severity down.
///
/// @implements REQ-CHECK.qualifier_soundness
/// @drt REQ-CHECK.qualifier_soundness
pub fn qualifier_kinds(qualifier: Option<Qualifier>) -> Vec<Kind> {
    match qualifier {
        Some(Qualifier::Exempt { reason, judged_by, .. })
            if reason.is_none() || judged_by.is_none() =>
        {
            vec![Kind::UnsoundExemption]
        }
        Some(Qualifier::Partial { reason: None })
        | Some(Qualifier::Nondeterministic { reason: None })
        // A structural clause that does not say why there is no law to state is
        // indistinguishable from one nobody got round to modelling.
        | Some(Qualifier::Structural { reason: None }) => vec![Kind::UnsoundQualifier],
        _ => Vec::new(),
    }
}

/// Which roles a clause carries more than one of, as the kinds that report it.
///
/// A clause has one model (the function computing what it talks about), at
/// most one specification and at most one pinning theorem (ADR-0014). A second
/// of any of them leaves which one is meant to be guessed — by position, as
/// pinning once did — so it is named instead.
///
/// @implements REQ-CHECK.one_of_each_role
/// @drt REQ-CHECK.one_of_each_role
pub fn crowded_kinds(roles: Vec<Role>) -> Vec<Kind> {
    let count = |role: Role| roles.iter().filter(|r| **r == role).count();
    [(Role::Models, Kind::SeveralModels), (Role::Specifies, Kind::SeveralSpecs), (Role::Pins, Kind::SeveralPins)]
        .into_iter()
        .filter(|(role, _)| count(*role) > 1)
        .map(|(_, kind)| kind)
        .collect()
}

/// Check an index.
///
/// @implements REQ-CHECK.exactly_once
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn check(index: &Index, policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();

    for problem in &index.problems {
        let kind = if problem.imprecise { Kind::Imprecise } else { Kind::Malformed };
        findings.push(Finding::new(kind, problem.message.clone(), &problem.file, problem.line, policy));
    }

    for (id, parent) in requirement::dangling_refines(&index.requirements) {
        let file = index.requirements.get(&id).map(|r| r.file.clone()).unwrap_or_default();
        findings.push(
            Finding::new(
                Kind::DanglingRefines,
                format!("`{id}` refines `{parent}`, which does not exist"),
                &file,
                1,
                policy,
            )
            .about(&id, None),
        );
    }

    if let Some(cycle) = requirement::refinement_cycle(&index.requirements) {
        let file = index
            .requirements
            .get(&cycle[0])
            .map(|r| r.file.clone())
            .unwrap_or_default();
        findings.push(Finding::new(
            Kind::RefinesCycle,
            format!("refinement cycle: {}", cycle.join(" -> ")),
            &file,
            1,
            policy,
        ));
    }

    // Links whose requirement or clause does not exist.
    for link in &index.links {
        let known = match index.requirements.get(&link.req_id) {
            None => false,
            Some(req) => match &link.clause {
                None => true,
                Some(clause) => req.has_clause(clause),
            },
        };
        if !known {
            let what = match &link.clause {
                Some(c) => format!("{}.{c}", link.req_id),
                None => link.req_id.clone(),
            };
            findings.push(
                Finding::new(
                    Kind::Dangling,
                    format!("`@{} {what}` names nothing that exists", link.role.as_str()),
                    &link.anchor.file,
                    link.line,
                    policy,
                )
                .about(&link.req_id, link.clause.as_deref()),
            );
        }

        for kind in qualifier_kinds(link.qualifier.clone()) {
            let message = match kind {
                Kind::UnsoundExemption => "an exemption must carry a reason and an approver",
                _ => "a qualifier must say why",
            };
            findings.push(
                Finding::new(kind, message.into(), &link.anchor.file, link.line, policy)
                    .about(&link.req_id, link.clause.as_deref()),
            );
        }
    }

    // Per clause: what roles claim it.
    let mut roles: BTreeMap<(String, Option<String>), BTreeSet<Role>> = BTreeMap::new();
    let mut exempt: BTreeSet<(String, Option<String>)> = BTreeSet::new();
    let mut structural: BTreeSet<(String, Option<String>)> = BTreeSet::new();
    for link in &index.links {
        let key = (link.req_id.clone(), link.clause.clone());
        roles.entry(key.clone()).or_default().insert(link.role);
        if link.is_exempt() {
            exempt.insert(key.clone());
        }
        if link.is_structural() {
            structural.insert(key);
        }
    }

    // Per clause: each declaration claiming it as its model, specification or
    // pin, once per declaration, with where it is.
    let mut declared: BTreeMap<(String, Option<String>), BTreeMap<(Role, String), (String, u32)>> = BTreeMap::new();
    for link in index.links.iter().filter(|l| matches!(l.role, Role::Models | Role::Specifies | Role::Pins)) {
        declared
            .entry((link.req_id.clone(), link.clause.clone()))
            .or_default()
            .entry((link.role, link.anchor.ident()))
            .or_insert((link.anchor.file.clone(), link.line));
    }
    for ((id, clause), seen) in &declared {
        let what = match clause {
            Some(c) => format!("{id}.{c}"),
            None => id.clone(),
        };
        for kind in crowded_kinds(seen.keys().map(|(role, _)| *role).collect()) {
            let role = match kind {
                Kind::SeveralModels => Role::Models,
                Kind::SeveralSpecs => Role::Specifies,
                _ => Role::Pins,
            };
            let mut places: Vec<&(String, u32)> =
                seen.iter().filter(|((r, _), _)| *r == role).map(|(_, at)| at).collect();
            places.sort();
            let message = format!(
                "{what} has {} @{} declarations, and may have one: {}",
                places.len(),
                role.as_str(),
                places.iter().map(|(file, line)| format!("{file}:{line}")).collect::<Vec<_>>().join(", ")
            );
            let (file, line) = places[0];
            findings.push(Finding::new(kind, message, file, *line, policy).about(id, clause.as_deref()));
        }
    }

    for (id, req) in &index.requirements {
        for clause in req.clause_keys() {
            let key = (id.clone(), clause.clone());
            if exempt.contains(&key) {
                continue;
            }
            let have = roles.get(&key).cloned().unwrap_or_default();
            let what = match &clause {
                Some(c) => format!("{id}.{c}"),
                None => id.clone(),
            };

            let is_structural = structural.contains(&key);
            for kind in coverage_kinds(have.into_iter().collect(), false, is_structural) {
                let message = match kind {
                    Kind::Unmodeled => format!("{what} has no model"),
                    Kind::Unimplemented => format!("{what} is modelled but not implemented"),
                    // The most important finding: the formal work was done, the
                    // code was written, and nothing compares them.
                    Kind::Unbound => {
                        format!("{what} has a model and an implementation, and nothing binds them")
                    }
                    _ if is_structural => {
                        format!("{what} is checked structurally and nothing tests it")
                    }
                    _ => format!("{what} is implemented but not tested"),
                };
                findings.push(
                    Finding::new(kind, message, &req.file, 1, policy).about(id, clause.as_deref()),
                );
            }
        }
    }

    findings.sort_by(|a, b| {
        (b.severity, &a.file, a.line, a.kind).cmp(&(a.severity, &b.file, b.line, b.kind))
    });
    findings
}

/// Whether a set of findings fails the gate.
pub fn blocks(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.blocking)
}

/// Clauses a requirement is answerable for, excluding exempted ones.
///
/// @implements REQ-ROLLUP.exempt_leaves_denominator
pub fn denominator(index: &Index, id: &str) -> usize {
    let Some(req) = index.requirements.get(id) else { return 0 };
    let exempt: BTreeSet<Option<String>> = index
        .links
        .iter()
        .filter(|l| l.req_id == id && l.is_exempt())
        .map(|l| l.clause.clone())
        .collect();
    req.clause_keys().into_iter().filter(|c| !exempt.contains(c)).count()
}

/// Whether a requirement's figures may be presented as exact.
///
/// @implements REQ-ROLLUP.open_is_lower_bound
/// @implements REQ-ROLLUP.never_complete_when_open
pub fn is_exact(index: &Index, id: &str) -> bool {
    index
        .requirements
        .get(id)
        .map(|r| r.decomposition == Decomposition::Complete)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::index::{build, Index};
    use std::path::Path;

    fn tree(files: &[(&str, &str)]) -> (tempdir::Dir, Index) {
        let dir = tempdir::Dir::new();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let index = build(dir.path());
        (dir, index)
    }

    /// A minimal scratch directory, removed on drop.
    mod tempdir {
        use std::path::{Path, PathBuf};
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn new() -> Dir {
                use std::sync::atomic::{AtomicU64, Ordering};
                static COUNTER: AtomicU64 = AtomicU64::new(0);
                let unique = format!(
                    "tracelean-check-{}-{}",
                    std::process::id(),
                    COUNTER.fetch_add(1, Ordering::Relaxed)
                );
                let path = std::env::temp_dir().join(unique);
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).unwrap();
                Dir(path)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    const REQ: &str = "---\nid: REQ-A\ndecomposition: complete\nclauses:\n  one: First.\n---\nbody";

    /// @tests REQ-CHECK.unbound_reported
    #[test]
    fn a_model_and_an_implementation_with_nothing_between_them_is_reported() {
        let (_d, index) = tree(&[
            ("reqs/a.md", REQ),
            ("src/m.lean", "-- @models REQ-A.one\ndef f (n : Nat) : Nat := n\n"),
            ("src/i.rs", "// @implements REQ-A.one\npub fn f(n: u8) -> u8 { n }\n"),
        ]);
        let findings = check(&index, &Policy::default());
        let unbound: Vec<_> = findings.iter().filter(|f| f.kind == Kind::Unbound).collect();
        assert_eq!(unbound.len(), 1, "{findings:#?}");
        assert_eq!(unbound[0].clause.as_deref(), Some("one"));
    }

    /// Two models of one clause are named, each by where it is, and do not
    /// block while the clauses carrying them are sorted out.
    ///
    /// @tests REQ-CHECK.one_of_each_role
    #[test]
    fn a_second_model_spec_or_pin_is_named_with_every_declaration() {
        let (_d, index) = tree(&[
            ("reqs/a.md", REQ),
            (
                "src/m.lean",
                "/-- @models REQ-A.one -/\ndef f (n : Nat) : Nat := n\n\n/-- @models REQ-A.one -/\ndef g (n : Nat) : Nat := n\n\n\
                 /-- @specifies REQ-A.one -/\ndef P (n y : Nat) : Prop := y = n\n\n/-- @pins REQ-A.one -/\ntheorem t : True := trivial\n",
            ),
        ]);
        let findings = check(&index, &Policy::default());
        let several: Vec<_> = findings.iter().filter(|f| f.kind == Kind::SeveralModels).collect();
        assert_eq!(several.len(), 1, "{findings:#?}");
        assert!(several[0].message.contains("src/m.lean:1") && several[0].message.contains("src/m.lean:4"), "{}", several[0].message);
        assert_eq!(several[0].severity, Severity::Warn);
        assert!(!several[0].blocking);
        assert!(!findings.iter().any(|f| matches!(f.kind, Kind::SeveralSpecs | Kind::SeveralPins)));
        assert_eq!(crowded_kinds(vec![Role::Specifies, Role::Pins, Role::Pins]), vec![Kind::SeveralPins]);
        assert_eq!(crowded_kinds(vec![Role::Models, Role::Tests, Role::Tests]), vec![]);
    }

    /// @tests REQ-CHECK.progress_not_fault
    #[test]
    fn missing_work_is_information_and_does_not_block() {
        let (_d, index) = tree(&[("reqs/a.md", REQ)]);
        let findings = check(&index, &Policy::default());
        assert!(findings.iter().any(|f| f.kind == Kind::Unmodeled));
        assert!(!blocks(&findings), "{findings:#?}");
    }

    /// @tests REQ-CHECK.exactly_once
    #[test]
    fn one_condition_gives_one_finding() {
        let (_d, index) = tree(&[("reqs/a.md", REQ)]);
        let findings = check(&index, &Policy::default());
        let unmodeled: Vec<_> = findings.iter().filter(|f| f.kind == Kind::Unmodeled).collect();
        assert_eq!(unmodeled.len(), 1);
        // Unmodeled and Unimplemented are mutually exclusive: a clause with no
        // model is not also reported as modelled-but-unimplemented.
        assert!(!findings.iter().any(|f| f.kind == Kind::Unimplemented));
    }

    #[test]
    fn an_annotation_naming_nothing_blocks() {
        let (_d, index) = tree(&[
            ("reqs/a.md", REQ),
            ("src/i.rs", "// @implements REQ-A.nope\npub fn f() {}\n"),
        ]);
        let findings = check(&index, &Policy::default());
        assert!(findings.iter().any(|f| f.kind == Kind::Dangling));
        assert!(blocks(&findings));
    }

    /// @tests REQ-ROLLUP.exempt_leaves_denominator
    #[test]
    fn an_exemption_leaves_the_denominator_and_must_be_justified() {
        let (_d, index) = tree(&[
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n  two: Second.\n---\nb"),
            (
                "src/i.rs",
                "// @implements REQ-A.two\n// @exempt REQ-A.two reason=\"platform\" by=ana\npub fn f() {}\n",
            ),
        ]);
        assert_eq!(denominator(&index, "REQ-A"), 1);
        let findings = check(&index, &Policy::default());
        assert!(!findings.iter().any(|f| f.kind == Kind::UnsoundExemption));
    }

    #[test]
    fn an_exemption_without_an_approver_is_unsound() {
        let (_d, index) = tree(&[
            ("reqs/a.md", REQ),
            ("src/i.rs", "// @implements REQ-A.one\n// @exempt reason=\"later\"\npub fn f() {}\n"),
        ]);
        let findings = check(&index, &Policy::default());
        assert!(findings.iter().any(|f| f.kind == Kind::UnsoundExemption));
        assert!(blocks(&findings));
    }

    /// @tests REQ-ROLLUP.never_complete_when_open
    #[test]
    fn an_open_decomposition_is_never_exact() {
        let (_d, index) = tree(&[("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nb")]);
        assert!(!is_exact(&index, "REQ-A"));
        let (_d2, complete) = tree(&[("reqs/a.md", REQ)]);
        assert!(is_exact(&complete, "REQ-A"));
    }

    #[test]
    fn findings_are_ordered_worst_first() {
        let (_d, index) = tree(&[
            ("reqs/a.md", REQ),
            ("src/i.rs", "// @implements REQ-A.nope\npub fn f() {}\n"),
        ]);
        let findings = check(&index, &Policy::default());
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn the_index_of_an_empty_tree_is_empty() {
        let dir = tempdir::Dir::new();
        let index = build(Path::new(dir.path()));
        assert!(check(&index, &Policy::default()).is_empty());
    }
}
