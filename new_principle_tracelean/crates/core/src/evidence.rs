//! Evidence algebra: what a claim is worth, and how per-bond worth combines.
//!
//! The wire encoding mirrors what Lean's `deriving ToJson` produces for the
//! same datatypes — a nullary constructor is a bare string — so both sides of a
//! differential test exchange the same shape without either describing it twice.

use serde::{Deserialize, Serialize};

/// @implements REQ-EVID.ladder
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    L1,
    L2,
    L3,
    L4,
}

/// @implements REQ-EVID.bonds_separate
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Bond {
    /// Requirement text against the model. Judged by a person.
    RequirementModel,
    /// Model against the implementation. Checked by differential testing.
    ModelImpl,
    /// A property of the model, proved in Lean.
    ModelProof,
}

/// One evidence record: a level established on one bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub bond: Bond,
    pub level: Level,
}

/// The three bonds in a fixed order, so folding over them is deterministic.
///
/// @implements ARCH-DETERMINISM.stable_ordering
pub const ALL_BONDS: [Bond; 3] = [Bond::RequirementModel, Bond::ModelImpl, Bond::ModelProof];

/// The level established on one bond: the best record for it, or `L1` when
/// there is none.
///
/// A bond with no record is not absent from the aggregate — it contributes the
/// lowest level, which is what makes an unchecked bond visible.
///
/// @implements REQ-EVID.absent_is_lowest
pub fn bond_level(records: &[Record], bond: Bond) -> Level {
    records
        .iter()
        .filter(|r| r.bond == bond)
        .map(|r| r.level)
        .fold(Level::L1, Level::max)
}

/// A link's assurance: the minimum over the three bonds.
///
/// @implements REQ-EVID.weakest_link
/// @implements REQ-EVID.monotone
/// @drt REQ-EVID.weakest_link
/// @drt REQ-EVID.monotone
/// @drt REQ-EVID.absent_is_lowest
/// @drt REQ-EVID.bonds_separate
/// @drt REQ-EVID.ladder
pub fn assurance(records: Vec<Record>) -> Level {
    ALL_BONDS
        .iter()
        .fold(Level::L4, |acc, &b| acc.min(bond_level(&records, b)))
}

/// The per-bond chain, in `ALL_BONDS` order — the information the single value
/// collapses, kept presentable.
///
/// @implements REQ-EVID.chain_rendered
/// @drt REQ-EVID.chain_rendered
pub fn chain(records: Vec<Record>) -> Vec<Level> {
    ALL_BONDS.iter().map(|&b| bond_level(&records, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(bond: Bond, level: Level) -> Record {
        Record { bond, level }
    }

    /// @tests REQ-EVID.weakest_link
    #[test]
    fn proved_model_with_unbound_code_is_not_three_quarters_assured() {
        let records = vec![r(Bond::ModelProof, Level::L4)];
        assert_eq!(assurance(records.clone()), Level::L1);
        assert_eq!(chain(records), vec![Level::L1, Level::L1, Level::L4]);
    }

    /// The levels are a total order, and a strict one: no two are equal, and
    /// sorting them gives the ladder the requirement names.
    ///
    /// Adding a record never lowers an assurance.
    ///
    /// What keeps the algebra sane under incremental work: somebody proving a
    /// theorem, or a differential run finishing, can only move a figure up, so
    /// nobody has to reason about evidence that made things worse.
    ///
    /// @tests REQ-EVID.monotone
    #[test]
    fn adding_a_record_never_lowers_an_assurance() {
        let every = [Bond::RequirementModel, Bond::ModelImpl, Bond::ModelProof];
        let levels = [Level::L1, Level::L2, Level::L3, Level::L4];
        // Every starting set of up to two records, against every record that
        // could be added next.
        let mut starts: Vec<Vec<Record>> = vec![vec![]];
        for bond in every {
            for level in levels {
                starts.push(vec![r(bond, level)]);
                for other in every {
                    starts.push(vec![r(bond, level), r(other, Level::L2)]);
                }
            }
        }
        for start in &starts {
            let before = assurance(start.clone());
            for bond in every {
                for level in levels {
                    let mut after = start.clone();
                    after.push(r(bond, level));
                    assert!(
                        assurance(after.clone()) >= before,
                        "adding {bond:?} at {level:?} to {start:?} lowered the assurance"
                    );
                }
            }
        }
    }

    /// @tests REQ-EVID.ladder
    #[test]
    fn the_levels_climb_and_no_two_are_the_same_rung() {
        let mut levels = vec![Level::L3, Level::L1, Level::L4, Level::L2];
        levels.sort();
        assert_eq!(levels, vec![Level::L1, Level::L2, Level::L3, Level::L4]);
        assert!(Level::L1 < Level::L2 && Level::L2 < Level::L3 && Level::L3 < Level::L4);
        // Annotation is below judgement is below differential testing is below
        // proof, and the minimum in `assurance` is a minimum of *this* order.
        assert_eq!(levels.iter().copied().fold(Level::L4, Level::min), Level::L1);
    }

    /// Evidence on one bond does not move another. This is the clause that
    /// stops a proof from paying for an untested implementation, and it is a
    /// property of `bond_level` rather than of any one call.
    ///
    /// @tests REQ-EVID.bonds_separate
    #[test]
    fn evidence_on_one_bond_does_not_raise_another() {
        let base = vec![r(Bond::ModelImpl, Level::L3)];
        for bond in ALL_BONDS {
            let mut records = base.clone();
            records.push(r(bond, Level::L4));
            for other in ALL_BONDS {
                if other == bond {
                    continue;
                }
                assert_eq!(
                    bond_level(&records, other),
                    bond_level(&base, other),
                    "L4 on {bond:?} changed what {other:?} is worth"
                );
            }
        }
    }

    /// @tests REQ-EVID.absent_is_lowest
    #[test]
    fn no_records_is_lowest_everywhere() {
        assert_eq!(assurance(vec![]), Level::L1);
        assert_eq!(chain(vec![]), vec![Level::L1; 3]);
    }

    /// @tests REQ-EVID.monotone
    #[test]
    fn adding_a_record_never_lowers_assurance() {
        let base = vec![r(Bond::RequirementModel, Level::L2), r(Bond::ModelImpl, Level::L3)];
        let before = assurance(base.clone());
        for bond in ALL_BONDS {
            for level in [Level::L1, Level::L2, Level::L3, Level::L4] {
                let mut more = base.clone();
                more.push(r(bond, level));
                assert!(assurance(more) >= before);
            }
        }
    }

    /// The whole point: a full chain is only as good as its weakest bond.
    #[test]
    fn assurance_is_the_minimum_bond() {
        let records = vec![
            r(Bond::RequirementModel, Level::L2),
            r(Bond::ModelImpl, Level::L3),
            r(Bond::ModelProof, Level::L4),
        ];
        assert_eq!(assurance(records), Level::L2);
    }

    /// Wire shape must match what Lean's derived encoding produces.
    #[test]
    fn encodes_the_way_the_model_does() {
        assert_eq!(serde_json::to_string(&Level::L4).unwrap(), "\"L4\"");
        assert_eq!(serde_json::to_string(&Bond::ModelImpl).unwrap(), "\"modelImpl\"");
        let rec = r(Bond::ModelProof, Level::L1);
        assert_eq!(
            serde_json::to_string(&rec).unwrap(),
            "{\"bond\":\"modelProof\",\"level\":\"L1\"}"
        );
    }
}
