use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::evidence::Marks;

use super::index::Index;
use super::requirement::Decomposition;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    pub met: u64,
    pub total: u64,
    pub exact: bool,
}

/// @implements REQ-ROLLUP.never_complete_when_open
/// @drt REQ-ROLLUP.never_complete_when_open
pub fn complete(figure: Figure) -> bool {
    figure.exact && figure.total > 0 && figure.met == figure.total
}

impl Figure {
    pub fn render(&self) -> String {
        render_figure(*self)
    }

    pub fn is_complete(&self) -> bool {
        complete(*self)
    }
}

/// @implements ARCH-HONEST.lower_bound_marked
/// @drt ARCH-HONEST.lower_bound_marked
pub fn render_figure(figure: Figure) -> String {
    let percent = if figure.total == 0 {
        0
    } else {
        (figure.met * 200 + figure.total) / (figure.total * 2)
    };
    if figure.exact {
        format!("{percent}%")
    } else {
        format!("≤ {percent}% (provisional)")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollUp {
    pub id: String,
    pub marks: Marks,
    pub covered: Figure,
    pub children: Vec<RollUp>,
}

/// @implements REQ-ROLLUP.every_part_holds
/// @implements ARCH-HONEST.weakest_link
/// @drt REQ-ROLLUP.every_part_holds
/// @drt ARCH-HONEST.weakest_link
pub fn combine(parts: Vec<Marks>) -> Marks {
    match parts.is_empty() {
        true => Marks::NONE,
        false => parts.into_iter().fold(Marks::NEUTRAL, Marks::and),
    }
}

/// @implements REQ-ROLLUP.untraced_counted
/// @implements ARCH-HONEST.untraced_visible
/// @drt REQ-ROLLUP.untraced_counted
pub fn file_coverage(scanned: Vec<String>, claimed: Vec<String>) -> Figure {
    let scanned: BTreeSet<String> = scanned.into_iter().collect();
    let claimed: BTreeSet<String> = claimed.into_iter().filter(|f| scanned.contains(f)).collect();
    Figure { met: claimed.len() as u64, total: scanned.len() as u64, exact: true }
}

/// @implements ARCH-CORE-SHELL.decision_total
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub complete: bool,
    pub clauses: Vec<Option<String>>,
    pub exempt: Vec<Option<String>>,
    #[serde(default)]
    pub partial_clauses: Vec<Option<String>>,
    pub refines: Vec<String>,
}

pub fn partial_cap(marks: Marks) -> Marks {
    Marks { tested: false, proved: false, ..marks }
}

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

fn own(node: &Node, levels: &BTreeMap<(String, Option<String>), Marks>, floor: Marks) -> (Vec<Marks>, u64) {
    let counted: Vec<Marks> = node
        .clauses
        .iter()
        .filter(|c| !node.exempt.contains(c))
        .map(|c| {
            let marks = levels.get(&(node.id.clone(), c.clone())).copied().unwrap_or(Marks::NONE);
            if node.partial_clauses.contains(c) {
                partial_cap(marks)
            } else {
                marks
            }
        })
        .collect();
    let met = counted.iter().filter(|m| m.holds(floor)).count() as u64;
    (counted, met)
}

/// @implements REQ-ROLLUP.open_is_lower_bound
/// @implements REQ-ROLLUP.exempt_leaves_denominator
/// @implements REQ-ROLLUP.partial_capped
/// @implements REQ-ROLLUP.deterministic_order
/// @implements REQ-ROLLUP.counted_once
/// @drt REQ-ROLLUP.open_is_lower_bound
/// @drt REQ-ROLLUP.exempt_leaves_denominator
/// @drt REQ-ROLLUP.partial_capped
/// @drt REQ-ROLLUP.deterministic_order
/// @drt REQ-ROLLUP.counted_once
pub fn roll_up(
    nodes: Vec<Node>,
    held: Vec<((String, Option<String>), Marks)>,
    root: String,
    floor: Marks,
) -> RollUp {
    let graph: BTreeMap<String, Node> =
        nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
    let held: BTreeMap<(String, Option<String>), Marks> = held.into_iter().collect();

    fn node_of(
        graph: &BTreeMap<String, Node>,
        id: &str,
        held: &BTreeMap<(String, Option<String>), Marks>,
        floor: Marks,
        path: &mut Vec<String>,
    ) -> RollUp {
        let members = reach(graph, id);
        let mut all = Vec::new();
        let (mut met, mut total, mut exact) = (0u64, 0u64, true);
        for member in &members {
            match graph.get(member) {
                Some(node) => {
                    let (counted, m) = own(node, held, floor);
                    met += m;
                    total += counted.len() as u64;
                    exact &= node.complete;
                    all.extend(counted);
                }
                None => exact = false,
            }
        }

        path.push(id.to_string());
        let mut children = Vec::new();
        for (child, node) in graph {
            if node.refines.iter().any(|p| p == id) && !path.contains(child) {
                children.push(node_of(graph, child, held, floor, path));
            }
        }
        path.pop();

        RollUp {
            id: id.to_string(),
            marks: combine(all),
            covered: Figure { met, total, exact },
            children,
        }
    }

    node_of(&graph, &root, &held, floor, &mut Vec::new())
}

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

pub fn tree(
    index: &Index,
    id: &str,
    held: &BTreeMap<(String, Option<String>), Marks>,
    floor: Marks,
) -> RollUp {
    roll_up(
        nodes(index),
        held.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        id.to_string(),
        floor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(text: &str) -> Marks {
        let at: Vec<char> = text.chars().collect();
        let held = |i: usize| at[i] != '-' && at[i] != '·';
        Marks { annotated: held(0), judged: held(1), tested: held(2), proved: held(3), structural: at[2] == '·' }
    }

    /// @tests REQ-ROLLUP.every_part_holds
    #[test]
    fn aggregation_holds_what_every_part_holds() {
        assert_eq!(combine(vec![m("AJTP"), m("A-TP")]), m("A-TP"));
        assert_eq!(combine(vec![m("AJ--"), m("A--P")]), m("A---"));
        assert_eq!(combine(vec![]), Marks::NONE);
    }

    #[test]
    fn a_mark_held_by_most_is_not_held() {
        let result = combine(vec![m("AJTP"), m("AJTP"), m("AJT-")]);
        assert!(!result.proved);
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

    fn at(id: &str, clause: &str, marks: &str) -> ((String, Option<String>), Marks) {
        ((id.into(), Some(clause.into())), m(marks))
    }

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
        let held = vec![
            at("A", "a", "AJTP"),
            at("B", "b", "AJTP"),
            at("C", "c", "AJTP"),
            at("D", "d", "A---"),
        ];
        let rolled = roll_up(nodes, held, "A".into(), m("--T-"));
        assert_eq!(rolled.covered, Figure { met: 3, total: 4, exact: true }, "D counted once at A");
        for parent in &rolled.children {
            assert_eq!(parent.marks, m("A---"), "{} lost D", parent.id);
            assert_eq!(parent.covered, Figure { met: 1, total: 2, exact: true }, "{}", parent.id);
            assert_eq!(parent.children.len(), 1, "{} does not show D", parent.id);
        }
    }

    /// @tests REQ-ROLLUP.counted_once
    #[test]
    fn a_cycle_ends_and_counts_each_member_once() {
        let nodes = vec![node("A", "a", &["B"]), node("B", "b", &["A"])];
        let held = vec![at("A", "a", "AJTP"), at("B", "b", "AJ--")];
        let rolled = roll_up(nodes, held, "A".into(), m("--T-"));
        assert_eq!(rolled.marks, m("AJ--"));
        assert_eq!(rolled.covered.total, 2);
        assert_eq!(rolled.children.len(), 1);
        assert!(rolled.children[0].children.is_empty(), "the walk went round the cycle");
    }

    /// @tests REQ-ROLLUP.exempt_leaves_denominator
    /// @tests REQ-ROLLUP.partial_capped
    #[test]
    fn exempt_leaves_and_partial_is_capped() {
        let mut a = node("A", "a", &[]);
        a.clauses = vec![Some("a".into()), Some("x".into()), Some("p".into())];
        a.exempt = vec![Some("x".into())];
        a.partial_clauses = vec![Some("p".into())];
        let held = vec![at("A", "a", "AJTP"), at("A", "p", "AJTP")];
        let rolled = roll_up(vec![a], held, "A".into(), m("--T-"));
        assert_eq!(rolled.covered, Figure { met: 1, total: 2, exact: true });
        assert_eq!(rolled.marks, m("AJ--"));
    }
}
