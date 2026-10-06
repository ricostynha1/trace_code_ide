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
    pub note: Option<String>,
    pub requirement_hash: String,
    pub model_hash: String,
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
    /// Evidence, at the judgement level and no higher.
    Recorded { evidence: Box<Evidence> },
    /// Drift: no evidence, and the checker will report it from the record.
    Drifted,
    /// A proposal for a person to act on.
    Proposed { proposal: Proposal },
}

pub const PROMPT_VERSION: &str = "1";

/// Record a judgement.
///
/// A judgement writes evidence at the judgement level and never promotes a link
/// further: a reading is not an execution, and the most fallible bond in the
/// system must not be able to produce its most confident output.
///
/// @implements REQ-JUDGE.caps_at_judgement
/// @drt REQ-JUDGE.caps_at_judgement
/// @drt REQ-JUDGE.human_decides
/// @drt REQ-JUDGE.proposal_not_mutation
/// @implements REQ-JUDGE.no_call
/// @implements REQ-JUDGE.advice_is_not_evidence
pub fn record(material: Material, judgement: Judgement, link_hash: String) -> Outcome {
    match judgement.verdict {
        Verdict::Agrees => Outcome::Recorded { evidence: Box::new(Evidence {
            key: Key {
                req_id: material.req_id,
                clause: material.clause,
                bond: Bond::RequirementModel,
            },
            // The ceiling is enforced by the record itself; stating it here too
            // would be a second place for the rule to be wrong.
            level: Level::L2,
            detail: Detail::Judge {
                verdict: "agrees".into(),
                judged_by: judgement.judged_by,
                prompt_version: PROMPT_VERSION.into(),
            },
            link_hash,
            // What the judgement was about. Changing either half means nobody
            // has judged the pair that now exists.
            inputs: vec![
                ("requirement".into(), judgement.requirement_hash),
                ("model".into(), judgement.model_hash),
            ],
        }) },
        Verdict::Drift => Outcome::Drifted,
        Verdict::Unmodelable => Outcome::Proposed { proposal: Proposal {
            req_id: material.req_id,
            clause: material.clause,
            suggestion: judgement
                .note
                .unwrap_or_else(|| "the clause cannot be modelled as written".into()),
        } },
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
            note: None,
            requirement_hash: "rh1".into(),
            model_hash: "mh1".into(),
        }
    }

    /// @tests REQ-JUDGE.human_decides
    /// @tests REQ-JUDGE.caps_at_judgement
    #[test]
    fn agreement_records_a_judgement_and_nothing_more() {
        match record(material(), judgement(Verdict::Agrees), "link1".into()) {
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
            record(material(), judgement(Verdict::Agrees), "link1".into())
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
        match record(material(), j, "link1".into()) {
            Outcome::Proposed { proposal } => {
                assert_eq!(proposal.req_id, "REQ-EVID");
                assert_eq!(proposal.suggestion, "the clause names two behaviours");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn drift_writes_no_evidence() {
        assert_eq!(record(material(), judgement(Verdict::Drift), "l".into()), Outcome::Drifted);
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
