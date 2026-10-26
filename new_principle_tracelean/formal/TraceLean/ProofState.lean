import Lean

namespace TraceLean.ProofState

/-- @models REQ-LSP.proof_state_shown -/
def goalsShown (answered : Bool) (goals : Option (List String)) (rendered : Option String) : String :=
  if answered then
    match goals with
    | some gs =>
      if gs.isEmpty then "no goals" else String.intercalate "\n\n" gs
    | none =>
      match rendered with
      | some text => text
      | none => "no goals here"
  else "no goals here"

/-- @proves REQ-LSP.proof_state_shown -/
theorem the_reported_goals_are_what_is_shown (goals : List String) (rendered : Option String)
    (answered : Bool) (reported : Option (List String)) (other : Option String) :
    (goals ≠ [] → goalsShown true (some goals) rendered = String.intercalate "\n\n" goals) ∧
    (goalsShown true (some []) rendered ≠ goalsShown false reported other) ∧
    (answered = false → goalsShown answered reported other = "no goals here") := by
  refine ⟨?_, ?_, ?_⟩
  · intro h
    cases goals with
    | nil => exact absurd rfl h
    | cons g gs => simp [goalsShown]
  · intro h
    simp [goalsShown] at h
  · intro h
    simp [goalsShown, h]

end TraceLean.ProofState
