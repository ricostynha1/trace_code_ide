use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::lockfile::Lockfile;
use super::proofcheck::{ProofReport, Verdict};
use super::requirement::Requirement;
use crate::evidence::{marks, structural, Mark, Record};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Linked {
    pub role: String,
    pub symbol: String,
    pub body_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClauseState {
    pub req_id: String,
    pub clause: Option<String>,
    pub text: String,
    pub held: Vec<Mark>,
    pub links: Vec<Linked>,
    pub assumed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Change {
    Added,
    Removed,
    Reworded { before: String, after: String },
    MarkLost { mark: Mark },
    MarkGained { mark: Mark },
    LinkRemoved { role: String, symbol: String },
    LinkAdded { role: String, symbol: String },
    BodyChanged { role: String, symbol: String },
    AssumptionDropped { name: String },
    AssumptionAdded { name: String },
}

impl Change {
    pub fn is_loss(&self) -> bool {
        matches!(self, Change::Removed | Change::MarkLost { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClauseDiff {
    pub req_id: String,
    pub clause: Option<String>,
    pub changes: Vec<Change>,
}

impl ClauseDiff {
    pub fn is_loss(&self) -> bool {
        self.changes.iter().any(Change::is_loss)
    }
}

const ALL_MARKS: [Mark; 4] = [Mark::Annotated, Mark::Judged, Mark::Tested, Mark::Proved];

fn same_link(a: &Linked, b: &Linked) -> bool {
    a.role == b.role && a.symbol == b.symbol
}

fn changes_of(req_id: &str, clause: &Option<String>, before: Option<&ClauseState>, after: Option<&ClauseState>) -> Vec<Change> {
    let empty = ClauseState { req_id: req_id.to_string(), clause: clause.clone(), ..Default::default() };
    let b = before.unwrap_or(&empty);
    let a = after.unwrap_or(&empty);
    let mut out = Vec::new();
    match (before, after) {
        (Some(_), None) => out.push(Change::Removed),
        (None, Some(_)) => out.push(Change::Added),
        (Some(b), Some(a)) if b.text != a.text => {
            out.push(Change::Reworded { before: b.text.clone(), after: a.text.clone() })
        }
        _ => {}
    }
    out.extend(ALL_MARKS.iter().filter(|m| b.held.contains(m) && !a.held.contains(m)).map(|&mark| Change::MarkLost { mark }));
    out.extend(ALL_MARKS.iter().filter(|m| !b.held.contains(m) && a.held.contains(m)).map(|&mark| Change::MarkGained { mark }));
    out.extend(
        b.links
            .iter()
            .filter(|l| !a.links.iter().any(|m| same_link(l, m)))
            .map(|l| Change::LinkRemoved { role: l.role.clone(), symbol: l.symbol.clone() }),
    );
    out.extend(
        a.links
            .iter()
            .filter(|l| !b.links.iter().any(|m| same_link(l, m)))
            .map(|l| Change::LinkAdded { role: l.role.clone(), symbol: l.symbol.clone() }),
    );
    out.extend(
        b.links
            .iter()
            .filter(|l| a.links.iter().any(|m| same_link(l, m)) && !a.links.contains(l))
            .map(|l| Change::BodyChanged { role: l.role.clone(), symbol: l.symbol.clone() }),
    );
    out.extend(b.assumed.iter().filter(|x| !a.assumed.contains(x)).map(|name| Change::AssumptionDropped { name: name.clone() }));
    out.extend(a.assumed.iter().filter(|x| !b.assumed.contains(x)).map(|name| Change::AssumptionAdded { name: name.clone() }));
    out
}

/// @implements REQ-DIFF.per_clause
/// @implements REQ-DIFF.text_shown
/// @implements REQ-DIFF.marks_moved
/// @implements REQ-DIFF.links_moved
/// @implements REQ-DIFF.assumptions_moved
/// @implements REQ-DIFF.unchanged_omitted
/// @implements REQ-DIFF.losses_first
/// @implements ARCH-DETERMINISM.stable_ordering
/// @drt REQ-DIFF.per_clause
/// @drt REQ-DIFF.text_shown
/// @drt REQ-DIFF.marks_moved
/// @drt REQ-DIFF.links_moved
/// @drt REQ-DIFF.assumptions_moved
/// @drt REQ-DIFF.unchanged_omitted
/// @drt REQ-DIFF.losses_first
pub fn diff(before: Vec<ClauseState>, after: Vec<ClauseState>) -> Vec<ClauseDiff> {
    let keys: BTreeSet<(String, Option<String>)> =
        before.iter().chain(&after).map(|s| (s.req_id.clone(), s.clause.clone())).collect();
    let find = |states: &[ClauseState], key: &(String, Option<String>)| {
        states.iter().find(|s| s.req_id == key.0 && s.clause == key.1).cloned()
    };
    let all: Vec<ClauseDiff> = keys
        .iter()
        .map(|key| ClauseDiff {
            req_id: key.0.clone(),
            clause: key.1.clone(),
            changes: changes_of(&key.0, &key.1, find(&before, key).as_ref(), find(&after, key).as_ref()),
        })
        .filter(|d| !d.changes.is_empty())
        .collect();
    let (losses, rest): (Vec<ClauseDiff>, Vec<ClauseDiff>) = all.into_iter().partition(ClauseDiff::is_loss);
    losses.into_iter().chain(rest).collect()
}

/// @implements REQ-DIFF.per_clause
pub fn states(lock: &Lockfile, requirements: &BTreeMap<String, Requirement>, proofs: &ProofReport) -> Vec<ClauseState> {
    let mut out = Vec::new();
    for (id, requirement) in requirements {
        for clause in requirement.clause_keys() {
            let here: Vec<_> = lock.links.iter().filter(|l| l.req == *id && l.clause == clause).collect();
            let records: Vec<Record> = lock
                .evidence
                .iter()
                .filter(|r| r.key.req_id == *id && r.key.clause == clause)
                .map(|r| Record { bond: r.key.bond, level: r.effective_level() })
                .collect();
            let held = marks(records, !here.is_empty());
            let by_reading = here.iter().any(|l| l.qualifier.as_deref() == Some("structural"))
                && !here.iter().any(|l| l.role == "models");
            let held = if by_reading { structural(held) } else { held };
            let mut links: Vec<Linked> = here
                .iter()
                .map(|l| Linked { role: l.role.clone(), symbol: l.anchor.clone(), body_hash: l.body_hash.clone() })
                .collect();
            links.sort();
            links.dedup();
            let assumed: BTreeSet<String> = proofs
                .proofs
                .iter()
                .filter(|p| p.req_id == *id && p.clause == clause)
                .filter_map(|p| match &p.verdict {
                    Verdict::Accepted { assumed } => Some(assumed.clone()),
                    Verdict::Refused { .. } => None,
                })
                .flatten()
                .collect();
            let text = match &clause {
                Some(key) => requirement.clause_text_with_narrowings(key).unwrap_or_default(),
                None => requirement.body.trim().to_string(),
            };
            out.push(ClauseState {
                req_id: id.clone(),
                clause,
                text,
                held: held.each().iter().filter(|(mark, on, _)| *on && held.applies(*mark)).map(|(mark, _, _)| *mark).collect(),
                links,
                assumed: assumed.into_iter().collect(),
            });
        }
    }
    out
}

fn mark_name(mark: Mark) -> &'static str {
    match mark {
        Mark::Annotated => "A",
        Mark::Judged => "J",
        Mark::Tested => "T",
        Mark::Proved => "P",
    }
}

pub fn said(change: &Change) -> String {
    match change {
        Change::Added => "added".into(),
        Change::Removed => "REMOVED".into(),
        Change::Reworded { before, after } => format!("reworded\n      was: {before}\n      now: {after}"),
        Change::MarkLost { mark } => format!("LOST {}", mark_name(*mark)),
        Change::MarkGained { mark } => format!("gained {}", mark_name(*mark)),
        Change::LinkRemoved { role, symbol } => format!("@{role} removed: {symbol}"),
        Change::LinkAdded { role, symbol } => format!("@{role} added: {symbol}"),
        Change::BodyChanged { role, symbol } => format!("@{role} body changed: {symbol}"),
        Change::AssumptionDropped { name } => format!("no longer assumes {name}"),
        Change::AssumptionAdded { name } => format!("NOW ASSUMES {name}"),
    }
}

pub fn render(base: &str, diffs: &[ClauseDiff]) -> String {
    let losses = diffs.iter().filter(|d| d.is_loss()).count();
    let mut out = format!("== against {base}: {} clauses changed, {losses} with a loss ==\n", diffs.len());
    for d in diffs {
        out.push_str(&format!("{}\n", crate::drt::qualified_op(&d.req_id, d.clause.as_deref())));
        for change in &d.changes {
            out.push_str(&format!("    {}\n", said(change)));
        }
    }
    out
}
