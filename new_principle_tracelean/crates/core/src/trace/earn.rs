//! Turning a backend's result into an evidence record.
//!
//! Every other module here decides what a claim is *worth*; this is where a
//! claim stops being a claim. It is pure on purpose: what a differential run or
//! a kernel-checked proof establishes is a function of the run and the tree, and
//! a backend that could decide its own level could award itself any.
//!
//! Two rules it exists to keep:
//!
//! - A record names the inputs it depended on, so a change underneath it makes
//!   it stale rather than leaving it standing (`REQ-STALE.inputs_identified`).
//! - A record is keyed to the link it was established for, so retargeting the
//!   annotation invalidates it (`REQ-STALE.retarget_invalidates`).

use crate::evidence::{Bond, Level};
use crate::trace::annotation::Role;
use crate::trace::index::Index;
use crate::trace::record::{Detail, Evidence, Key};

/// The hash of the one anchor a role claims for a clause.
///
/// `None` when nothing claims it, which is a record that must not be written:
/// a differential run naming no implementation has nothing to go stale against.
fn anchor_hash(index: &Index, req_id: &str, clause: Option<&str>, role: Role) -> Option<String> {
    let mut found: Vec<&str> = index
        .links
        .iter()
        .filter(|link| {
            link.role == role && link.req_id == req_id && link.clause.as_deref() == clause
        })
        .map(|link| link.anchor.body_hash.as_str())
        .collect();
    // Sorted and joined rather than "the first one": a clause implemented by two
    // functions depends on both, and picking one would leave a record that
    // survives a change to the other.
    found.sort();
    found.dedup();
    if found.is_empty() {
        return None;
    }
    Some(found.join("+"))
}

/// The identity of the link a record is established for.
///
/// The `@drt` annotation for a differential record, the `@proves` annotation for
/// a proof: the record belongs to the claim that the check happened, not to the
/// implementation it checked.
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

/// Why a record could not be written.
///
/// Named rather than swallowed: a backend that silently produced nothing would
/// look exactly like a backend that never ran.
///
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unearned {
    /// Nothing in the tree carries the annotation this record would belong to.
    NoLink { role: &'static str },
    /// An input the method depends on is not claimed by anything.
    NoInput { name: &'static str },
}

/// What a differential run established for one clause.
///
/// The level is `coverage::level`'s and not this function's: agreement alone is
/// not L3, and a run that met no floor establishes less than one that did.
///
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

/// What a derived differential run established (`drt::derive`): the same
/// record, resting on the `@implements` claim, because nobody wrote a `@drt`
/// one — the run itself is the claim that the code was tested against the
/// model, and retargeting the implementation still invalidates it.
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

/// The inputs a differential claim would rest on if it were recorded now: for
/// every clause it establishes, the link and the hashed inputs its record would
/// name, folded into one hash.
///
/// A differential record is earned in two halves that arrive from two test
/// processes — the runs agreed, the generator reached its floors — and each half
/// is stamped with this when it is established. A half stamped against other
/// code is about other code.
///
/// @implements REQ-STALE.no_silent_revalidation
pub fn drt_stamp(index: &Index, clauses: &[String]) -> String {
    let mut parts: Vec<String> = clauses
        .iter()
        .map(|qualified| {
            let (req_id, clause) = match qualified.split_once('.') {
                Some((req, clause)) => (req, Some(clause)),
                None => (qualified.as_str(), None),
            };
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

/// Whether the two halves of a differential claim may be composed into a record.
///
/// Only when the coverage half and at least one agreeing run per implementation
/// were all established against the inputs that are current now. Without the
/// stamps an "agreed" half left over from before a change, joined by a fresh
/// coverage half, wrote an L3 record before any run had compared the new code
/// with the model — a record made valid by a backend that never ran on it.
///
/// @implements REQ-STALE.no_silent_revalidation
pub fn halves_compose(
    agreed: &[String],
    covered: Option<&str>,
    current: &str,
    implementations: usize,
) -> bool {
    covered == Some(current)
        && agreed.iter().filter(|stamp| stamp.as_str() == current).count() >= implementations
}

/// The clause's text, as every record about it carries it: a clause reworded
/// or narrowed re-opens the judgement of its model, and the tests and proofs
/// made against that model too, until they are made again — and rewording a
/// sibling clause re-opens none of them (`Requirement::clause_hash`).
///
/// @implements REQ-STALE.requirement_reopens_all
fn requirement_hash(index: &Index, req_id: &str, clause: Option<&str>) -> Result<String, Unearned> {
    index
        .requirements
        .get(req_id)
        .and_then(|r| r.clause_hash(clause))
        .ok_or(Unearned::NoInput { name: "requirement" })
}

/// What a kernel-checked proof established for one clause.
///
/// The toolchain is an input because a theorem is only as accepted as the
/// checker that accepted it: a record that survived a toolchain change would be
/// claiming something nobody rechecked.
///
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
        ],
    })
}

/// A record's place in the set: one per requirement, clause and bond.
///
/// Two runs of the same backend over the same clause are the same record, not
/// two — otherwise a backend could raise a level by running twice.
pub fn slot(record: &Evidence) -> (String, Option<String>, Bond) {
    (record.key.req_id.clone(), record.key.clause.clone(), record.key.bond)
}

/// Fold newly produced records into the ones already held.
///
/// A new record replaces the one in its slot rather than joining it, and the
/// result is sorted, so the lock file's bytes do not depend on the order the
/// backends happened to finish in.
///
/// Nothing here judges: a record that arrives is taken at what it says, capped
/// only by `effective_level`, which is the method's own ceiling. This function
/// cannot promote anything — the most it does is let a backend replace its own
/// earlier answer.
///
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

/// The records that still apply, given the tree as it is now.
///
/// Stale ones are dropped from what gets written, not repaired: only the backend
/// that owns a key may make it valid again.
///
/// @implements REQ-STALE.no_silent_revalidation
/// @implements REQ-STALE.change_invalidates
pub fn still_standing(index: &Index, records: Vec<Evidence>) -> (Vec<Evidence>, Vec<Evidence>) {
    let live: Vec<String> = index.links.iter().map(|link| link.link_hash.clone()).collect();
    let mut valid = Vec::new();
    let mut stale = Vec::new();
    for record in records {
        // Each record names its own inputs and what they hashed to when it was
        // earned; what they hash to now comes from the tree the same way.
        let current = current_inputs(index, &record);
        match crate::trace::record::staleness(&record, &live, &current) {
            None => valid.push(record),
            Some(_) => stale.push(record),
        }
    }
    (valid, stale)
}

/// Why a record no longer stands, if it does not: which input moved, or that
/// its claim was retargeted.
///
/// @implements REQ-STALE.stale_is_visible
pub fn why_stale(index: &Index, record: &Evidence) -> Option<crate::trace::record::Staleness> {
    let live: Vec<String> = index.links.iter().map(|link| link.link_hash.clone()).collect();
    crate::trace::record::staleness(record, &live, &current_inputs(index, record))
}

/// Every input of a record whose hash is not what it was — all of them, where
/// `why_stale` names the first. A person deciding whether to re-approve needs
/// to know whether only the requirement's hash moved or the model did too.
pub fn changed_inputs(index: &Index, record: &Evidence) -> Vec<String> {
    let now = current_inputs(index, record);
    record
        .inputs
        .iter()
        .filter(|(name, was)| now.iter().find(|(n, _)| n == name).map(|(_, h)| h) != Some(was))
        .map(|(name, _)| name.clone())
        .collect()
}

/// What a record's named inputs hash to now.
pub fn current_inputs(index: &Index, record: &Evidence) -> Vec<(String, String)> {
    let req = &record.key.req_id;
    let clause = record.key.clause.as_deref();
    record
        .inputs
        .iter()
        .filter_map(|(name, was)| {
            let now = match name.as_str() {
                "model" => anchor_hash(index, req, clause, Role::Models),
                "implementation" => anchor_hash(index, req, clause, Role::Implements),
                // The requirement a judgement was about. Without this arm the
                // name resolved to nothing, and a missing input counts as
                // changed — so every judgement went stale the instant it was
                // written and no L2 record ever reached the lock. The hash is
                // of the record's own clause, matching what `judge::record`
                // stored from `material::assemble`.
                "requirement" => index.requirements.get(req).and_then(|r| r.clause_hash(clause)),
                // The toolchain is not in the tree; it is what it was when the
                // record was written, and a different one writes a new record.
                "toolchain" => Some(was.clone()),
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

    /// A stale "agreed" half and a fresh coverage half do not make a record:
    /// the run that agreed was a run against other code.
    ///
    /// @tests REQ-STALE.no_silent_revalidation
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

    /// A judgement recorded against the tree as it is survives that same tree.
    ///
    /// It did not. Two inputs a judgement names — the requirement, and the model
    /// when a clause has more than one — were recomputed here as nothing and as
    /// a different string, and a missing or differing input counts as changed.
    /// So every requirement↔model record went stale the instant it was written
    /// and no L2 evidence ever reached a lock file. Nothing reported it: a
    /// record dropped for going stale and a record never earned look the same
    /// from outside, which is the failure `Collected::dropped` exists to name
    /// and which nothing was reading.
    ///
    /// Two models, deliberately: one model hid the second half of the bug.
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

    /// And it does go stale when either half of what was judged moves.
    ///
    /// The other side of the rule above: the fix must not be "call it fresh",
    /// which would keep a judgement standing over a model nobody judged. The
    /// second model is the one edited, because naming only the first is exactly
    /// how the record survived a change to the other.
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

    /// A requirement reworded re-opens what was earned against it: the tests
    /// of its code against its model, and the proofs about that model.
    ///
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

    /// Rewording clause `a` re-opens `a`'s judgement and leaves `b`'s standing:
    /// the hash a record rests on is its own clause's, not the requirement's.
    ///
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

    /// Running twice does not make two records, and the second answer wins.
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

    /// A record survives while what it depended on holds still, and not after.
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
