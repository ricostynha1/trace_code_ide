use crate::evidence::{Bond, Level};
use crate::trace::annotation::Role;
use crate::trace::index::Index;
use crate::trace::record::{Detail, Evidence, Key};

fn anchor_hash(index: &Index, req_id: &str, clause: Option<&str>, role: Role) -> Option<String> {
    let mut found: Vec<&str> = index
        .links
        .iter()
        .filter(|link| {
            link.role == role && link.req_id == req_id && link.clause.as_deref() == clause
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

fn link_hash(index: &Index, req_id: &str, clause: Option<&str>, role: Role) -> Option<String> {
    let mut found: Vec<&str> = index
        .links
        .iter()
        .filter(|link| {
            link.role == role && link.req_id == req_id && link.clause.as_deref() == clause
        })
        .map(|link| link.link_hash.as_str())
        .collect();
    found.sort();
    found.first().map(|h| h.to_string())
}

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unearned {
    NoLink { role: &'static str },
    NoInput { name: &'static str },
}

/// @implements REQ-EVID.record_reproducible
/// @implements REQ-STALE.inputs_identified
/// @implements REQ-STALE.retarget_invalidates
pub fn drt_record(
    index: &Index,
    req_id: &str,
    clause: Option<&str>,
    level: Level,
    seed: u64,
    cases: u64,
    op: &str,
) -> Result<Evidence, Unearned> {
    let link = link_hash(index, req_id, clause, Role::Drt)
        .ok_or(Unearned::NoLink { role: "drt" })?;
    with_link(index, req_id, clause, level, seed, cases, op, link)
}

pub fn derived_drt_record(
    index: &Index,
    req_id: &str,
    clause: Option<&str>,
    level: Level,
    seed: u64,
    cases: u64,
    op: &str,
) -> Result<Evidence, Unearned> {
    let link = link_hash(index, req_id, clause, Role::Drt)
        .or_else(|| link_hash(index, req_id, clause, Role::Implements))
        .ok_or(Unearned::NoLink { role: "implements" })?;
    with_link(index, req_id, clause, level, seed, cases, op, link)
}

#[allow(clippy::too_many_arguments)]
fn with_link(
    index: &Index,
    req_id: &str,
    clause: Option<&str>,
    level: Level,
    seed: u64,
    cases: u64,
    op: &str,
    link: String,
) -> Result<Evidence, Unearned> {
    let model = anchor_hash(index, req_id, clause, Role::Models)
        .ok_or(Unearned::NoInput { name: "model" })?;
    let implementation = anchor_hash(index, req_id, clause, Role::Implements)
        .ok_or(Unearned::NoInput { name: "implementation" })?;
    Ok(Evidence {
        key: Key {
            req_id: req_id.to_string(),
            clause: clause.map(str::to_string),
            bond: Bond::ModelImpl,
        },
        level,
        detail: Detail::Drt { seed, cases, op: op.to_string() },
        link_hash: link,
        inputs: vec![
            ("implementation".to_string(), implementation),
            ("model".to_string(), model),
            ("requirement".to_string(), requirement_hash(index, req_id, clause)?),
        ],
    })
}

/// @implements REQ-STALE.no_silent_revalidation
pub fn drt_stamp(index: &Index, clauses: &[String]) -> String {
    let mut parts: Vec<String> = clauses
        .iter()
        .map(|qualified| {
            let (req_id, clause) = crate::drt::config::split_op(&qualified);
            match drt_record(index, req_id, clause, Level::L1, 0, 0, "")
                .or_else(|_| derived_drt_record(index, req_id, clause, Level::L1, 0, 0, ""))
            {
                Ok(record) => {
                    let inputs: Vec<String> =
                        record.inputs.iter().map(|(name, hash)| format!("{name}={hash}")).collect();
                    format!("{qualified}|{}|{}", record.link_hash, inputs.join(","))
                }
                Err(why) => format!("{qualified}|unearned {why:?}"),
            }
        })
        .collect();
    parts.sort();
    crate::trace::hash::text(&parts.join("\n"))
}

/// @implements REQ-STALE.no_silent_revalidation
/// @implements REQ-DRT.case_count_stated
pub fn halves_compose(
    agreed: &[String],
    covered: Option<&str>,
    current: &str,
    implementations: usize,
) -> bool {
    covered == Some(current)
        && agreed.iter().filter(|stamp| stamp.as_str() == current).count() >= implementations
}

/// @implements REQ-STALE.requirement_reopens_all
fn requirement_hash(index: &Index, req_id: &str, clause: Option<&str>) -> Result<String, Unearned> {
    index
        .requirements
        .get(req_id)
        .and_then(|r| r.clause_hash(clause))
        .ok_or(Unearned::NoInput { name: "requirement" })
}

/// @implements REQ-EVID.record_reproducible
/// @implements REQ-STALE.inputs_identified
pub fn proof_record(
    index: &Index,
    req_id: &str,
    clause: Option<&str>,
    theorem_name: &str,
    toolchain: &str,
) -> Result<Evidence, Unearned> {
    let link = link_hash(index, req_id, clause, Role::Proves)
        .ok_or(Unearned::NoLink { role: "proves" })?;
    let model = anchor_hash(index, req_id, clause, Role::Models)
        .ok_or(Unearned::NoInput { name: "model" })?;
    Ok(Evidence {
        key: Key {
            req_id: req_id.to_string(),
            clause: clause.map(str::to_string),
            bond: Bond::ModelProof,
        },
        level: Level::L4,
        detail: Detail::Proof {
            theorem_name: theorem_name.to_string(),
            toolchain: toolchain.to_string(),
        },
        link_hash: link,
        inputs: vec![
            ("model".to_string(), model),
            ("requirement".to_string(), requirement_hash(index, req_id, clause)?),
            ("toolchain".to_string(), toolchain.to_string()),
            ("rules".to_string(), crate::trace::proofcheck::rules_hash()),
        ],
    })
}

pub fn slot(record: &Evidence) -> (String, Option<String>, Bond) {
    (record.key.req_id.clone(), record.key.clause.clone(), record.key.bond)
}

/// @implements REQ-LOCK.evidence_preserved
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn merge(held: Vec<Evidence>, produced: Vec<Evidence>) -> Vec<Evidence> {
    let mut out: Vec<Evidence> = Vec::new();
    for record in held.into_iter().chain(produced) {
        match out.iter_mut().find(|kept| slot(kept) == slot(&record)) {
            Some(kept) => *kept = record,
            None => out.push(record),
        }
    }
    out.sort_by_key(|record| {
        (record.key.req_id.clone(), record.key.clause.clone(), record.key.bond)
    });
    out
}

/// @implements REQ-STALE.no_silent_revalidation
/// @implements REQ-STALE.change_invalidates
pub fn still_standing(index: &Index, records: Vec<Evidence>) -> (Vec<Evidence>, Vec<Evidence>) {
    let live: Vec<String> = index.links.iter().map(|link| link.link_hash.clone()).collect();
    let mut valid = Vec::new();
    let mut stale = Vec::new();
    for record in records {
        let current = current_inputs(index, &record);
        match crate::trace::record::staleness(&record, &live, &current) {
            None => valid.push(record),
            Some(_) => stale.push(record),
        }
    }
    (valid, stale)
}

/// @implements REQ-STALE.stale_is_visible
pub fn why_stale(index: &Index, record: &Evidence) -> Option<crate::trace::record::Staleness> {
    let live: Vec<String> = index.links.iter().map(|link| link.link_hash.clone()).collect();
    crate::trace::record::staleness(record, &live, &current_inputs(index, record))
}

pub fn changed_inputs(index: &Index, record: &Evidence) -> Vec<String> {
    let now = current_inputs(index, record);
    record
        .inputs
        .iter()
        .filter(|(name, was)| now.iter().find(|(n, _)| n == name).map(|(_, h)| h) != Some(was))
        .map(|(name, _)| name.clone())
        .collect()
}

pub fn current_inputs(index: &Index, record: &Evidence) -> Vec<(String, String)> {
    let req = &record.key.req_id;
    let clause = record.key.clause.as_deref();
    record
        .inputs
        .iter()
        .filter_map(|(name, was)| {
            let now = match name.as_str() {
                "model" => crate::trace::material::model_hashes(index, req, clause),
                "implementation" => anchor_hash(index, req, clause, Role::Implements),
                "requirement" => index.requirements.get(req).and_then(|r| r.clause_hash(clause)),
                "toolchain" => Some(was.clone()),
                "rules" => Some(crate::trace::proofcheck::rules_hash()),
                _ => None,
            };
            now.map(|hash| (name.clone(), hash))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::index::build;

    fn tree(files: &[(&str, &str)]) -> (std::path::PathBuf, Index) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tracelean-earn-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, content) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let index = build(&dir);
        (dir, index)
    }

    const FILES: &[(&str, &str)] = &[
        ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
        (
            "src/i.rs",
            "// @models REQ-A.one\npub fn m() {}\n\n// @implements REQ-A.one\npub fn f() {}\n\n// @drt REQ-A.one\npub fn d() {}\n\n// @proves REQ-A.one\npub fn p() {}\n",
        ),
    ];

    #[test]
    fn a_differential_record_names_both_sides_it_compared() {
        let (dir, index) = tree(FILES);
        let record =
            drt_record(&index, "REQ-A", Some("one"), Level::L3, 7, 500, "REQ-A.one").unwrap();
        assert_eq!(record.key.bond, Bond::ModelImpl);
        let named: Vec<&str> = record.inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(named, vec!["implementation", "model", "requirement"]);
        assert_eq!(
            crate::trace::record::reproducibility(record),
            crate::trace::record::Reproducibility::Reproducible
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// @tests REQ-STALE.no_silent_revalidation
    /// @tests REQ-DRT.case_count_stated
    #[test]
    fn halves_stamped_against_other_inputs_do_not_compose() {
        let (dir, before) = tree(FILES);
        let clauses = vec!["REQ-A.one".to_string()];
        let old = drt_stamp(&before, &clauses);
        let _ = std::fs::remove_dir_all(&dir);
        let (dir, after) = tree(&[
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
            (
                "src/i.rs",
                "// @models REQ-A.one\npub fn m() {}\n\n// @implements REQ-A.one\npub fn f() { changed() }\n\n// @drt REQ-A.one\npub fn d() {}\n",
            ),
        ]);
        let new = drt_stamp(&after, &clauses);
        assert_ne!(old, new, "changing the implementation changes the stamp");
        assert!(!halves_compose(&[old.clone()], Some(&new), &new, 1), "a stale agreed half composed");
        assert!(!halves_compose(&[new.clone()], Some(&old), &new, 1), "a stale coverage half composed");
        assert!(!halves_compose(&[new.clone()], None, &new, 1));
        assert!(!halves_compose(&[new.clone(), old.clone()], Some(&new), &new, 2));
        assert!(halves_compose(&[new.clone()], Some(&new), &new, 1));
        assert_eq!(drt_stamp(&after, &clauses), new, "the stamp is a function of the tree");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clause_nothing_implements_earns_no_differential_record() {
        let (dir, index) = tree(&[
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
            ("src/i.rs", "// @models REQ-A.one\npub fn m() {}\n\n// @drt REQ-A.one\npub fn d() {}\n"),
        ]);
        assert_eq!(
            drt_record(&index, "REQ-A", Some("one"), Level::L3, 7, 500, "REQ-A.one"),
            Err(Unearned::NoInput { name: "implementation" })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_judgement_does_not_go_stale_against_the_tree_it_was_made_on() {
        let (dir, index) = tree(&[
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
            (
                "src/i.rs",
                "// @models REQ-A.one\npub fn m() {}\n\n// @models REQ-A.one\npub fn n() {}\n\n// @implements REQ-A.one\npub fn f() {}\n",
            ),
        ]);
        let (material, at) = crate::trace::material::assemble(
            &index,
            "REQ-A",
            Some("one"),
            "pub fn m() {}".into(),
            None,
        )
        .unwrap();
        let judgement = crate::judge::Judgement {
            verdict: crate::judge::Verdict::Agrees,
            judged_by: "someone".into(),
            delegated_by: None,
            note: None,
            requirement_hash: material.requirement_hash.clone(),
            model_hash: material.model_hash.clone(),
        };
        let crate::judge::Outcome::Recorded { evidence } =
            crate::judge::record(material, judgement, None, at.link_hash)
        else {
            panic!("an `agrees` verdict recorded nothing");
        };

        let (valid, stale) = still_standing(&index, vec![*evidence]);
        assert!(stale.is_empty(), "a judgement went stale on the tree it was made on");
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].level, Level::L2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_judgement_goes_stale_when_a_model_it_was_about_changes() {
        let (dir, index) = tree(&[
            ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: First.\n---\nbody"),
            (
                "src/i.rs",
                "// @models REQ-A.one\npub fn m() {}\n\n// @models REQ-A.one\npub fn n() {}\n\n// @implements REQ-A.one\npub fn f() {}\n",
            ),
        ]);
        let (material, at) = crate::trace::material::assemble(
            &index,
            "REQ-A",
            Some("one"),
            "pub fn m() {}".into(),
            None,
        )
        .unwrap();
        let judgement = crate::judge::Judgement {
            verdict: crate::judge::Verdict::Agrees,
            judged_by: "someone".into(),
            delegated_by: None,
            note: None,
            requirement_hash: material.requirement_hash.clone(),
            model_hash: material.model_hash.clone(),
        };
        let crate::judge::Outcome::Recorded { evidence } =
            crate::judge::record(material, judgement, None, at.link_hash)
        else {
            panic!("an `agrees` verdict recorded nothing");
        };

        std::fs::write(
            dir.join("src/i.rs"),
            "// @models REQ-A.one\npub fn m() {}\n\n// @models REQ-A.one\npub fn n() { 1 }\n\n// @implements REQ-A.one\npub fn f() {}\n",
        )
        .unwrap();
        let (valid, stale) = still_standing(&build(&dir), vec![*evidence]);
        assert!(valid.is_empty(), "the judgement survived a change to a model it covered");
        assert_eq!(stale.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// @tests REQ-STALE.requirement_reopens_all
    #[test]
    fn rewording_a_requirement_reopens_its_tests_and_proofs() {
        let (dir, index) = tree(FILES);
        let drt = drt_record(&index, "REQ-A", Some("one"), Level::L3, 7, 500, "REQ-A.one").unwrap();
        let proof = proof_record(&index, "REQ-A", Some("one"), "a_theorem", "4.12.0").unwrap();
        let (valid, _) = still_standing(&index, vec![drt.clone(), proof.clone()]);
        assert_eq!(valid.len(), 2);
        std::fs::write(dir.join("reqs/a.md"), "---\nid: REQ-A\nclauses:\n  one: First, reworded.\n---\nbody").unwrap();
        let (valid, stale) = still_standing(&build(&dir), vec![drt, proof]);
        assert!(valid.is_empty(), "a record survived its requirement being reworded");
        assert_eq!(stale.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// @tests REQ-STALE.requirement_reopens_all
    /// @tests REQ-REQDOC.clause_addressable
    #[test]
    fn rewording_one_clause_reopens_its_judgement_and_no_sibling_s() {
        let doc = "---\nid: REQ-A\nclauses:\n  a: First.\n  b: Second.\n---\nbody";
        let (dir, index) = tree(&[
            ("reqs/a.md", doc),
            (
                "src/i.rs",
                "// @models REQ-A.a\npub fn m() {}\n\n// @models REQ-A.b\npub fn n() {}\n",
            ),
        ]);
        let judged = |clause: &str| {
            let (material, at) = crate::trace::material::assemble(
                &index,
                "REQ-A",
                Some(clause),
                "pub fn m() {}".into(),
                None,
            )
            .unwrap();
            let judgement = crate::judge::Judgement {
                verdict: crate::judge::Verdict::Agrees,
                judged_by: "someone".into(),
                delegated_by: None,
                note: None,
                requirement_hash: material.requirement_hash.clone(),
                model_hash: material.model_hash.clone(),
            };
            let crate::judge::Outcome::Recorded { evidence } =
                crate::judge::record(material, judgement, None, at.link_hash)
            else {
                panic!("an `agrees` verdict recorded nothing");
            };
            *evidence
        };
        let (a, b) = (judged("a"), judged("b"));
        let (valid, _) = still_standing(&index, vec![a.clone(), b.clone()]);
        assert_eq!(valid.len(), 2);

        std::fs::write(dir.join("reqs/a.md"), doc.replace("First.", "First, reworded.")).unwrap();
        let (valid, stale) = still_standing(&build(&dir), vec![a, b.clone()]);
        assert_eq!(stale.len(), 1, "rewording `a` re-opened {} judgements", stale.len());
        assert_eq!(stale[0].key.clause.as_deref(), Some("a"));
        assert_eq!(valid, vec![b]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_proof_record_depends_on_the_checker_that_accepted_it() {
        let (dir, index) = tree(FILES);
        let record = proof_record(&index, "REQ-A", Some("one"), "a_theorem", "4.12.0").unwrap();
        assert_eq!(record.level, Level::L4);
        assert!(record.inputs.iter().any(|(n, h)| n == "toolchain" && h == "4.12.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_backend_replaces_its_own_record_rather_than_adding_one() {
        let (dir, index) = tree(FILES);
        let first = drt_record(&index, "REQ-A", Some("one"), Level::L1, 1, 10, "REQ-A.one").unwrap();
        let second =
            drt_record(&index, "REQ-A", Some("one"), Level::L3, 2, 500, "REQ-A.one").unwrap();
        let merged = merge(vec![first], vec![second.clone()]);
        assert_eq!(merged, vec![second]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_the_implementation_makes_the_record_stale() {
        let (dir, index) = tree(FILES);
        let record =
            drt_record(&index, "REQ-A", Some("one"), Level::L3, 7, 500, "REQ-A.one").unwrap();
        let (valid, stale) = still_standing(&index, vec![record.clone()]);
        assert_eq!(valid.len(), 1, "a record went stale against the tree it was earned from");
        assert!(stale.is_empty());

        std::fs::write(
            dir.join("src/i.rs"),
            "// @models REQ-A.one\npub fn m() {}\n\n// @implements REQ-A.one\npub fn f() { 1 }\n\n// @drt REQ-A.one\npub fn d() {}\n\n// @proves REQ-A.one\npub fn p() {}\n",
        )
        .unwrap();
        let (valid, stale) = still_standing(&build(&dir), vec![record]);
        assert!(valid.is_empty(), "the record survived a change to what it checked");
        assert_eq!(stale.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
