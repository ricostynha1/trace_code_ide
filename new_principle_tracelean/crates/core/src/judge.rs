use serde::{Deserialize, Serialize};

use crate::evidence::{Bond, Level};
use crate::trace::record::{Detail, Evidence, Key};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Agrees,
    Drift,
    Unmodelable,
}

/// @implements REQ-JUDGE.divergence_presented
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub req_id: String,
    pub clause: Option<String>,
    pub clause_text: String,
    pub model_source: String,
    pub requirement_hash: String,
    pub model_hash: String,
    pub divergence: Option<String>,
}

/// @implements REQ-JUDGE.human_decides
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgement {
    pub verdict: Verdict,
    pub judged_by: String,
    #[serde(default)]
    pub delegated_by: Option<String>,
    pub note: Option<String>,
    pub requirement_hash: String,
    pub model_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judges {
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    #[serde(default)]
    pub delegates_to: Vec<String>,
}

pub fn parse_judges(text: &str) -> Result<Judges, String> {
    serde_json::from_str(text).map_err(|e| format!("judges.json is not readable: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Authority {
    Accepted { delegated_by: Option<String> },
    Refused { reason: String },
}

fn blank(name: &str) -> bool {
    name.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

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

/// @implements REQ-JUDGE.proposal_not_mutation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub req_id: String,
    pub clause: Option<String>,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Recorded { evidence: Box<Evidence> },
    Drifted { evidence: Box<Evidence> },
    Proposed { evidence: Box<Evidence>, proposal: Proposal },
    Refused { reason: String },
}

pub const PROMPT_VERSION: &str = "1";

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

/// @implements REQ-JUDGE.invalidated_by_change
pub fn still_applies(judgement: &Judgement, material: &Material) -> bool {
    judgement.requirement_hash == material.requirement_hash
        && judgement.model_hash == material.model_hash
}

/// @implements REQ-JUDGE.invalidated_by_change
/// @drt REQ-JUDGE.invalidated_by_change
pub fn still_applies_to(judgement: Judgement, material: Material) -> bool {
    still_applies(&judgement, &material)
}

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

        assert_eq!(delegate_of(&record(material(), judgement(Verdict::Drift), judges(), "l".into())), None);
        let mut j = judgement(Verdict::Agrees);
        j.judged_by = "anyone".into();
        assert!(matches!(record(material(), j, None, "l".into()), Outcome::Recorded { .. }));
    }

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
