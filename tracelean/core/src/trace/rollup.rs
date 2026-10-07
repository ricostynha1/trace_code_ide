//! Aggregating coverage and assurance up the refinement DAG.
//!
//! Two rules keep these numbers honest, and both are the opposite of what a
//! dashboard usually does:
//!
//! * **Assurance aggregates by minimum, never by mean.** A parent is worth its
//!   weakest leaf. An average would report "L2.7" and bury the one unchecked
//!   clause that matters.
//! * **A percentage is only exact when the denominator is claimed complete.**
//!   If any requirement on the reachable subgraph says `decomposition: open`,
//!   more children may exist, so the number is a lower bound and must render
//!   as "≥ x%" — and such a parent can never display as finished.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::{Decomposition, FindingKind, Level, Role, Severity, TraceIndex};

/// Aggregate state of a requirement and everything it refines down to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollUp {
    pub req_id: String,
    /// Fraction of counted leaf clauses carrying evidence.
    pub coverage: f32,
    /// True when some node in the reachable subgraph is `open`, so `coverage`
    /// is only a lower bound.
    pub coverage_is_lower_bound: bool,
    /// The weakest leaf clause — what the whole subtree is actually worth.
    pub assurance: Level,
    pub leaf_clauses: usize,
    pub exempt_clauses: usize,
    pub stale_clauses: usize,
    pub error_count: usize,
    pub warn_count: usize,
    /// Requirements reachable from here, including itself.
    pub reachable: Vec<String>,
}

/// One node of the DAG rendered as a tree for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub req_id: String,
    pub title: String,
    pub depth: usize,
    pub rollup: RollUp,
    pub decomposition: Decomposition,
    pub children: Vec<TreeNode>,
    /// True when this requirement already appeared elsewhere in the tree
    /// (the DAG allows two parents). Its subtree is not repeated; the UI dims
    /// it rather than pretending there are two requirements.
    pub duplicate: bool,
}

impl TraceIndex {
    /// Requirements nobody refines — the roots of the display tree.
    pub fn roots(&self) -> Vec<String> {
        self.requirements
            .values()
            .filter(|r| r.refines.is_empty())
            .map(|r| r.id.clone())
            .collect()
    }

    /// Everything reachable from `req_id` through `refines` edges, inclusive.
    /// Terminates on cycles (which the checker reports separately).
    pub fn reachable(&self, req_id: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![req_id.to_string()];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            for child in self.children_of(&id) {
                stack.push(child);
            }
        }
        seen.into_iter().collect()
    }

    /// Aggregate over every clause reachable from this requirement.
    ///
    /// Computed over the *set* of reachable leaf clauses rather than by
    /// averaging children, because a diamond in the DAG would otherwise count
    /// the shared leaf twice.
    pub fn rollup(&self, req_id: &str) -> RollUp {
        let reachable = self.reachable(req_id);

        let mut counted = 0usize;
        let mut earned = 0.0f32;
        let mut exempt_clauses = 0usize;
        let mut stale_clauses = 0usize;
        let mut assurance = Level::L4;
        let mut any_counted = false;
        let mut lower_bound = false;

        for id in &reachable {
            let Some(req) = self.requirements.get(id) else { continue };
            if req.decomposition == Decomposition::Open {
                lower_bound = true;
            }

            for clause in req.clause_keys() {
                let c = clause.as_deref();
                let links = self.links_for_clause(id, c);

                if links.iter().any(|l| l.is_exempt()) {
                    exempt_clauses += 1;
                    continue; // exemptions leave the denominator
                }

                counted += 1;
                any_counted = true;

                let a = self.assurance(id, c);
                if !a.stale.is_empty() {
                    stale_clauses += 1;
                }

                let has_evidence = a.requirement_model.is_some()
                    || a.model_impl.is_some()
                    || a.model_proof.is_some();
                let claimed = links.iter().any(|l| l.role == Role::Implements);
                let partial = links.iter().any(|l| l.is_partial());

                let value: f32 = if has_evidence {
                    1.0
                } else if claimed {
                    0.5
                } else {
                    0.0
                };
                earned += if partial { value.min(0.5) } else { value };

                assurance = assurance.min(a.weakest());
            }
        }

        let (error_count, warn_count) = self.finding_counts(&reachable);

        RollUp {
            req_id: req_id.to_string(),
            coverage: if counted == 0 { 1.0 } else { earned / counted as f32 },
            coverage_is_lower_bound: lower_bound,
            assurance: if any_counted { assurance } else { Level::L1 },
            leaf_clauses: counted,
            exempt_clauses,
            stale_clauses,
            error_count,
            warn_count,
            reachable,
        }
    }

    fn finding_counts(&self, ids: &[String]) -> (usize, usize) {
        let set: BTreeSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let relevant = self
            .findings
            .iter()
            .filter(|f| f.req_id.as_deref().map(|r| set.contains(r)).unwrap_or(false));
        let mut errors = 0;
        let mut warns = 0;
        for f in relevant {
            match f.severity {
                Severity::Error => errors += 1,
                Severity::Warn => warns += 1,
                Severity::Info => {}
            }
        }
        (errors, warns)
    }

    /// The DAG rendered as a tree, to an optional maximum depth.
    ///
    /// Collapsing never improves the picture: a node's roll-up already covers
    /// everything beneath it, so a truncated tree shows the same errors, stale
    /// counts and assurance as the expanded one.
    pub fn tree(&self, max_depth: Option<usize>) -> Vec<TreeNode> {
        let mut emitted = BTreeSet::new();
        let mut roots: Vec<String> = self.roots();

        // A cycle can leave every node with a parent; fall back to showing all
        // requirements rather than an empty tree.
        if roots.is_empty() {
            roots = self.requirements.keys().cloned().collect();
        }
        roots.sort();

        roots
            .iter()
            .map(|id| self.tree_node(id, 0, max_depth, &mut emitted))
            .collect()
    }

    fn tree_node(
        &self,
        req_id: &str,
        depth: usize,
        max_depth: Option<usize>,
        emitted: &mut BTreeSet<String>,
    ) -> TreeNode {
        let duplicate = !emitted.insert(req_id.to_string());
        let req = self.requirements.get(req_id);

        let children = if duplicate || max_depth.map(|m| depth >= m).unwrap_or(false) {
            Vec::new()
        } else {
            let mut kids = self.children_of(req_id);
            kids.sort();
            kids.iter()
                .map(|c| self.tree_node(c, depth + 1, max_depth, emitted))
                .collect()
        };

        TreeNode {
            req_id: req_id.to_string(),
            title: req.map(|r| r.title.clone()).unwrap_or_default(),
            depth,
            rollup: self.rollup(req_id),
            decomposition: req.map(|r| r.decomposition).unwrap_or_default(),
            children,
            duplicate,
        }
    }
}

/// Findings whose kind blocks a CI run under the given policy.
pub fn blocking<'a>(index: &'a TraceIndex, policy: &'a super::Policy) -> Vec<&'a super::Finding> {
    index
        .findings
        .iter()
        .filter(|f| policy.blocks(f.kind) || f.kind == FindingKind::PolicyUnmet)
        .collect()
}
