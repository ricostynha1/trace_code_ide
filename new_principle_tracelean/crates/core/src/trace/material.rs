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
    declarations_of(index, req_id, clause, Role::Models).into_iter().next()
}

/// Every declaration claiming a clause in one role, in file and line order.
///
/// A clause has one model and at most one specification (ADR-0014); until the
/// clauses with several are sorted out, the judge is shown all of them.
pub fn declarations_of(index: &Index, req_id: &str, clause: Option<&str>, role: Role) -> Vec<ModelAt> {
    let mut found: Vec<ModelAt> = index
        .links
        .iter()
        .filter(|link| link.role == role && link.req_id == req_id && link.clause.as_deref() == clause)
        .map(|link| ModelAt {
            file: link.anchor.file.clone(),
            start_line: link.anchor.start_line,
            end_line: link.anchor.end_line,
            body_hash: link.anchor.body_hash.clone(),
            link_hash: link.link_hash.clone(),
        })
        .collect();
    found.sort_by(|a, b| (&a.file, a.start_line).cmp(&(&b.file, b.start_line)));
    found.dedup_by(|a, b| a.file == b.file && a.start_line == b.start_line);
    found
}

/// What the judge reads as "the model": each model and the specification, with
/// their sources, labelled where there is more than the one model.
///
/// `models` and `specs` pair each declaration with its source text. One model
/// and no specification reads exactly as the model's own source.
///
/// @implements REQ-JUDGE.prompt_exported
pub fn shown_source(models: &[(ModelAt, String)], specs: &[(ModelAt, String)]) -> String {
    if models.len() == 1 && specs.is_empty() {
        return models[0].1.clone();
    }
    let mut sections = Vec::new();
    for (n, (at, source)) in models.iter().enumerate() {
        let label = if models.len() == 1 {
            "model".to_string()
        } else {
            format!("model {} of {} (a clause should have one)", n + 1, models.len())
        };
        sections.push(format!("-- {label}: {}:{}\n{}", at.file, at.start_line + 1, source.trim_end()));
    }
    for (at, source) in specs {
        sections.push(format!("-- specification: {}:{}\n{}", at.file, at.start_line + 1, source.trim_end()));
    }
    sections.join("\n\n")
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

    // A clause's narrowings are part of what it requires, so the judge reads
    // them with it, and the hash below covers them.
    let clause_text = match clause {
        None => requirement.body.clone(),
        Some(key) => requirement.clause_text_with_narrowings(key).ok_or_else(|| Missing::NoClause {
            req_id: req_id.to_string(),
            clause: key.to_string(),
        })?,
    };
    let requirement_hash = requirement.clause_hash(clause).ok_or_else(|| Missing::NoClause {
        req_id: req_id.to_string(),
        clause: clause.unwrap_or_default().to_string(),
    })?;

    let at = model_of(index, req_id, clause).ok_or(Missing::NoModel)?;
    let material = Material {
        req_id: req_id.to_string(),
        clause: clause.map(str::to_string),
        clause_text,
        model_source,
        requirement_hash,
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

    /// The judge sees the model and the specification, each labelled with
    /// where it is; a lone model reads as itself.
    ///
    /// @tests REQ-JUDGE.prompt_exported
    #[test]
    fn the_model_and_the_specification_are_both_shown() {
        let at = |file: &str, line: u32| ModelAt {
            file: file.into(),
            start_line: line,
            end_line: line,
            body_hash: String::new(),
            link_hash: String::new(),
        };
        let model = (at("m.lean", 3), "def f := 1".to_string());
        assert_eq!(shown_source(&[model.clone()], &[]), "def f := 1");
        let both = shown_source(&[model.clone()], &[(at("s.lean", 9), "def P : Prop := True".into())]);
        assert_eq!(both, "-- model: m.lean:4\ndef f := 1\n\n-- specification: s.lean:10\ndef P : Prop := True");
        let two = shown_source(&[model.clone(), (at("m.lean", 7), "def g := 2".into())], &[]);
        assert!(two.contains("model 1 of 2") && two.contains("model 2 of 2") && two.contains("def g := 2"), "{two}");
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
