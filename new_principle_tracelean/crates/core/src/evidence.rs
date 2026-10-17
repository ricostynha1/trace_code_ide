//! Evidence algebra: what a claim is worth, and how per-bond worth combines.
//!
//! The wire encoding mirrors what Lean's `deriving ToJson` produces for the
//! same datatypes — a nullary constructor is a bare string — so both sides of a
//! differential test exchange the same shape without either describing it twice.

use serde::{Deserialize, Serialize};

/// What kind of record a piece of evidence is: a judgement is the second, a
/// differential run the third, a proof the fourth. Ordered, because a record
/// can be capped below what it claims (`judgement_caps`); never ranked across
/// bonds, which is what `Marks` is for.
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
/// A bond with no record reads `L1`, which sets no mark: an unchecked bond is
/// a gap, not a zero averaged away.
///
/// @implements REQ-EVID.absent_is_unset
pub fn bond_level(records: &[Record], bond: Bond) -> Level {
    records
        .iter()
        .filter(|r| r.bond == bond)
        .map(|r| r.level)
        .fold(Level::L1, Level::max)
}

/// What evidence a clause holds, each kind on its own: a model can be proved
/// and tested before anybody judged it, and a ladder would read that as
/// "annotated only".
///
/// @implements REQ-EVID.marks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Marks {
    pub annotated: bool,
    pub judged: bool,
    pub tested: bool,
    pub proved: bool,
    /// Checked by reading the tree rather than against a model (ADR-0012), so
    /// there is nothing to test or prove: those two are written `·`, not `-`,
    /// because nothing is missing. Never held alongside them.
    pub structural: bool,
}

/// Which of the four marks a position of `AJTP` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mark {
    Annotated,
    Judged,
    Tested,
    Proved,
}

impl Marks {
    /// Each mark in its position, with whether it is held and its letter.
    pub fn each(&self) -> [(Mark, bool, char); 4] {
        [
            (Mark::Annotated, self.annotated, 'A'),
            (Mark::Judged, self.judged, 'J'),
            (Mark::Tested, self.tested, 'T'),
            (Mark::Proved, self.proved, 'P'),
        ]
    }

    /// No mark at all.
    pub const NONE: Marks =
        Marks { annotated: false, judged: false, tested: false, proved: false, structural: false };

    /// What withholds nothing from what it is joined with: `x.and(NEUTRAL)` is
    /// `x`, with a test or proof that cannot apply dropped. Never a clause's
    /// own marks, only where a join starts.
    pub const NEUTRAL: Marks =
        Marks { annotated: true, judged: true, tested: false, proved: false, structural: true };

    /// Whether a mark can be held at all: testing and proving need a model,
    /// which a structural clause has none of.
    pub fn applies(&self, mark: Mark) -> bool {
        !(self.structural && matches!(mark, Mark::Tested | Mark::Proved))
    }

    /// Every mark `floor` asks for is held here. One that does not apply is
    /// not held: a structural clause never meets a floor asking for a test.
    pub fn holds(&self, floor: Marks) -> bool {
        (self.annotated || !floor.annotated)
            && (self.judged || !floor.judged)
            && (self.tested || !floor.tested)
            && (self.proved || !floor.proved)
    }

    /// The marks both hold. A mark that does not apply to one side leaves the
    /// other's standing, so a structural clause neither grants nor withholds
    /// a test; one that applies to neither side applies to neither together.
    pub fn and(self, other: Marks) -> Marks {
        let structural = self.structural && other.structural;
        let met = |a: bool, b: bool| !structural && (a || self.structural) && (b || other.structural);
        Marks {
            annotated: self.annotated && other.annotated,
            judged: self.judged && other.judged,
            tested: met(self.tested, other.tested),
            proved: met(self.proved, other.proved),
            structural,
        }
    }

    /// The marks as they are written, `AJTP` with gaps.
    pub fn text(&self) -> String {
        written(*self)
    }
}

/// A clause's marks, from whether something claims it and its records: each
/// bond sets its own mark and no other.
///
/// @implements REQ-EVID.mark_per_bond
/// @implements REQ-EVID.monotone
/// @drt REQ-EVID.mark_per_bond
/// @drt REQ-EVID.monotone
/// @drt REQ-EVID.absent_is_unset
/// @drt REQ-EVID.bonds_separate
/// @drt REQ-EVID.marks
pub fn marks(records: Vec<Record>, annotated: bool) -> Marks {
    Marks {
        annotated,
        judged: bond_level(&records, Bond::RequirementModel) >= Level::L2,
        tested: bond_level(&records, Bond::ModelImpl) >= Level::L3,
        proved: bond_level(&records, Bond::ModelProof) >= Level::L4,
        structural: false,
    }
}

/// A structural clause's marks: whatever it holds, testing and proving do not
/// apply to it.
///
/// @implements REQ-EVID.structural_not_applicable
/// @drt REQ-EVID.structural_not_applicable
pub fn structural(marks: Marks) -> Marks {
    Marks { tested: false, proved: false, structural: true, ..marks }
}

/// One position of the written marks: its letter, a dash where it is not
/// held, a dot where it cannot be.
pub fn letter_of(marks: Marks, (mark, held, letter): (Mark, bool, char)) -> char {
    match (marks.applies(mark), held) {
        (false, _) => '·',
        (true, true) => letter,
        (true, false) => '-',
    }
}

/// Marks in fixed positions, a dash where one is not held, so a gap names
/// what is missing: `AJTP`, `A-TP`, `A---`; a dot where one cannot be held,
/// `AJ··`.
///
/// @implements REQ-EVID.marks_written
/// @drt REQ-EVID.marks_written
pub fn written(marks: Marks) -> String {
    marks.each().iter().map(|&each| letter_of(marks, each)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(bond: Bond, level: Level) -> Record {
        Record { bond, level }
    }

    /// A proved model nobody judged or tested reads `A--P`: the proof is not
    /// hidden behind the gaps, and the gaps are not hidden behind the proof.
    ///
    /// @tests REQ-EVID.mark_per_bond
    #[test]
    fn proved_model_with_unbound_code_reads_as_proved_only() {
        let records = vec![r(Bond::ModelProof, Level::L4)];
        assert_eq!(marks(records, true).text(), "A--P");
    }

    /// Each mark is held or not on its own: every one of the sixteen
    /// combinations is reachable, so none implies another.
    ///
    /// @tests REQ-EVID.marks
    #[test]
    fn every_combination_of_marks_is_reachable() {
        let mut seen = std::collections::BTreeSet::new();
        for bits in 0..16u8 {
            let mut records = Vec::new();
            if bits & 2 != 0 {
                records.push(r(Bond::RequirementModel, Level::L2));
            }
            if bits & 4 != 0 {
                records.push(r(Bond::ModelImpl, Level::L3));
            }
            if bits & 8 != 0 {
                records.push(r(Bond::ModelProof, Level::L4));
            }
            seen.insert(marks(records, bits & 1 != 0).text());
        }
        assert_eq!(seen.len(), 16);
    }

    /// @tests REQ-EVID.marks_written
    #[test]
    fn marks_are_written_in_fixed_positions() {
        assert_eq!(written(Marks::NONE), "----");
        assert_eq!(written(Marks { annotated: true, tested: true, proved: true, ..Marks::NONE }), "A-TP");
        assert_eq!(
            written(Marks { annotated: true, judged: true, tested: true, proved: true, structural: false }),
            "AJTP"
        );
        assert_eq!(written(structural(Marks { annotated: true, ..Marks::NONE })), "A-··");
    }

    /// A structural clause can hold neither a test nor a proof, whatever its
    /// records say, and joined with a modelled clause it leaves that clause's
    /// test and proof standing: it neither grants them nor withholds them.
    ///
    /// @tests REQ-EVID.structural_not_applicable
    #[test]
    fn testing_and_proving_do_not_apply_to_a_structural_clause() {
        let full = marks(
            vec![
                r(Bond::RequirementModel, Level::L2),
                r(Bond::ModelImpl, Level::L3),
                r(Bond::ModelProof, Level::L4),
            ],
            true,
        );
        let read = structural(full);
        assert_eq!(read.text(), "AJ··");
        assert!(!read.holds(Marks { tested: true, ..Marks::NONE }));
        let tested = marks(vec![r(Bond::ModelImpl, Level::L3)], true);
        assert_eq!(read.and(tested).text(), "A-T-");
        assert_eq!(tested.and(read).text(), "A-T-");
        assert_eq!(read.and(read).text(), "AJ··");
        assert_eq!(read.and(marks(vec![], true)).text(), "A---");
    }

    /// Adding a record never clears a mark.
    ///
    /// What keeps the algebra sane under incremental work: somebody proving a
    /// theorem, or a differential run finishing, can only add a mark, so
    /// nobody has to reason about evidence that made things worse.
    ///
    /// @tests REQ-EVID.monotone
    #[test]
    fn adding_a_record_never_clears_a_mark() {
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
            let before = marks(start.clone(), true);
            for bond in every {
                for level in levels {
                    let mut after = start.clone();
                    after.push(r(bond, level));
                    assert!(
                        marks(after.clone(), true).holds(before),
                        "adding {bond:?} at {level:?} to {start:?} cleared a mark"
                    );
                }
            }
        }
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

    /// @tests REQ-EVID.absent_is_unset
    #[test]
    fn no_records_sets_no_mark() {
        assert_eq!(marks(vec![], true).text(), "A---");
        assert_eq!(marks(vec![], false), Marks::NONE);
    }

    /// A record below what its bond's mark needs sets nothing: a drift
    /// judgement at L1 is not a judged mark, nor a capped run a tested one.
    #[test]
    fn a_record_below_its_mark_sets_nothing() {
        let records = vec![r(Bond::RequirementModel, Level::L1), r(Bond::ModelImpl, Level::L2)];
        assert_eq!(marks(records, true).text(), "A---");
        let full = vec![
            r(Bond::RequirementModel, Level::L2),
            r(Bond::ModelImpl, Level::L3),
            r(Bond::ModelProof, Level::L4),
        ];
        assert_eq!(marks(full, true).text(), "AJTP");
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
