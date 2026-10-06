//! Roll-up: assurance and coverage aggregated over the refinement graph.
//!
//! Where a wrong number does the most damage, because a percentage is the one
//! output people quote without reading what produced it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::evidence::Level;

use super::checker;
use super::index::Index;
use super::requirement::Decomposition;

/// A figure, and whether it may be presented as exact.
///
/// A roll-up over an unclaimed decomposition is a *lower bound* on the truth.
/// Rendering it as exact converts an unknown denominator into a confident
/// number, which is the failure this type exists to make impossible: a caller
/// cannot read the value without also reading whether it is exact.
///
/// @implements REQ-ROLLUP.open_is_lower_bound
/// @implements ARCH-HONEST.lower_bound_marked
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    /// Clauses meeting the floor, and clauses counted. Kept as a fraction
    /// rather than a number: a percentage is a rendering, and two languages
    /// rounding one differently would be a divergence about nothing.
    pub met: u64,
    pub total: u64,
    pub exact: bool,
}

impl Figure {
    /// How this should be written down. A lower bound is never rendered bare.
    pub fn render(&self) -> String {
        let percent = if self.total == 0 {
            0
        } else {
            // Rounded half away from zero, in integer arithmetic, so the
            // rendering does not depend on a floating-point mode.
            (self.met * 200 + self.total) / (self.total * 2)
        };
        if self.exact {
            format!("{percent}%")
        } else {
            format!("≥ {percent}%")
        }
    }

    /// A requirement whose decomposition is unclaimed can never read as done.
    ///
    /// @implements REQ-ROLLUP.never_complete_when_open
    pub fn is_complete(&self) -> bool {
        self.exact && self.total > 0 && self.met == self.total
    }
}

/// Assurance of one node, and of everything under it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollUp {
    pub id: String,
    pub assurance: Level,
    pub covered: Figure,
    pub children: Vec<RollUp>,
}

/// Aggregate over children by taking the minimum.
///
/// An average invents a level nothing established; a minimum is always a level
/// something did.
///
/// @implements REQ-ROLLUP.min_not_mean
/// @implements ARCH-HONEST.weakest_link
/// @drt REQ-ROLLUP.min_not_mean
/// @drt ARCH-HONEST.weakest_link
pub fn combine(levels: Vec<Level>) -> Level {
    levels.into_iter().min().unwrap_or(Level::L1)
}

/// Coverage over files: how many of the files scanned are claimed by at least
/// one annotation.
///
/// A file nothing claims is in the denominator. The alternative — counting only
/// annotated files — is the figure that rises when somebody deletes an
/// annotation, and a coverage number that improves when you do less work is
/// worse than no number at all.
///
/// Exact, because the denominator is not a claim anybody made: it is the set of
/// files that were scanned, which is known.
///
/// @implements REQ-ROLLUP.untraced_counted
/// @implements ARCH-HONEST.untraced_visible
/// @drt REQ-ROLLUP.untraced_counted
pub fn file_coverage(scanned: Vec<String>, claimed: Vec<String>) -> Figure {
    let scanned: BTreeSet<String> = scanned.into_iter().collect();
    // A claim about a file nothing scanned is not coverage of anything, so the
    // numerator is an intersection rather than a count of claims.
    let claimed: BTreeSet<String> = claimed.into_iter().filter(|f| scanned.contains(f)).collect();
    Figure { met: claimed.len() as u64, total: scanned.len() as u64, exact: true }
}

/// A requirement reduced to what a roll-up depends on.
///
/// The roll-up is a question about a declared graph: which clauses exist, which
/// were exempted, whether the decomposition was claimed complete, and what
/// refines what. Nothing else a requirement carries changes the answer.
///
/// @implements ARCH-CORE-SHELL.decision_total
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    /// The author claimed the clauses exhaust the requirement.
    pub complete: bool,
    /// Clause keys, or one implicit clause when the document declares none.
    pub clauses: Vec<Option<String>>,
    /// Clauses an exemption removed from the denominator.
    pub exempt: Vec<Option<String>>,
    pub refines: Vec<String>,
}

/// Roll a declared graph up, over nothing but the graph.
///
/// A requirement reached more than once is visited once, so a diamond does not
/// double-count and a cycle cannot loop.
///
/// @implements REQ-ROLLUP.min_not_mean
/// @implements REQ-ROLLUP.open_is_lower_bound
/// @implements REQ-ROLLUP.never_complete_when_open
/// @implements REQ-ROLLUP.exempt_leaves_denominator
/// @implements REQ-ROLLUP.deterministic_order
/// @implements ARCH-HONEST.lower_bound_marked
/// @drt REQ-ROLLUP.open_is_lower_bound
/// @drt ARCH-HONEST.lower_bound_marked
/// @drt REQ-ROLLUP.never_complete_when_open
/// @drt REQ-ROLLUP.exempt_leaves_denominator
/// @drt REQ-ROLLUP.deterministic_order
pub fn roll_up(
    nodes: Vec<Node>,
    levels: Vec<((String, Option<String>), Level)>,
    root: String,
    floor: Level,
) -> RollUp {
    // Through maps, so a repeated identifier resolves the way the index
    // resolves it rather than the way a list happens to be ordered.
    let graph: BTreeMap<String, Node> =
        nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
    let levels: BTreeMap<(String, Option<String>), Level> = levels.into_iter().collect();

    fn figure_for(node: Option<&Node>, levels: &BTreeMap<(String, Option<String>), Level>, floor: Level) -> Figure {
        let Some(node) = node else { return Figure { met: 0, total: 0, exact: false } };
        let counted: Vec<&Option<String>> =
            node.clauses.iter().filter(|c| !node.exempt.contains(c)).collect();
        if counted.is_empty() {
            return Figure { met: 0, total: 0, exact: node.complete };
        }
        let met = counted
            .iter()
            .filter(|c| {
                levels.get(&(node.id.clone(), (**c).clone())).is_some_and(|l| *l >= floor)
            })
            .count();
        Figure { met: met as u64, total: counted.len() as u64, exact: node.complete }
    }

    fn walk(
        graph: &BTreeMap<String, Node>,
        id: &str,
        levels: &BTreeMap<(String, Option<String>), Level>,
        floor: Level,
        seen: &mut BTreeSet<String>,
    ) -> RollUp {
        let node = graph.get(id);
        let own: Vec<Level> = node
            .map(|n| {
                n.clauses
                    .iter()
                    .map(|c| levels.get(&(id.to_string(), c.clone())).copied().unwrap_or(Level::L1))
                    .collect()
            })
            .unwrap_or_default();

        let mut kids = Vec::new();
        for (child, n) in graph {
            if n.refines.iter().any(|p| p == id) && seen.insert(child.clone()) {
                kids.push(walk(graph, child, levels, floor, seen));
            }
        }

        let mut levels_here = own;
        levels_here.extend(kids.iter().map(|k| k.assurance));

        RollUp {
            id: id.to_string(),
            assurance: combine(levels_here),
            covered: figure_for(node, levels, floor),
            children: kids,
        }
    }

    let mut seen = BTreeSet::from([root.clone()]);
    walk(&graph, &root, &levels, floor, &mut seen)
}

/// Fraction of a requirement's clauses that reach at least `floor`.
///
/// Exact only when the author claimed the clauses exhaust the requirement.
///
/// @implements REQ-ROLLUP.deterministic_order
pub fn covered(index: &Index, id: &str, levels: &BTreeMap<(String, Option<String>), Level>, floor: Level) -> Figure {
    let exact = index
        .requirements
        .get(id)
        .map(|r| r.decomposition == Decomposition::Complete)
        .unwrap_or(false);
    let total = checker::denominator(index, id);
    if total == 0 {
        return Figure { met: 0, total: 0, exact };
    }
    let Some(req) = index.requirements.get(id) else {
        return Figure { met: 0, total: 0, exact };
    };
    let met = req
        .clause_keys()
        .into_iter()
        .filter(|clause| {
            levels
                .get(&(id.to_string(), clause.clone()))
                .map(|l| *l >= floor)
                .unwrap_or(false)
        })
        .count();
    Figure { met: met as u64, total: total as u64, exact }
}

/// Children of a requirement in the refinement graph, sorted.
///
/// @implements REQ-ROLLUP.deterministic_order
pub fn children(index: &Index, id: &str) -> Vec<String> {
    let mut out: Vec<String> = index
        .requirements
        .iter()
        .filter(|(_, r)| r.refines.iter().any(|p| p == id))
        .map(|(child, _)| child.clone())
        .collect();
    out.sort();
    out
}

/// Roll a requirement and its refinements up into a tree.
///
/// A requirement reached more than once through the graph is visited once, so
/// a diamond does not double-count and a cycle cannot loop.
pub fn tree(
    index: &Index,
    id: &str,
    levels: &BTreeMap<(String, Option<String>), Level>,
    floor: Level,
) -> RollUp {
    fn walk(
        index: &Index,
        id: &str,
        levels: &BTreeMap<(String, Option<String>), Level>,
        floor: Level,
        seen: &mut BTreeSet<String>,
    ) -> RollUp {
        let own: Vec<Level> = index
            .requirements
            .get(id)
            .map(|r| {
                r.clause_keys()
                    .into_iter()
                    .map(|c| levels.get(&(id.to_string(), c)).copied().unwrap_or(Level::L1))
                    .collect()
            })
            .unwrap_or_default();

        let mut kids = Vec::new();
        for child in children(index, id) {
            if seen.insert(child.clone()) {
                kids.push(walk(index, &child, levels, floor, seen));
            }
        }

        let mut levels_here = own;
        levels_here.extend(kids.iter().map(|k| k.assurance));

        RollUp {
            id: id.to_string(),
            assurance: combine(levels_here),
            covered: covered(index, id, levels, floor),
            children: kids,
        }
    }

    let mut seen = BTreeSet::from([id.to_string()]);
    walk(index, id, levels, floor, &mut seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-ROLLUP.min_not_mean
    #[test]
    fn aggregation_is_the_minimum() {
        assert_eq!(combine(vec![Level::L4, Level::L1]), Level::L1);
        assert_eq!(combine(vec![Level::L3, Level::L3]), Level::L3);
        // Nothing to aggregate is the lowest, not the highest.
        assert_eq!(combine(vec![]), Level::L1);
    }

    /// A mean would report a level nothing established.
    #[test]
    fn the_result_is_always_a_level_something_established() {
        let inputs = vec![Level::L4, Level::L2];
        let result = combine(inputs.clone());
        assert!(inputs.contains(&result));
    }

    /// @tests ARCH-HONEST.lower_bound_marked
    #[test]
    fn an_inexact_figure_renders_as_a_lower_bound() {
        assert_eq!(Figure { met: 1, total: 2, exact: true }.render(), "50%");
        assert_eq!(Figure { met: 1, total: 2, exact: false }.render(), "≥ 50%");
    }

    /// @tests REQ-ROLLUP.never_complete_when_open
    #[test]
    fn an_inexact_figure_is_never_complete() {
        assert!(!Figure { met: 2, total: 2, exact: false }.is_complete());
        assert!(Figure { met: 2, total: 2, exact: true }.is_complete());
    }
}
