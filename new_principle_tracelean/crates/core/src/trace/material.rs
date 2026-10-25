use crate::judge::Material;
use crate::trace::annotation::Role;
use crate::trace::index::Index;

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    NoRequirement { req_id: String },
    NoClause { req_id: String, clause: String },
    NoModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelAt {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub body_hash: String,
    pub link_hash: String,
}

/// @implements ARCH-DETERMINISM.stable_ordering
/// @implements REQ-JUDGE.structural_judged
pub fn model_of(index: &Index, req_id: &str, clause: Option<&str>) -> Option<ModelAt> {
    declarations_of(index, req_id, clause, Role::Models)
        .into_iter()
        .next()
        .or_else(|| structural_checks(index, req_id, clause).into_iter().next())
}

pub fn structural_checks(index: &Index, req_id: &str, clause: Option<&str>) -> Vec<ModelAt> {
    let mut found: Vec<ModelAt> = index
        .links
        .iter()
        .filter(|link| link.is_structural() && link.req_id == req_id && link.clause.as_deref() == clause)
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

/// @implements REQ-JUDGE.structural_judged
pub fn structural_source(checks: &[(ModelAt, String)]) -> String {
    checks
        .iter()
        .map(|(at, source)| {
            format!(
                "-- structural check: a test reading the repository, {}:{}\n{}",
                at.file,
                at.start_line + 1,
                source.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn model_hashes(index: &Index, req_id: &str, clause: Option<&str>) -> Option<String> {
    let claims = |keep: &dyn Fn(&crate::trace::index::Link) -> bool| -> Vec<String> {
        index
            .links
            .iter()
            .filter(|link| keep(link) && link.req_id == req_id && link.clause.as_deref() == clause)
            .map(|link| link.anchor.body_hash.clone())
            .collect()
    };
    rests_on(claims(&|link| link.role == Role::Models), claims(&|link| link.is_structural()))
}

/// @implements REQ-JUDGE.structural_judged
/// @drt REQ-JUDGE.structural_judged
pub fn rests_on(models: Vec<String>, checks: Vec<String>) -> Option<String> {
    let mut found = if models.is_empty() { checks } else { models };
    found.sort();
    found.dedup();
    if found.is_empty() {
        return None;
    }
    Some(found.join("+"))
}

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

    /// @tests REQ-JUDGE.structural_judged
    #[test]
    fn a_structural_clause_is_judged_against_its_test() {
        let dir = std::env::temp_dir().join(format!("tracelean-material-structural-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |name: &str, content: &str| {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        };
        write("reqs/a.md", "---\nid: REQ-A\nclauses:\n  shape: The tree shall have a shape.\n---\nbody");
        let test = "/// @tests REQ-A.shape\n/// @structural REQ-A.shape reason=\"a shape\"\n#[test]\nfn shaped() {}\n";
        write("tests/a.rs", test);
        let index = build(&dir);
        let at = model_of(&index, "REQ-A", Some("shape")).expect("the structural test stands in");
        assert_eq!(at.file, "tests/a.rs");
        let (material, _) = assemble(&index, "REQ-A", Some("shape"), String::new(), None).unwrap();
        assert_eq!(Some(material.model_hash.clone()), model_hashes(&index, "REQ-A", Some("shape")));
        let shown = structural_source(&[(at, "fn shaped() {}".into())]);
        assert!(shown.starts_with("-- structural check: a test reading the repository, tests/a.rs:"), "{shown}");

        write("tests/a.rs", &test.replace("fn shaped() {}", "fn shaped() { changed() }"));
        let moved = build(&dir);
        assert_ne!(model_hashes(&moved, "REQ-A", Some("shape")), Some(material.model_hash));
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
