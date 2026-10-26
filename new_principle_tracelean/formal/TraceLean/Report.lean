import Lean
import TraceLean.Diff

namespace TraceLean.Report

open Lean (ToJson FromJson)
open TraceLean.Diff (Mark allMarks)

structure CardFacts where
  held : List Mark
  structural : Bool
  roles : List String
  refused : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Missing where
  | notAnnotated
  | noModel
  | notJudged
  | notImplemented
  | unbound
  | notAgreed
  | notApplicable
  | noTheorem
  | allRefused (reasons : List String)
  | notChecked
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Reason where
  mark : Mark
  missing : Missing
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def has (facts : CardFacts) (role : String) : Bool := facts.roles.contains role

def missingFor (facts : CardFacts) : Mark → Missing
  | .annotated => .notAnnotated
  | .judged => if has facts "models" then .notJudged else .noModel
  | .tested =>
    if facts.structural then .notApplicable
    else if !has facts "models" then .noModel
    else if !has facts "implements" then .notImplemented
    else if !has facts "drt" then .unbound
    else .notAgreed
  | .proved =>
    if facts.structural then .notApplicable
    else if !has facts "models" then .noModel
    else if !has facts "proves" then .noTheorem
    else if facts.refused.isEmpty then .notChecked
    else .allRefused facts.refused

/-- @models REQ-REPORT.why_not_held -/
def reasons (facts : CardFacts) : List Reason :=
  (allMarks.filter (fun m => !facts.held.contains m)).map (fun m => ⟨m, missingFor facts m⟩)

/-- @proves REQ-REPORT.why_not_held -/
theorem every_mark_not_held_and_only_those_is_given_a_reason (facts : CardFacts) (m : Mark) :
    (∃ r ∈ reasons facts, r.mark = m) ↔ m ∉ facts.held := by
  have all : m ∈ allMarks := by cases m <;> simp [allMarks]
  unfold reasons
  simp only [List.mem_map, List.mem_filter]
  constructor
  · rintro ⟨r, ⟨x, ⟨_, hx⟩, rfl⟩, rfl⟩
    simpa using hx
  · intro h
    exact ⟨_, ⟨m, ⟨all, by simpa using h⟩, rfl⟩, rfl⟩

/-- @proves REQ-REPORT.why_not_held -/
theorem the_reason_is_the_first_missing_step (facts : CardFacts) (r : Reason) (hr : r ∈ reasons facts) :
    (r.mark = .tested ∨ r.mark = .proved → facts.structural = false → has facts "models" = false →
      r.missing = .noModel) ∧
    (r.mark = .tested → facts.structural = false → has facts "models" = true →
      has facts "implements" = true → has facts "drt" = false → r.missing = .unbound) ∧
    (r.mark = .proved → facts.structural = false → has facts "models" = true → has facts "proves" = true →
      facts.refused ≠ [] → r.missing = .allRefused facts.refused) := by
  unfold reasons at hr
  obtain ⟨m, _, rfl⟩ := List.mem_map.mp hr
  refine ⟨fun hm hs hmod => ?_, fun hm hs hmod himp hdrt => ?_, fun hm hs hmod hpr hre => ?_⟩
  · rcases hm with hm | hm <;> (simp only at hm; subst hm; simp [missingFor, hs, hmod])
  · simp only at hm; subst hm; simp [missingFor, hs, hmod, himp, hdrt]
  · simp only at hm; subst hm
    have : facts.refused.isEmpty = false := by cases h : facts.refused <;> simp_all
    simp [missingFor, hs, hmod, hpr, this]

end TraceLean.Report
