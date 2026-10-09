//! Roll-up: assurance and coverage aggregated over the refinement graph.
//!
//! Where a wrong number does the most damage, because a percentage is the one
//! output people quote without reading what produced it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::evidence::Level;

use super::index::Index;
use super::requirement::Decomposition;

/// A figure, and whether it may be presented as exact.
///
/// A roll-up over an unclaimed decomposition is *provisional*: a clause not yet
/// written carries no evidence, so writing it can only lower the figure. A
/// caller cannot read the value without also reading whether it is exact.
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
    /// How this should be written down. A provisional figure is never rendered bare.
    pub fn render(&self) -> String {
        render_figure(*self)
    }

    /// A requirement whose decomposition is unclaimed can never read as done.
    ///
    /// @implements REQ-ROLLUP.never_complete_when_open
    pub fn is_complete(&self) -> bool {
        self.exact && self.total > 0 && self.met == self.total
    }
}

/// A figure as it is written down.
///
/// An open figure is marked provisional, with the direction it can move: a
/// clause nobody has written yet has no evidence, so it can only bring the
/// figure down. `≤` says that; `≥` would say the opposite of the truth.
///
/// @implements ARCH-HONEST.lower_bound_marked
/// @drt ARCH-HONEST.lower_bound_marked
pub fn render_figure(figure: Figure) -> String {
    let percent = if figure.total == 0 {
        0
    } else {
        // Rounded half away from zero, in integer arithmetic, so the
        // rendering does not depend on a floating-point mode.
        (figure.met * 200 + figure.total) / (figure.total * 2)
    };
    if figure.exact {
        format!("{percent}%")
    } else {
        format!("≤ {percent}% (provisional)")
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
/// worse than no number at all. Counted per file: unclaimed code inside a
/// claimed file is line coverage's question (`REQ-LINECOV`), not this one's.
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
/// were exempted or marked partial, whether the decomposition was claimed
/// complete, and what refines what. Nothing else a requirement carries changes
/// the answer.
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
    /// Clauses claimed only in part: counted, and capped.
    #[serde(default)]
    pub partial_clauses: Vec<Option<String>>,
    pub refines: Vec<String>,
}

/// The highest level a clause claimed only in part can contribute: below the
/// level at which anything ran, so half of a clause never counts as checked.
pub const PARTIAL_CAP: Level = Level::L2;

/// The requirements reachable from `root` by following "is refined by", root
/// included, each once.
///
/// A set, not a walk: a requirement two paths reach is in it once, and a cycle
/// ends when nothing new is found.
pub fn reach(graph: &BTreeMap<String, Node>, root: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::from([root.to_string()]);
    let mut frontier = vec![root.to_string()];
    while let Some(id) = frontier.pop() {
        for (child, node) in graph {
            if node.refines.iter().any(|p| *p == id) && found.insert(child.clone()) {
                frontier.push(child.clone());
            }
        }
    }
    found
}

/// What one requirement contributes on its own: the level of each clause that
/// counts (exempt clauses do not; partial ones are capped), and how many of
/// them reach the floor.
fn own(node: &Node, levels: &BTreeMap<(String, Option<String>), Level>, floor: Level) -> (Vec<Level>, u64) {
    let counted: Vec<Level> = node
        .clauses
        .iter()
        .filter(|c| !node.exempt.contains(c))
        .map(|c| {
            let level = levels.get(&(node.id.clone(), c.clone())).copied().unwrap_or(Level::L1);
            if node.partial_clauses.contains(c) {
                level.min(PARTIAL_CAP)
            } else {
                level
            }
        })
        .collect();
    let met = counted.iter().filter(|l| **l >= floor).count() as u64;
    (counted, met)
}

/// Roll a declared graph up, over nothing but the graph.
///
/// Each requirement's figures are computed over its own reachable set, each
/// member counted once: the minimum of every counted clause's level, and the
/// fraction of them at the floor. The figure is exact only when every member
/// claimed its decomposition complete. Children are shown under every parent
/// they refine, sorted; a child already on the path is left out, so a cycle
/// cannot loop.
///
/// @implements REQ-ROLLUP.open_is_lower_bound
/// @implements REQ-ROLLUP.exempt_leaves_denominator
/// @implements REQ-ROLLUP.partial_capped
/// @implements REQ-ROLLUP.deterministic_order
/// @implements REQ-ROLLUP.counted_once
/// @drt REQ-ROLLUP.open_is_lower_bound
/// @drt REQ-ROLLUP.never_complete_when_open
/// @drt REQ-ROLLUP.exempt_leaves_denominator
/// @drt REQ-ROLLUP.partial_capped
/// @drt REQ-ROLLUP.deterministic_order
/// @drt REQ-ROLLUP.counted_once
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

    fn node_of(
        graph: &BTreeMap<String, Node>,
        id: &str,
        levels: &BTreeMap<(String, Option<String>), Level>,
        floor: Level,
        path: &mut Vec<String>,
    ) -> RollUp {
        let members = reach(graph, id);
        let mut all = Vec::new();
        let (mut met, mut total, mut exact) = (0u64, 0u64, true);
        for member in &members {
            match graph.get(member) {
                Some(node) => {
                    let (counted, m) = own(node, levels, floor);
                    met += m;
                    total += counted.len() as u64;
                    exact &= node.complete;
                    all.extend(counted);
                }
                // A requirement nobody declared is not a claim anybody made.
                None => exact = false,
            }
        }

        path.push(id.to_string());
        let mut children = Vec::new();
        for (child, node) in graph {
            if node.refines.iter().any(|p| p == id) && !path.contains(child) {
                children.push(node_of(graph, child, levels, floor, path));
            }
        }
        path.pop();

        RollUp {
            id: id.to_string(),
            assurance: combine(all),
            covered: Figure { met, total, exact },
            children,
        }
    }

    node_of(&graph, &root, &levels, floor, &mut Vec::new())
}

/// The graph an index declares, as `roll_up` reads it.
pub fn nodes(index: &Index) -> Vec<Node> {
    index
        .requirements
        .iter()
        .map(|(id, r)| {
            let qualified = |exempt: bool| -> Vec<Option<String>> {
                let set: BTreeSet<Option<String>> = index
                    .links
                    .iter()
                    .filter(|l| l.req_id == *id && if exempt { l.is_exempt() } else { l.is_partial() })
                    .map(|l| l.clause.clone())
                    .collect();
                set.into_iter().collect()
            };
            Node {
                id: id.clone(),
                complete: r.decomposition == Decomposition::Complete,
                clauses: r.clause_keys(),
                exempt: qualified(true),
                partial_clauses: qualified(false),
                refines: r.refines.clone(),
            }
        })
        .collect()
}

/// Roll a requirement and its refinements up into a tree.
pub fn tree(
    index: &Index,
    id: &str,
    levels: &BTreeMap<(String, Option<String>), Level>,
    floor: Level,
) -> RollUp {
    roll_up(
        nodes(index),
        levels.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        id.to_string(),
        floor,
    )
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
    fn an_inexact_figure_renders_as_provisional() {
        assert_eq!(Figure { met: 1, total: 2, exact: true }.render(), "50%");
        assert_eq!(Figure { met: 1, total: 2, exact: false }.render(), "≤ 50% (provisional)");
    }

    /// @tests REQ-ROLLUP.never_complete_when_open
    #[test]
    fn an_inexact_figure_is_never_complete() {
        assert!(!Figure { met: 2, total: 2, exact: false }.is_complete());
        assert!(Figure { met: 2, total: 2, exact: true }.is_complete());
    }

    fn node(id: &str, clause: &str, refines: &[&str]) -> Node {
        Node {
            id: id.into(),
            complete: true,
            clauses: vec![Some(clause.into())],
            exempt: vec![],
            partial_clauses: vec![],
            refines: refines.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn at(id: &str, clause: &str, level: Level) -> ((String, Option<String>), Level) {
        ((id.into(), Some(clause.into())), level)
    }

    /// The diamond: B and C refine A, D refines both. A walk sharing one
    /// visited set across siblings put D under B only, so C read L4 while D was L1.
    ///
    /// @tests REQ-ROLLUP.counted_once
    /// @tests REQ-ROLLUP.deterministic_order
    /// @tests ARCH-HONEST.weakest_link
    #[test]
    fn a_diamond_counts_the_shared_child_under_both_parents() {
        let nodes = vec![
            node("A", "a", &[]),
            node("B", "b", &["A"]),
            node("C", "c", &["A"]),
            node("D", "d", &["B", "C"]),
        ];
        let levels = vec![
            at("A", "a", Level::L4),
            at("B", "b", Level::L4),
            at("C", "c", Level::L4),
            at("D", "d", Level::L1),
        ];
        let rolled = roll_up(nodes, levels, "A".into(), Level::L3);
        assert_eq!(rolled.covered, Figure { met: 3, total: 4, exact: true }, "D counted once at A");
        for parent in &rolled.children {
            assert_eq!(parent.assurance, Level::L1, "{} lost D", parent.id);
            assert_eq!(parent.covered, Figure { met: 1, total: 2, exact: true }, "{}", parent.id);
            assert_eq!(parent.children.len(), 1, "{} does not show D", parent.id);
        }
    }

    /// A cycle ends, and every member of it sees the others.
    ///
    /// @tests REQ-ROLLUP.counted_once
    #[test]
    fn a_cycle_ends_and_counts_each_member_once() {
        let nodes = vec![node("A", "a", &["B"]), node("B", "b", &["A"])];
        let levels = vec![at("A", "a", Level::L4), at("B", "b", Level::L2)];
        let rolled = roll_up(nodes, levels, "A".into(), Level::L3);
        assert_eq!(rolled.assurance, Level::L2);
        assert_eq!(rolled.covered.total, 2);
        assert_eq!(rolled.children.len(), 1);
        assert!(rolled.children[0].children.is_empty(), "the walk went round the cycle");
    }

    /// An exempt clause leaves both the denominator and the assurance; a
    /// partial one stays and is capped below the floor.
    ///
    /// @tests REQ-ROLLUP.exempt_leaves_denominator
    /// @tests REQ-ROLLUP.partial_capped
    #[test]
    fn exempt_leaves_and_partial_is_capped() {
        let mut a = node("A", "a", &[]);
        a.clauses = vec![Some("a".into()), Some("x".into()), Some("p".into())];
        a.exempt = vec![Some("x".into())];
        a.partial_clauses = vec![Some("p".into())];
        let levels = vec![at("A", "a", Level::L4), at("A", "p", Level::L4)];
        let rolled = roll_up(vec![a], levels, "A".into(), Level::L3);
        assert_eq!(rolled.covered, Figure { met: 1, total: 2, exact: true });
        assert_eq!(rolled.assurance, PARTIAL_CAP);
    }
}
