//! What a passing run is worth, given what its cases reached.
//!
//! A differential run that finds no disagreement says nothing on its own. It
//! says something once you know *which situations the cases reached*: a law
//! about deletions, checked over two thousand cases none of which deleted
//! anything, is satisfied and vacuous, and reporting that as evidence is the
//! most comfortable lie this system could tell.
//!
//! So a binding states a floor — the situations its runs must reach, and how
//! often — and a run is judged against it. Everything here is a decision from
//! data to data; counting the situations is the caller's job, because only the
//! caller knows what a situation is.

use serde::{Deserialize, Serialize};

use crate::evidence::Level;

/// A situation a binding's runs must reach, and how often.
///
/// A count rather than a fraction: the number of cases is already recorded
/// alongside, and an integer floor reads the same in a diff a year later.
///
/// @implements REQ-DRT-COVER.floor_stated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Floor {
    pub situation: String,
    pub at_least: u64,
}

/// How often a run actually reached a situation.
///
/// @implements REQ-DRT-COVER.law_coverage
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub situation: String,
    pub reached: u64,
}

/// What the coverage of a run amounts to.
///
/// Four answers, and the distinction between the last two is the point.
/// *Vacuous* means no case reached the situation at all, so the law was never
/// asked its question; *short* means it was asked, just not often enough to be
/// convincing. They call for different work — a generator that cannot produce
/// the case, against one that produces it rarely.
///
/// @implements ARCH-HONEST.named_findings
/// @implements REQ-DRT-COVER.vacuous_named
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Verdict {
    /// Every declared floor was reached.
    Met,
    /// A declared situation no case reached.
    Vacuous { situation: String },
    /// A situation reached, but under its floor.
    Short { situation: String, reached: u64, at_least: u64 },
    /// The binding states no floor, so nothing is known about what the run
    /// reached. Not the same as meeting a floor of zero: one is a claim
    /// somebody made, the other is the absence of one.
    Undeclared,
}

/// How often a run reached one situation, or zero.
fn reached(observed: &[Observed], situation: &str) -> u64 {
    observed
        .iter()
        .find(|o| o.situation == situation)
        .map(|o| o.reached)
        .unwrap_or(0)
}

/// Judge a run's coverage against the floors its binding declared.
///
/// The first floor that fails is the one reported, in declaration order, so
/// that the message is stable between runs and a person fixing them has an
/// order to work in.
///
/// @implements REQ-DRT-COVER.floor_stated
/// @implements REQ-DRT-COVER.law_coverage
/// @implements REQ-DRT-COVER.vacuous_named
/// @drt REQ-DRT-COVER.floor_stated
/// @drt REQ-DRT-COVER.law_coverage
/// @drt REQ-DRT-COVER.vacuous_named
pub fn verdict(floors: Vec<Floor>, observed: Vec<Observed>) -> Verdict {
    if floors.is_empty() {
        return Verdict::Undeclared;
    }
    for floor in &floors {
        let count = reached(&observed, &floor.situation);
        if count == 0 && floor.at_least > 0 {
            return Verdict::Vacuous { situation: floor.situation.clone() };
        }
        if count < floor.at_least {
            return Verdict::Short {
                situation: floor.situation.clone(),
                reached: count,
                at_least: floor.at_least,
            };
        }
    }
    Verdict::Met
}

/// What a run establishes, given whether it agreed and what it covered.
///
/// L3 needs both. A run that agreed but did not reach its floor has not shown
/// what the floor exists to make it show, and reporting it as L3 would put the
/// most comfortable answer at the highest rung this system can reach without a
/// proof.
///
/// @implements REQ-DRT-COVER.floor_unmet_is_not_pass
/// @implements REQ-DRT.falsification_only
/// @drt REQ-DRT-COVER.floor_unmet_is_not_pass
pub fn level(agreed: bool, verdict: Verdict) -> Level {
    match (agreed, verdict) {
        (true, Verdict::Met) => Level::L3,
        _ => Level::L1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(situation: &str, at_least: u64) -> Floor {
        Floor { situation: situation.into(), at_least }
    }

    fn observed(situation: &str, reached: u64) -> Observed {
        Observed { situation: situation.into(), reached }
    }

    /// @tests REQ-DRT-COVER.vacuous_named
    #[test]
    fn a_law_nothing_reached_is_vacuous_and_not_short() {
        assert_eq!(
            verdict(vec![floor("deletes a file", 10)], vec![]),
            Verdict::Vacuous { situation: "deletes a file".into() }
        );
        assert_eq!(
            verdict(vec![floor("deletes a file", 10)], vec![observed("deletes a file", 3)]),
            Verdict::Short { situation: "deletes a file".into(), reached: 3, at_least: 10 }
        );
    }

    /// @tests REQ-DRT-COVER.floor_stated
    #[test]
    fn stating_no_floor_is_not_the_same_as_meeting_one() {
        assert_eq!(verdict(vec![], vec![observed("anything", 900)]), Verdict::Undeclared);
        assert_eq!(verdict(vec![floor("anything", 0)], vec![]), Verdict::Met);
    }

    /// @tests REQ-DRT-COVER.floor_unmet_is_not_pass
    #[test]
    fn a_run_below_its_floor_is_not_evidence() {
        assert_eq!(level(true, Verdict::Met), Level::L3);
        assert_eq!(level(true, Verdict::Undeclared), Level::L1);
        assert_eq!(level(true, Verdict::Vacuous { situation: "x".into() }), Level::L1);
        assert_eq!(
            level(true, Verdict::Short { situation: "x".into(), reached: 1, at_least: 2 }),
            Level::L1
        );
        assert_eq!(level(false, Verdict::Met), Level::L1);
    }
}
