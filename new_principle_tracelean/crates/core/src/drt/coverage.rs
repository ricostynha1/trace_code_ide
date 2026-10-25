use serde::{Deserialize, Serialize};

use crate::evidence::Level;

fn once() -> u64 {
    1
}

/// @implements REQ-DRT-COVER.floor_stated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Floor {
    pub situation: String,
    #[serde(default = "once")]
    pub at_least: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub situation: String,
    pub reached: u64,
}

/// @implements REQ-DRT-COVER.waiver_reasoned
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiver {
    pub situations: Vec<String>,
    pub reason: String,
}

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Gap {
    Vacuous { situation: String },
    Short { situation: String, reached: u64, at_least: u64 },
}

impl Gap {
    pub fn situation(&self) -> &str {
        match self {
            Gap::Vacuous { situation } | Gap::Short { situation, .. } => situation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Verdict {
    Met,
    Unmet { gaps: Vec<Gap> },
    Undeclared,
}

fn reached(observed: &[Observed], situation: &str) -> u64 {
    observed.iter().find(|o| o.situation == situation).map(|o| o.reached).unwrap_or(0)
}

fn gap_of(observed: &[Observed], floor: &Floor) -> Option<Gap> {
    let count = reached(observed, &floor.situation);
    if count == 0 {
        Some(Gap::Vacuous { situation: floor.situation.clone() })
    } else if count < floor.at_least {
        Some(Gap::Short { situation: floor.situation.clone(), reached: count, at_least: floor.at_least })
    } else {
        None
    }
}

fn blank(reason: &str) -> bool {
    reason.trim_matches([' ', '\t', '\r', '\n']).is_empty()
}

fn excused(waivers: &[Waiver]) -> Vec<&str> {
    waivers
        .iter()
        .filter(|w| !blank(&w.reason))
        .flat_map(|w| w.situations.iter().map(String::as_str))
        .collect()
}

/// @implements REQ-DRT-COVER.floor_stated
/// @implements REQ-DRT-COVER.law_coverage
/// @partial reason="per-law floors over a law's precondition are not designed; a binding's named situations stand in for them"
/// @implements REQ-DRT-COVER.vacuous_named
/// @implements REQ-DRT-COVER.waiver_reasoned
/// @drt REQ-DRT-COVER.floor_stated
/// @drt REQ-DRT-COVER.law_coverage
/// @drt REQ-DRT-COVER.vacuous_named
/// @drt REQ-DRT-COVER.waiver_reasoned
pub fn verdict(floors: Vec<Floor>, observed: Vec<Observed>, waivers: Vec<Waiver>) -> Verdict {
    if floors.is_empty() {
        return Verdict::Undeclared;
    }
    let excused = excused(&waivers);
    let gaps: Vec<Gap> = floors
        .iter()
        .filter_map(|floor| gap_of(&observed, floor))
        .filter(|gap| !excused.contains(&gap.situation()))
        .collect();
    if gaps.is_empty() {
        Verdict::Met
    } else {
        Verdict::Unmet { gaps }
    }
}

/// @implements REQ-DRT-COVER.waiver_unused_reported
/// @drt REQ-DRT-COVER.waiver_unused_reported
pub fn unused_waivers(floors: Vec<Floor>, observed: Vec<Observed>, waivers: Vec<Waiver>) -> Vec<String> {
    let used = |name: &str| {
        floors.iter().find(|f| f.situation == name).is_some_and(|floor| gap_of(&observed, floor).is_some())
    };
    let mut out: Vec<String> = Vec::new();
    for waiver in &waivers {
        for name in &waiver.situations {
            if (blank(&waiver.reason) || !used(name)) && !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// @implements REQ-DRT-COVER.lines_run
/// @drt REQ-DRT-COVER.lines_run
pub fn line_reach(hits: Vec<(u32, u64)>, item: Vec<(u32, String)>) -> Vec<Observed> {
    let mut out: Vec<Observed> = Vec::new();
    for (line, text) in &item {
        let Some((_, count)) = hits.iter().find(|(at, _)| at == line) else { continue };
        let name = format!("line: {}", text.trim_matches([' ', '\t', '\r', '\n']));
        match out.iter_mut().find(|o| o.situation == name) {
            Some(seen) => seen.reached = seen.reached.min(*count),
            None => out.push(Observed { situation: name, reached: *count }),
        }
    }
    out
}

pub fn floors_of(observed: &[Observed]) -> Vec<Floor> {
    observed.iter().map(|o| Floor { situation: o.situation.clone(), at_least: 1 }).collect()
}

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

    fn waiver(situations: &[&str], reason: &str) -> Waiver {
        Waiver { situations: situations.iter().map(|s| s.to_string()).collect(), reason: reason.into() }
    }

    fn vacuous(situation: &str) -> Gap {
        Gap::Vacuous { situation: situation.into() }
    }

    /// @tests REQ-DRT-COVER.vacuous_named
    #[test]
    fn a_law_nothing_reached_is_vacuous_and_not_short() {
        assert_eq!(
            verdict(vec![floor("deletes a file", 10)], vec![], vec![]),
            Verdict::Unmet { gaps: vec![vacuous("deletes a file")] }
        );
        assert_eq!(
            verdict(vec![floor("deletes a file", 10)], vec![observed("deletes a file", 3)], vec![]),
            Verdict::Unmet {
                gaps: vec![Gap::Short { situation: "deletes a file".into(), reached: 3, at_least: 10 }]
            }
        );
    }

    /// @tests REQ-DRT-COVER.vacuous_named
    #[test]
    fn a_zero_floor_is_vacuous_and_every_gap_is_named() {
        assert_eq!(
            verdict(vec![floor("x", 0)], vec![], vec![]),
            Verdict::Unmet { gaps: vec![vacuous("x")] }
        );
        assert_eq!(
            verdict(vec![floor("a", 5), floor("b", 1)], vec![observed("a", 2)], vec![]),
            Verdict::Unmet {
                gaps: vec![Gap::Short { situation: "a".into(), reached: 2, at_least: 5 }, vacuous("b")]
            }
        );
    }

    /// @tests REQ-DRT-COVER.floor_stated
    #[test]
    fn stating_no_floor_is_not_the_same_as_meeting_one() {
        assert_eq!(verdict(vec![], vec![observed("anything", 900)], vec![]), Verdict::Undeclared);
        assert_eq!(verdict(vec![floor("anything", 0)], vec![observed("anything", 1)], vec![]), Verdict::Met);
    }

    /// @tests REQ-DRT-COVER.floor_stated
    #[test]
    fn a_floor_without_a_count_needs_one_case() {
        let parsed: Floor = serde_json::from_str(r#"{"situation": "deletes"}"#).unwrap();
        assert_eq!(parsed, floor("deletes", 1));
    }

    /// @tests REQ-DRT-COVER.waiver_reasoned
    #[test]
    fn only_a_reasoned_waiver_excuses() {
        let floors = vec![floor("x negative", 1), floor("x zero", 1)];
        let seen = vec![observed("x zero", 4)];
        assert_eq!(
            verdict(floors.clone(), seen.clone(), vec![waiver(&["x negative"], " \t")]),
            Verdict::Unmet { gaps: vec![vacuous("x negative")] }
        );
        assert_eq!(verdict(floors, seen, vec![waiver(&["x negative"], "callers never send one")]), Verdict::Met);
    }

    /// @tests REQ-DRT-COVER.waiver_unused_reported
    #[test]
    fn a_waiver_that_excuses_nothing_is_reported() {
        let floors = vec![floor("x negative", 1), floor("x zero", 1)];
        let seen = vec![observed("x zero", 4)];
        let waivers = vec![
            waiver(&["x negative"], "rare"),
            waiver(&["x zero", "line: gone()"], "stale"),
            waiver(&["x negative"], ""),
        ];
        assert_eq!(unused_waivers(floors, seen, waivers), ["x zero", "line: gone()", "x negative"]);
    }

    /// @tests REQ-DRT-COVER.lines_run
    #[test]
    fn lines_are_named_by_text_and_a_repeat_keeps_its_least() {
        let reach = line_reach(
            vec![(3, 5), (7, 0), (9, 2)],
            vec![(3, "  x += 1;".into()), (7, "x += 1;".into()), (8, "// no".into()), (9, "}".into())],
        );
        assert_eq!(reach, vec![observed("line: x += 1;", 0), observed("line: }", 2)]);
    }

    /// @tests REQ-DRT-COVER.floor_unmet_is_not_pass
    #[test]
    fn a_run_below_its_floor_is_not_evidence() {
        assert_eq!(level(true, Verdict::Met), Level::L3);
        assert_eq!(level(true, Verdict::Undeclared), Level::L1);
        assert_eq!(level(true, Verdict::Unmet { gaps: vec![vacuous("x")] }), Level::L1);
        assert_eq!(level(false, Verdict::Met), Level::L1);
    }
}
