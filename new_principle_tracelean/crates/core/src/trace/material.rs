//! Assembling what a judge needs to see.
//!
//! The requirement↔model bond is the one nothing can check by running it, so the
//! whole of TraceLean's contribution is putting the two halves in front of a
//! person, correctly and completely: the clause as written, the model as it is
//! now, and both hashes so the judgement can be invalidated when either moves.
//!
//! Pure. Reading the model's source off disk is the caller's job, because this
//! must stay a function of the tree it is given rather than of the tree it
//! happens to run in.

use crate::judge::Material;
use crate::trace::annotation::Role;
use crate::trace::index::Index;

/// Why no material could be assembled.
///
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No requirement carries that identifier.
    NoRequirement { req_id: String },
    /// The requirement has no such clause.
    NoClause { req_id: String, clause: String },
    /// Nothing models the clause, so there is nothing to judge it against.
    NoModel,
}

/// Where the model for a clause lives, and what it hashed to.
///
/// Separate from the material because reading a file is an effect and this is
/// not: the caller takes the location, reads it, and hands the text back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAt {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub body_hash: String,
    pub link_hash: String,
}

/// Which declaration models a clause.
///
/// The first in sorted order when several do, so that asking twice puts the
/// same thing in front of the judge. A clause modelled by two declarations is a
/// judgement about the pair, and the caller can show both; what must not happen
/// is the choice moving between runs.
///
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn model_of(index: &Index, req_id: &str, clause: Option<&str>) -> Option<ModelAt> {
    let mut found: Vec<ModelAt> = index
        .links
        .iter()
        .filter(|link| {
            link.role == Role::Models && link.req_id == req_id && link.clause.as_deref() == clause
        })
        .map(|link| ModelAt {
            file: link.anchor.file.clone(),
            start_line: link.anchor.start_line,
            end_line: link.anchor.end_line,
            body_hash: link.anchor.body_hash.clone(),
            link_hash: link.link_hash.clone(),
        })
        .collect();
    found.sort_by(|a, b| (&a.file, a.start_line).cmp(&(&b.file, b.start_line)));
    found.into_iter().next()
}

/// What every declaration modelling a clause hashes to, together.
///
/// Sorted, deduplicated and joined, exactly as `earn::anchor_hash` does it, so
/// that a judgement's recorded input and the input recomputed against the tree
/// are the same string. They were not: the record carried `model_of`'s single
/// first-by-line hash while the check recomputed the join, so every clause with
/// two `@models` went stale the moment it was judged.
///
/// The join is the right side of that disagreement. A clause modelled by two
/// declarations is a judgement about the pair, and a record naming only one of
/// them would survive a change to the other — a judgement nobody made, still
/// standing.
fn model_hashes(index: &Index, req_id: &str, clause: Option<&str>) -> Option<String> {
    let mut found: Vec<&str> = index
        .links
        .iter()
        .filter(|link| {
            link.role == Role::Models && link.req_id == req_id && link.clause.as_deref() == clause
        })
        .map(|link| link.anchor.body_hash.as_str())
        .collect();
    found.sort();
    found.dedup();
    if found.is_empty() {
        return None;
    }
    Some(found.join("+"))
}

/// Assemble what the judge reads, given the model's source text.
///
/// The hashes are what makes the judgement perishable: `still_applies` compares
/// them, so a judgement survives exactly as long as the pair it was about.
///
/// @implements REQ-JUDGE.invalidated_by_change
/// @implements REQ-JUDGE.divergence_presented
pub fn assemble(
    index: &Index,
    req_id: &str,
    clause: Option<&str>,
    model_source: String,
    divergence: Option<String>,
) -> Result<(Material, ModelAt), Missing> {
    let requirement = index
        .requirements
        .get(req_id)
        .ok_or_else(|| Missing::NoRequirement { req_id: req_id.to_string() })?;

    let clause_text = match clause {
        None => requirement.body.clone(),
        Some(key) => requirement.clauses.get(key).cloned().ok_or_else(|| Missing::NoClause {
            req_id: req_id.to_string(),
            clause: key.to_string(),
        })?,
    };

    let at = model_of(index, req_id, clause).ok_or(Missing::NoModel)?;
    let material = Material {
        req_id: req_id.to_string(),
        clause: clause.map(str::to_string),
        clause_text,
        model_source,
        requirement_hash: requirement.content_hash.clone(),
        // Every model of the clause, not the one whose source is shown: the
        // judgement is about the pair when there are two, and the staleness
        // check asks the tree the same question this way.
        model_hash: model_hashes(index, req_id, clause).unwrap_or_else(|| at.body_hash.clone()),
        divergence,
    };
    Ok((material, at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::index::build;

    fn tree() -> (std::path::PathBuf, Index) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tracelean-material-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, content) in [
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: It shall do the thing.\n---\nbody"),
            ("src/m.rs", "// @models REQ-A.one\npub fn m() {}\n"),
        ] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let index = build(&dir);
        (dir, index)
    }

    #[test]
    fn the_material_carries_the_clause_as_written_and_both_hashes() {
        let (dir, index) = tree();
        let (material, at) =
            assemble(&index, "REQ-A", Some("one"), "fn m() {}".into(), None).unwrap();
        assert_eq!(material.clause_text, "It shall do the thing.");
        assert!(!material.requirement_hash.is_empty());
        assert_eq!(material.model_hash, at.body_hash);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clause_nothing_models_has_nothing_to_judge() {
        let (dir, index) = tree();
        assert_eq!(
            assemble(&index, "REQ-A", Some("two"), String::new(), None),
            Err(Missing::NoClause { req_id: "REQ-A".into(), clause: "two".into() })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
