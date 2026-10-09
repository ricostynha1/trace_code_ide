//! The human judge.
//!
//! Nothing binds English to a formal model by execution — there is no oracle
//! for prose. Every other bond is checked by running something; this one cannot
//! be, which is why it is graded below them and why the decision is a person's.
//!
//! There is no network code in this module, and none anywhere in this project.

use serde::{Deserialize, Serialize};

use crate::evidence::{Bond, Level};
use crate::trace::record::{Detail, Evidence, Key};

/// What a judge decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// The model says what the clause says.
    Agrees,
    /// It does not, and the difference matters.
    Drift,
    /// The clause cannot be modelled as written.
    Unmodelable,
}

/// The material a judgement is made from.
///
/// A divergence found by differential testing is presented alongside, because
/// a model that disagrees with the code is a fact the judge needs before
/// deciding whether it agrees with the requirement.
///
/// @implements REQ-JUDGE.divergence_presented
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub req_id: String,
    pub clause: Option<String>,
    pub clause_text: String,
    pub model_source: String,
    /// What the requirement hashed to when this was assembled.
    pub requirement_hash: String,
    /// What the model hashed to when this was assembled.
    pub model_hash: String,
    /// A disagreement between model and implementation, if one is known.
    pub divergence: Option<String>,
}

/// A decision, attributable to the person who made it.
///
/// @implements REQ-JUDGE.human_decides
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgement {
    pub verdict: Verdict,
    /// Who decided. A judgement is attributable or it is not a judgement.
    pub judged_by: String,
    /// The listed person who authorised `judged_by`, when `judged_by` is a
    /// delegate rather than a person (ADR-0015).
    #[serde(default)]
    pub delegated_by: Option<String>,
    pub note: Option<String>,
    pub requirement_hash: String,
    pub model_hash: String,
}

/// Who may judge on a project: `.tracelean/judges.json`.
///
/// ```json
/// {"people": [{"name": "ana", "delegatesTo": ["claude-review"]}]}
/// ```
///
/// A person judges in their own name. A delegate judges only in the name of a
/// person who lists them, and the record keeps both names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judges {
    pub people: Vec<Person>,
}

/// One person who may judge, and whom they authorise to judge for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    #[serde(default)]
    pub delegates_to: Vec<String>,
}

/// Read a project's judges file.
pub fn parse_judges(text: &str) -> Result<Judges, String> {
    serde_json::from_str(text).map_err(|e| format!("judges.json is not readable: {e}"))
}

/// Whether a judge may record a verdict, and in whose name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Authority {
    /// Accepted; the person who delegated, if anyone did.
    Accepted { delegated_by: Option<String> },
    /// Refused, and what would authorise it.
    Refused { reason: String },
}

/// Lean's `Char.isWhitespace`, so the two sides agree on what a blank name is.
fn blank(name: &str) -> bool {
    name.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

/// Who may judge.
///
/// A blank name is refused whatever the project says. Without a judges file
/// every name is taken at its word (the record is then attributed to a name,
/// not to a listed person). With one, a listed person judges in their own
/// name, and anyone else needs a listed person who delegates to them.
///
/// @implements REQ-JUDGE.human_decides
pub fn authority(judges: &Option<Judges>, judged_by: &str, delegated_by: &Option<String>) -> Authority {
    if blank(judged_by) {
        return Authority::Refused { reason: "a judgement names who made it, and --by is empty".into() };
    }
    let Some(judges) = judges else {
        return Authority::Accepted { delegated_by: delegated_by.clone() };
    };
    if judges.people.iter().any(|p| p.name == judged_by) {
        return Authority::Accepted { delegated_by: None };
    }
    match delegated_by {
        None => Authority::Refused {
            reason: format!(
                "{judged_by} is not a person in .tracelean/judges.json; name the person who \
                 delegates to them with --delegated-by"
            ),
        },
        Some(person) => {
            let delegates = judges
                .people
                .iter()
                .any(|p| &p.name == person && p.delegates_to.iter().any(|d| d == judged_by));
            if delegates {
                Authority::Accepted { delegated_by: Some(person.clone()) }
            } else {
                Authority::Refused {
                    reason: format!(
                        "{person} does not delegate to {judged_by}; add {judged_by} to the \
                         delegatesTo of {person} in .tracelean/judges.json to authorise it"
                    ),
                }
            }
        }
    }
}

/// A change a judgement suggests, which nothing applies automatically.
///
/// `Unmodelable` produces a proposal, never a mutation: the judge is saying the
/// clause cannot be formalised as written, and rewriting somebody's requirement
/// on that basis is not a conclusion a tool gets to draw.
///
/// @implements REQ-JUDGE.proposal_not_mutation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub req_id: String,
    pub clause: Option<String>,
    pub suggestion: String,
}

/// What recording a judgement produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Agreement: evidence at the judgement level and no higher.
    Recorded { evidence: Box<Evidence> },
    /// Drift: recorded at L1, so it is shown and goes stale like any record.
    Drifted { evidence: Box<Evidence> },
    /// Unmodelable: recorded at L1, and a proposal for a person to act on.
    Proposed { evidence: Box<Evidence>, proposal: Proposal },
    /// The judge may not judge here; nothing is recorded.
    Refused { reason: String },
}

pub const PROMPT_VERSION: &str = "1";

/// Record a judgement.
///
/// Every verdict a judge may make is recorded, in the one slot a clause has for
/// its judgement: agreement at the judgement level and never further — a
/// reading is not an execution, and the most fallible bond in the system must
/// not be able to produce its most confident output — and drift or
/// `unmodelable` at L1, which raises nothing but is shown, and goes stale when
/// the clause or the model it was about changes.
///
/// @implements REQ-JUDGE.caps_at_judgement
/// @implements REQ-JUDGE.human_decides
/// @implements REQ-JUDGE.drift_recorded
/// @drt REQ-JUDGE.caps_at_judgement
/// @drt REQ-JUDGE.human_decides
/// @drt REQ-JUDGE.proposal_not_mutation
/// @drt REQ-JUDGE.drift_recorded
/// @implements REQ-JUDGE.no_call
/// @implements REQ-JUDGE.advice_is_not_evidence
pub fn record(
    material: Material,
    judgement: Judgement,
    judges: Option<Judges>,
    link_hash: String,
) -> Outcome {
    let delegated_by = match authority(&judges, &judgement.judged_by, &judgement.delegated_by) {
        Authority::Accepted { delegated_by } => delegated_by,
        Authority::Refused { reason } => return Outcome::Refused { reason },
    };
    // The ceiling is enforced by the record itself; the level here is what the
    // verdict says, never more than a judgement can.
    let (verdict, level) = match judgement.verdict {
        Verdict::Agrees => ("agrees", Level::L2),
        Verdict::Drift => ("drift", Level::L1),
        Verdict::Unmodelable => ("unmodelable", Level::L1),
    };
    let evidence = Box::new(Evidence {
        key: Key {
            req_id: material.req_id.clone(),
            clause: material.clause.clone(),
            bond: Bond::RequirementModel,
        },
        level,
        detail: Detail::Judge {
            verdict: verdict.into(),
            judged_by: judgement.judged_by,
            delegated_by,
            prompt_version: PROMPT_VERSION.into(),
            note: judgement.note.clone(),
        },
        link_hash,
        // What the judgement was about. Changing either half means nobody has
        // judged the pair that now exists.
        inputs: vec![
            ("requirement".into(), judgement.requirement_hash),
            ("model".into(), judgement.model_hash),
        ],
    });
    match judgement.verdict {
        Verdict::Agrees => Outcome::Recorded { evidence },
        Verdict::Drift => Outcome::Drifted { evidence },
        Verdict::Unmodelable => Outcome::Proposed {
            evidence,
            proposal: Proposal {
                req_id: material.req_id,
                clause: material.clause,
                suggestion: judgement
                    .note
                    .unwrap_or_else(|| "the clause cannot be modelled as written".into()),
            },
        },
    }
}

/// Whether a judgement still applies to the material in front of us.
///
/// @implements REQ-JUDGE.invalidated_by_change
pub fn still_applies(judgement: &Judgement, material: &Material) -> bool {
    judgement.requirement_hash == material.requirement_hash
        && judgement.model_hash == material.model_hash
}

/// `still_applies`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-JUDGE.invalidated_by_change
/// @drt REQ-JUDGE.invalidated_by_change
pub fn still_applies_to(judgement: Judgement, material: Material) -> bool {
    still_applies(&judgement, &material)
}

/// The prompt a person may carry to a tool of their choosing.
///
/// Exported, never sent. Whatever comes back is advice, and the decision the
/// person then records is their own.
///
/// @implements REQ-JUDGE.prompt_exported
/// @implements REQ-JUDGE.no_call
/// @implements REQ-JUDGE.divergence_presented
/// @implements REQ-JUDGE.advice_is_not_evidence
/// @drt REQ-JUDGE.prompt_exported
/// @drt REQ-JUDGE.divergence_presented
/// @drt REQ-JUDGE.advice_is_not_evidence
pub fn prompt(material: Material) -> String {
    let clause = match &material.clause {
        Some(clause) => format!("{}.{clause}", material.req_id),
        None => material.req_id.clone(),
    };
    let divergence = match &material.divergence {
        Some(text) => format!(
            "\nDifferential testing found this disagreement between the model and the \
             implementation:\n\n{text}\n"
        ),
        None => String::new(),
    };

    format!(
        "Does this formal model say what this requirement clause says?\n\n\
         Requirement {clause}:\n\n{}\n\nModel:\n\n```lean\n{}\n```\n{divergence}\n\
         Answer with one of: agrees, drift, unmodelable — and, if drift, the concrete \
         behaviour on which the two differ.\n\n\
         (Prompt version {PROMPT_VERSION}. This is advice. The decision is recorded as \
         the judgement of the person who asked.)\n",
        material.clause_text.trim(),
        material.model_source.trim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material() -> Material {
        Material {
            req_id: "REQ-EVID".into(),
            clause: Some("weakest_link".into()),
            clause_text: "A link's assurance shall be the minimum over its bonds.".into(),
            model_source: "def assurance := ...".into(),
            requirement_hash: "rh1".into(),
            model_hash: "mh1".into(),
            divergence: None,
        }
    }

    fn judgement(verdict: Verdict) -> Judgement {
        Judgement {
            verdict,
            judged_by: "ana".into(),
            delegated_by: None,
            note: None,
            requirement_hash: "rh1".into(),
            model_hash: "mh1".into(),
        }
    }

    fn judges() -> Option<Judges> {
        Some(Judges {
            people: vec![Person { name: "ana".into(), delegates_to: vec!["reviewer".into()] }],
        })
    }

    fn delegate_of(judgement: &Outcome) -> Option<String> {
        let (Outcome::Recorded { evidence } | Outcome::Drifted { evidence } | Outcome::Proposed { evidence, .. }) =
            judgement
        else {
            panic!("refused: {judgement:?}")
        };
        match &evidence.detail {
            Detail::Judge { delegated_by, .. } => delegated_by.clone(),
            other => panic!("{other:?}"),
        }
    }

    /// @tests REQ-JUDGE.human_decides
    /// @tests REQ-JUDGE.caps_at_judgement
    #[test]
    fn agreement_records_a_judgement_and_nothing_more() {
        match record(material(), judgement(Verdict::Agrees), judges(), "link1".into()) {
            Outcome::Recorded { evidence } => {
                assert_eq!(evidence.level, Level::L2);
                assert_eq!(evidence.effective_level(), Level::L2);
                assert_eq!(evidence.key.bond, Bond::RequirementModel);
                match &evidence.detail {
                    Detail::Judge { judged_by, .. } => assert_eq!(judged_by, "ana"),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }

    /// Even a judge who claims more gets no more: the ceiling is a property of
    /// the method, not of the claim.
    #[test]
    fn a_judgement_can_never_reach_testing_or_proof() {
        let Outcome::Recorded { mut evidence } =
            record(material(), judgement(Verdict::Agrees), None, "link1".into())
        else {
            panic!("expected a record")
        };
        evidence.level = Level::L4;
        assert_eq!(evidence.effective_level(), Level::L2);
    }

    /// @tests REQ-JUDGE.proposal_not_mutation
    #[test]
    fn unmodelable_produces_a_proposal_and_changes_nothing() {
        let mut j = judgement(Verdict::Unmodelable);
        j.note = Some("the clause names two behaviours".into());
        match record(material(), j, judges(), "link1".into()) {
            Outcome::Proposed { proposal, evidence } => {
                assert_eq!(proposal.req_id, "REQ-EVID");
                assert_eq!(proposal.suggestion, "the clause names two behaviours");
                assert_eq!(evidence.level, Level::L1);
            }
            other => panic!("{other:?}"),
        }
    }

    /// Drift is recorded in the judgement's slot at L1, with the note and the
    /// two hashes it was about, so it shows and goes stale like any record.
    ///
    /// @tests REQ-JUDGE.drift_recorded
    #[test]
    fn drift_is_recorded_at_the_lowest_level_with_its_note() {
        let mut j = judgement(Verdict::Drift);
        j.note = Some("the clause rounds, the model truncates".into());
        let Outcome::Drifted { evidence } = record(material(), j, judges(), "l".into()) else {
            panic!("drift was not recorded")
        };
        assert_eq!(evidence.level, Level::L1);
        assert_eq!(evidence.key.bond, Bond::RequirementModel);
        assert_eq!(
            evidence.inputs,
            vec![("requirement".to_string(), "rh1".to_string()), ("model".to_string(), "mh1".to_string())]
        );
        match &evidence.detail {
            Detail::Judge { verdict, note, .. } => {
                assert_eq!(verdict, "drift");
                assert_eq!(note.as_deref(), Some("the clause rounds, the model truncates"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// @tests REQ-JUDGE.human_decides
    #[test]
    fn a_blank_judge_is_refused_with_or_without_a_judges_file() {
        for by in ["", "  ", "\t\n"] {
            let mut j = judgement(Verdict::Agrees);
            j.judged_by = by.into();
            assert!(matches!(record(material(), j.clone(), None, "l".into()), Outcome::Refused { .. }));
            assert!(matches!(record(material(), j, judges(), "l".into()), Outcome::Refused { .. }));
        }
    }

    /// A delegate counts only in the name of a listed person who delegates to
    /// them, and the record keeps that person's name.
    ///
    /// @tests REQ-JUDGE.human_decides
    #[test]
    fn a_delegate_judges_only_for_a_person_who_delegated_to_them() {
        let as_delegate = |delegated_by: Option<&str>| {
            let mut j = judgement(Verdict::Agrees);
            j.judged_by = "reviewer".into();
            j.delegated_by = delegated_by.map(String::from);
            record(material(), j, judges(), "l".into())
        };
        assert_eq!(delegate_of(&as_delegate(Some("ana"))), Some("ana".into()));
        let Outcome::Refused { reason } = as_delegate(None) else { panic!("an undelegated agent counted") };
        assert!(reason.contains("--delegated-by"), "{reason}");
        let Outcome::Refused { reason } = as_delegate(Some("bob")) else { panic!("an unlisted person delegated") };
        assert!(reason.contains("delegatesTo"), "{reason}");

        // A person judges in their own name; without a judges file the name is
        // taken at its word.
        assert_eq!(delegate_of(&record(material(), judgement(Verdict::Drift), judges(), "l".into())), None);
        let mut j = judgement(Verdict::Agrees);
        j.judged_by = "anyone".into();
        assert!(matches!(record(material(), j, None, "l".into()), Outcome::Recorded { .. }));
    }

    /// A record made before delegation existed still loads, and one without a
    /// delegate or a note writes neither.
    #[test]
    fn an_older_judgement_record_still_loads() {
        let old = r#"{"judge": {"verdict": "agrees", "judgedBy": "ana", "promptVersion": "1"}}"#;
        let detail: Detail = serde_json::from_str(old).unwrap();
        assert!(matches!(&detail, Detail::Judge { delegated_by: None, note: None, .. }));
        assert_eq!(serde_json::to_value(&detail).unwrap(), serde_json::from_str::<serde_json::Value>(old).unwrap());
    }

    #[test]
    fn a_judges_file_reads_with_or_without_delegates() {
        let judges = parse_judges(r#"{"people": [{"name": "ana", "delegatesTo": ["r"]}, {"name": "bo"}]}"#).unwrap();
        assert_eq!(judges.people[0].delegates_to, vec!["r".to_string()]);
        assert!(judges.people[1].delegates_to.is_empty());
    }

    /// @tests REQ-JUDGE.invalidated_by_change
    #[test]
    fn changing_either_half_invalidates_the_judgement() {
        let j = judgement(Verdict::Agrees);
        assert!(still_applies(&j, &material()));

        let mut changed_requirement = material();
        changed_requirement.requirement_hash = "rh2".into();
        assert!(!still_applies(&j, &changed_requirement));

        let mut changed_model = material();
        changed_model.model_hash = "mh2".into();
        assert!(!still_applies(&j, &changed_model));
    }

    /// @tests REQ-JUDGE.prompt_exported
    #[test]
    fn the_prompt_carries_the_material_and_asks_for_a_verdict() {
        let text = prompt(material());
        assert!(text.contains("REQ-EVID.weakest_link"));
        assert!(text.contains("minimum over its bonds"));
        assert!(text.contains("def assurance"));
        assert!(text.contains("agrees, drift, unmodelable"));
    }

    /// @tests REQ-JUDGE.divergence_presented
    #[test]
    fn a_known_divergence_is_put_in_front_of_the_judge() {
        let mut m = material();
        m.divergence = Some("input {} -> model L1, implementation L2".into());
        let text = prompt(m);
        assert!(text.contains("model L1, implementation L2"));
    }
}
