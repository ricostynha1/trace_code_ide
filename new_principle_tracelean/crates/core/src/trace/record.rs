//! Evidence records and their invalidation.
//!
//! This is the soundness core. Everything else decides what a claim is worth;
//! this decides when it stops being worth it, and getting it wrong is worse
//! than having no tool — evidence would be displayed for code that has changed.

use serde::{Deserialize, Serialize};

use crate::evidence::{Bond, Level};

/// What a record is about: one clause of one requirement, on one bond.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub req_id: String,
    pub clause: Option<String>,
    pub bond: Bond,
}

/// Backend-specific detail, typed rather than free-form so a record cannot
/// quietly omit what makes it reproducible.
///
/// @implements REQ-EVID.record_reproducible
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Detail {
    /// A person, or a delegate a person authorised, judged requirement against
    /// model: `agrees` at L2, `drift` or `unmodelable` at L1.
    Judge {
        verdict: String,
        /// Who decided. A judgement is attributable or it is not a judgement.
        ///
        /// Spelled `judgedBy` rather than `by` because the model's language
        /// reserves the word, and a name escaped there cannot be read by the
        /// grammar this project parses Lean with (ADR-0008).
        ///
        /// @implements REQ-JUDGE.human_decides
        judged_by: String,
        /// The listed person who authorised `judged_by` to judge, when the
        /// judge is not a person themselves (ADR-0015). Absent in records made
        /// before delegation existed, which still load.
        ///
        /// @implements REQ-JUDGE.human_decides
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delegated_by: Option<String>,
        prompt_version: String,
        /// Why, in the judge's words: what a drift differs on.
        ///
        /// @implements REQ-JUDGE.drift_recorded
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Differential testing ran and found no disagreement.
    Drt { seed: u64, cases: u64, op: String },
    /// A Lean theorem discharged the property.
    Proof { theorem_name: String, toolchain: String },
}

impl Detail {
    /// The highest level this kind of evidence can establish.
    ///
    /// A judgement is a reading, not an execution: it can never promote a link
    /// past the level its method supports, which is what stops the most
    /// fallible bond producing the most confident output.
    ///
    /// @implements REQ-EVID.judgement_caps
    /// @implements REQ-JUDGE.caps_at_judgement
    pub fn ceiling(&self) -> Level {
        match self {
            Detail::Judge { .. } => Level::L2,
            Detail::Drt { .. } => Level::L3,
            Detail::Proof { .. } => Level::L4,
        }
    }
}

/// `Detail::ceiling`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-EVID.judgement_caps
/// Named `detail_ceiling` rather than `ceiling` because `Detail` already has a
/// `ceiling` method, and a binding resolves a symbol by name.
///
/// @drt REQ-EVID.judgement_caps
pub fn detail_ceiling(detail: Detail) -> Level {
    detail.ceiling()
}

/// What is wrong with a record, if anything.
///
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Reproducibility {
    Reproducible,
    /// A field the method needs is empty.
    Incomplete { field: String },
    /// An input the method depends on is not named.
    MissingInput { name: String },
}

/// The inputs a record of this kind must name.
///
/// A judgement depends on the requirement text and the model; a differential
/// run on the model and the implementation; a proof on the model and the
/// toolchain that checked it. Each is something that can change underneath the
/// record, and something that changes and is not named is a record that stays
/// valid through a change it should not have survived.
///
/// @implements REQ-STALE.inputs_identified
pub fn required_inputs(detail: &Detail) -> &'static [&'static str] {
    match detail {
        Detail::Judge { .. } => &["requirement", "model"],
        Detail::Drt { .. } => &["model", "implementation"],
        Detail::Proof { .. } => &["model", "toolchain"],
    }
}

/// Whether a record carries what is needed to reproduce and to invalidate it.
///
/// The type does most of the work — a differential record has nowhere to put a
/// seed except the `seed` field — but a type cannot stop an empty string, and
/// an empty theorem name is a record nobody can check.
///
/// The first fault is the one reported, checking the method's own fields before
/// its inputs: a record whose method is not even complete is not worth asking
/// about dependencies.
///
/// @implements REQ-EVID.record_reproducible
/// @implements REQ-STALE.inputs_identified
/// @drt REQ-EVID.record_reproducible
/// @drt REQ-STALE.inputs_identified
pub fn reproducibility(record: Evidence) -> Reproducibility {
    let missing_field = match &record.detail {
        Detail::Judge { verdict, judged_by, prompt_version, .. } => {
            if verdict.is_empty() {
                Some("verdict")
            } else if judged_by.is_empty() {
                Some("judgedBy")
            } else if prompt_version.is_empty() {
                Some("promptVersion")
            } else {
                None
            }
        }
        Detail::Drt { cases, op, .. } => {
            if *cases == 0 {
                Some("cases")
            } else if op.is_empty() {
                Some("op")
            } else {
                None
            }
        }
        Detail::Proof { theorem_name, toolchain } => {
            if theorem_name.is_empty() {
                Some("theoremName")
            } else if toolchain.is_empty() {
                Some("toolchain")
            } else {
                None
            }
        }
    };
    if let Some(field) = missing_field {
        return Reproducibility::Incomplete { field: field.to_string() };
    }
    if record.link_hash.is_empty() {
        return Reproducibility::Incomplete { field: "linkHash".to_string() };
    }
    match required_inputs(&record.detail)
        .iter()
        .find(|name| !record.inputs.iter().any(|(had, _)| had == *name))
    {
        Some(name) => Reproducibility::MissingInput { name: (*name).to_string() },
        None => Reproducibility::Reproducible,
    }
}

/// One piece of evidence, and everything it depended on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub key: Key,
    pub level: Level,
    pub detail: Detail,
    /// Identity of the link this was established for. Evidence keyed on it
    /// cannot be inherited by a link that has been retargeted.
    ///
    /// @implements REQ-STALE.retarget_invalidates
    pub link_hash: String,
    /// Every input this depended on, as `(name, hash)`, sorted.
    ///
    /// @implements REQ-STALE.inputs_identified
    pub inputs: Vec<(String, String)>,
}

impl Evidence {
    /// The level this record actually establishes: never above what its backend
    /// can support, whatever it claims.
    pub fn effective_level(&self) -> Level {
        self.level.min(self.detail.ceiling())
    }
}

/// Why a record no longer applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Staleness {
    /// An input changed.
    InputChanged { name: String },
    /// The link it was established for is gone or points elsewhere.
    LinkRetargeted,
}

/// Whether a record still applies, given the current state of the world.
///
/// `current` maps input names to their hashes now. A missing input counts as
/// changed: absence is not evidence that nothing moved.
///
/// @implements REQ-STALE.change_invalidates
pub fn staleness(
    record: &Evidence,
    live_link_hashes: &[String],
    current: &[(String, String)],
) -> Option<Staleness> {
    if !live_link_hashes.iter().any(|h| *h == record.link_hash) {
        return Some(Staleness::LinkRetargeted);
    }
    for (name, was) in &record.inputs {
        let now = current.iter().find(|(n, _)| n == name).map(|(_, h)| h);
        if now != Some(was) {
            return Some(Staleness::InputChanged { name: name.clone() });
        }
    }
    None
}

/// What staleness depends on, and nothing else.
///
/// The model is about this shape rather than the full record: the rest of an
/// evidence record is what it established, not what keeps it true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StalenessInput {
    #[serde(rename = "linkHash")]
    pub link_hash: String,
    pub inputs: Vec<(String, String)>,
}

/// `staleness`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-STALE.change_invalidates
/// @implements REQ-STALE.retarget_invalidates
/// @drt REQ-STALE.change_invalidates
/// @drt REQ-STALE.retarget_invalidates
pub fn staleness_of(
    record: StalenessInput,
    live_link_hashes: Vec<String>,
    current: Vec<(String, String)>,
) -> Option<Staleness> {
    if !live_link_hashes.iter().any(|h| *h == record.link_hash) {
        return Some(Staleness::LinkRetargeted);
    }
    record
        .inputs
        .iter()
        .find(|(name, was)| {
            current.iter().find(|(n, _)| n == name).map(|(_, h)| h) != Some(was)
        })
        .map(|(name, _)| Staleness::InputChanged { name: name.clone() })
}

/// The two sides of a sweep, in the order the records were given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sweep {
    /// Records that still apply.
    pub valid: Vec<StalenessInput>,
    /// Records that no longer apply, with why.
    pub stale: Vec<(StalenessInput, Staleness)>,
}

/// Partition records into those that still apply and those that do not.
///
/// Every record comes back, unchanged, on one side or the other. A scanner that
/// could rewrite a record could write one that was never earned; a scanner that
/// omitted stale records would leave a clause looking untested rather than
/// looking like a claim that has expired, and those call for different actions.
///
/// @implements REQ-STALE.scanner_never_writes
/// @implements REQ-STALE.stale_is_visible
/// @drt REQ-STALE.scanner_never_writes
/// @drt REQ-STALE.stale_is_visible
pub fn sweep(
    records: Vec<StalenessInput>,
    live_link_hashes: Vec<String>,
    current: Vec<(String, String)>,
) -> Sweep {
    let mut out = Sweep::default();
    for record in records {
        match staleness_of(record.clone(), live_link_hashes.clone(), current.clone()) {
            None => out.valid.push(record),
            Some(why) => out.stale.push((record, why)),
        }
    }
    out
}

/// The records that replace stale ones: newly produced, never resurrected.
///
/// A stale record is not repaired, re-judged or quietly forgiven. The only
/// thing that makes a key valid again is the backend that owns it producing a
/// new record — which is why this is a filter over what was produced and never
/// touches what was stale.
///
/// @implements REQ-STALE.no_silent_revalidation
/// @drt REQ-STALE.no_silent_revalidation
pub fn revalidated(
    stale: Vec<StalenessInput>,
    produced: Vec<StalenessInput>,
) -> Vec<StalenessInput> {
    produced
        .into_iter()
        .filter(|fresh| stale.iter().any(|old| old.link_hash == fresh.link_hash))
        .collect()
}

/// Records that still apply, in the order given.
///
/// The scanner marks records stale; it never rewrites, regenerates or deletes
/// one. A scanner that could regenerate a record could regenerate one that was
/// never earned.
///
/// @implements REQ-STALE.scanner_never_writes
/// @implements REQ-STALE.stale_is_visible
pub fn partition<'a>(
    records: &'a [Evidence],
    live_link_hashes: &[String],
    current: &[(String, String)],
) -> (Vec<&'a Evidence>, Vec<(&'a Evidence, Staleness)>) {
    let mut valid = Vec::new();
    let mut stale = Vec::new();
    for record in records {
        match staleness(record, live_link_hashes, current) {
            None => valid.push(record),
            Some(why) => stale.push((record, why)),
        }
    }
    (valid, stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(inputs: &[(&str, &str)]) -> Evidence {
        Evidence {
            key: Key { req_id: "REQ-X".into(), clause: Some("c".into()), bond: Bond::ModelImpl },
            level: Level::L3,
            detail: Detail::Drt { seed: 7, cases: 2_000, op: "REQ-X.c".into() },
            link_hash: "link1".into(),
            inputs: inputs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        }
    }

    fn now(inputs: &[(&str, &str)]) -> Vec<(String, String)> {
        inputs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    /// @tests REQ-STALE.change_invalidates
    #[test]
    fn a_changed_input_invalidates() {
        let r = record(&[("model", "h1"), ("code", "h2")]);
        assert_eq!(staleness(&r, &["link1".into()], &now(&[("model", "h1"), ("code", "h2")])), None);
        assert_eq!(
            staleness(&r, &["link1".into()], &now(&[("model", "h1"), ("code", "CHANGED")])),
            Some(Staleness::InputChanged { name: "code".into() })
        );
    }

    /// Absence is not evidence that nothing moved.
    ///
    /// @tests REQ-STALE.change_invalidates
    #[test]
    fn a_missing_input_counts_as_changed() {
        let r = record(&[("model", "h1")]);
        assert!(staleness(&r, &["link1".into()], &now(&[])).is_some());
    }

    /// @tests REQ-STALE.retarget_invalidates
    #[test]
    fn evidence_is_not_inherited_by_a_retargeted_link() {
        let r = record(&[("model", "h1")]);
        assert_eq!(
            staleness(&r, &["link2".into()], &now(&[("model", "h1")])),
            Some(Staleness::LinkRetargeted)
        );
    }

    /// @tests REQ-STALE.stale_is_visible
    #[test]
    fn stale_records_are_reported_not_dropped() {
        let good = record(&[("model", "h1")]);
        let mut bad = record(&[("model", "h1")]);
        bad.link_hash = "gone".into();
        let records = [good, bad];
        let (valid, stale) =
            partition(&records, &["link1".into()], &now(&[("model", "h1")]));
        assert_eq!(valid.len(), 1);
        assert_eq!(stale.len(), 1);
    }

    /// @tests REQ-EVID.judgement_caps
    #[test]
    fn a_judgement_cannot_claim_more_than_judgement() {
        let mut r = record(&[]);
        r.level = Level::L4;
        r.detail = Detail::Judge {
            verdict: "agrees".into(),
            judged_by: "ana".into(),
            delegated_by: None,
            prompt_version: "1".into(),
            note: None,
        };
        assert_eq!(r.effective_level(), Level::L2);
    }

    #[test]
    fn a_proof_may_claim_the_top() {
        let mut r = record(&[]);
        r.level = Level::L4;
        r.detail = Detail::Proof {
            theorem_name: "assurance_le_bond".into(),
            toolchain: "leanprover/lean4:v4.12.0".into(),
        };
        assert_eq!(r.effective_level(), Level::L4);
    }
}
