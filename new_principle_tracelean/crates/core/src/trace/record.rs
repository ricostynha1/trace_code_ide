use serde::{Deserialize, Serialize};

use crate::evidence::{Bond, Level};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub req_id: String,
    pub clause: Option<String>,
    pub bond: Bond,
}

/// @implements REQ-EVID.record_reproducible
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Detail {
    Judge {
        verdict: String,
        /// @implements REQ-JUDGE.human_decides
        judged_by: String,
        /// @implements REQ-JUDGE.human_decides
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delegated_by: Option<String>,
        prompt_version: String,
        /// @implements REQ-JUDGE.drift_recorded
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Drt { seed: u64, cases: u64, op: String },
    Proof { theorem_name: String, toolchain: String },
}

impl Detail {
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

/// @implements REQ-EVID.judgement_caps
/// @drt REQ-EVID.judgement_caps
pub fn detail_ceiling(detail: Detail) -> Level {
    detail.ceiling()
}

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Reproducibility {
    Reproducible,
    Incomplete { field: String },
    MissingInput { name: String },
}

/// @implements REQ-STALE.inputs_identified
pub fn required_inputs(detail: &Detail) -> &'static [&'static str] {
    match detail {
        Detail::Judge { .. } => &["requirement", "model"],
        Detail::Drt { .. } => &["model", "implementation"],
        Detail::Proof { .. } => &["model", "toolchain"],
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub key: Key,
    pub level: Level,
    pub detail: Detail,
    /// @implements REQ-STALE.retarget_invalidates
    pub link_hash: String,
    /// @implements REQ-STALE.inputs_identified
    pub inputs: Vec<(String, String)>,
}

impl Evidence {
    pub fn effective_level(&self) -> Level {
        self.level.min(self.detail.ceiling())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Staleness {
    InputChanged { name: String },
    LinkRetargeted,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StalenessInput {
    #[serde(rename = "linkHash")]
    pub link_hash: String,
    pub inputs: Vec<(String, String)>,
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sweep {
    pub valid: Vec<StalenessInput>,
    pub stale: Vec<(StalenessInput, Staleness)>,
}

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

    /// @tests REQ-STALE.scanner_never_writes
    /// @tests REQ-STALE.stale_is_visible
    #[test]
    fn a_sweep_returns_every_record_on_its_side() {
        let input = |link: &str, hash: &str| StalenessInput { link_hash: link.into(), inputs: now(&[("model", hash)]) };
        let swept = sweep(
            vec![input("l1", "h1"), input("l1", "old"), input("gone", "h1")],
            vec!["l1".into()],
            now(&[("model", "h1")]),
        );
        assert_eq!(swept.valid, vec![input("l1", "h1")]);
        assert_eq!(
            swept.stale,
            vec![
                (input("l1", "old"), Staleness::InputChanged { name: "model".into() }),
                (input("gone", "h1"), Staleness::LinkRetargeted),
            ]
        );
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
