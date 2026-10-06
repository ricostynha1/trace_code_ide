import TraceLean.Evidence
import Lean

/-!
# What a passing run is worth

Models `REQ-DRT-COVER`. A differential run that finds no disagreement says
nothing on its own. It says something once you know which situations its cases
reached: a law about deletions, checked over two thousand cases none of which
deleted anything, is satisfied and vacuous, and reporting that as evidence is
the most comfortable lie this system could tell.
-/

namespace TraceLean.Coverage

open TraceLean.Evidence

open Lean (ToJson FromJson)

/-- A situation a binding's runs must reach, and how often. -/
structure Floor where
  situation : String
  atLeast : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- How often a run actually reached a situation. -/
structure Observed where
  situation : String
  reached : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
What the coverage of a run amounts to.

Four answers, and the distinction between the middle two is the point.
*Vacuous* means no case reached the situation at all, so the law was never asked
its question; *short* means it was asked, just not often enough to be
convincing. They call for different work — a generator that cannot produce the
case, against one that produces it rarely.
-/
inductive CoverageVerdict where
  | met
  | vacuous (situation : String)
  | short (situation : String) (reached atLeast : Nat)
  /-- The binding states no floor, so nothing is known about what the run
  reached. Not the same as meeting a floor of zero: one is a claim somebody
  made, the other is the absence of one. -/
  | undeclared
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def reachedCount (observed : List Observed) (situation : String) : Nat :=
  ((observed.find? (·.situation == situation)).map (·.reached)).getD 0

/--
Judge a run's coverage against the floors its binding declared.

The first floor that fails is the one reported, in declaration order, so the
message is stable between runs and a person fixing them has an order to work in.

@models REQ-DRT-COVER.floor_stated
@models REQ-DRT-COVER.law_coverage
@models REQ-DRT-COVER.vacuous_named
-/
def coverageVerdict (floors : List Floor) (observed : List Observed) : CoverageVerdict :=
  match floors with
  | [] => .undeclared
  | _ =>
    let failure := floors.findSome? (fun floor =>
      let count := reachedCount observed floor.situation
      if count == 0 && floor.atLeast > 0 then some (CoverageVerdict.vacuous floor.situation)
      else if count < floor.atLeast then
        some (CoverageVerdict.short floor.situation count floor.atLeast)
      else none)
    failure.getD .met

/--
What a run establishes, given whether it agreed and what it covered.

L3 needs both. A run that agreed but did not reach its floor has not shown what
the floor exists to make it show, and reporting it as L3 would put the most
comfortable answer at the highest rung this system can reach without a proof.

@models REQ-DRT-COVER.floor_unmet_is_not_pass
-/
def coverageLevel (agreed : Bool) (verdict : CoverageVerdict) : Level :=
  match agreed, verdict with
  | true, .met => Level.L3
  | _, _ => Level.L1

/-- A law nothing reached is vacuous, which is a different answer from short.

@proves REQ-DRT-COVER.vacuous_named -/
theorem nothing_reached_is_vacuous_not_short :
    coverageVerdict [{ situation := "deletes", atLeast := 10 }] []
      = CoverageVerdict.vacuous "deletes" := by
  native_decide

/-- Stating no floor is not the same as meeting one.

@proves REQ-DRT-COVER.floor_stated -/
theorem no_floor_is_not_a_met_floor :
    coverageVerdict [] [{ situation := "deletes", reached := 900 }]
      = CoverageVerdict.undeclared := by
  native_decide

/-- Nothing but an agreeing run that met its floor reaches L3.

@proves REQ-DRT-COVER.floor_unmet_is_not_pass -/
theorem only_a_covered_agreement_is_evidence (agreed : Bool) (v : CoverageVerdict) :
    coverageLevel agreed v = Level.L3 → agreed = true ∧ v = CoverageVerdict.met := by
  intro h
  cases agreed <;> cases v <;> simp_all [coverageLevel]

end TraceLean.Coverage
