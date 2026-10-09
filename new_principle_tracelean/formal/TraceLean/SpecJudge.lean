import TraceLean.Judge

/-!
# Specifications for the human judge

What `REQ-JUDGE.invalidated_by_change` asks of the question "does this
judgement still apply", written from the clause rather than from `stillApplies`.
-/

namespace TraceLean.SpecJudge

open TraceLean.Judge

/-- `applies` says whether `judgement` still stands for `material`. It stands
exactly when neither the requirement text nor the model has changed since it
was made: the hashes it was made against are the hashes in front of us. Either
one differing invalidates it.

@specifies REQ-JUDGE.invalidated_by_change -/
def StillStands (judgement : Judgement) (material : Material) (applies : Bool) : Prop :=
  applies = true ↔
    (judgement.requirementHash = material.requirementHash ∧
      judgement.modelHash = material.modelHash)

/-- The check meets the clause, and nothing else does.

@pins REQ-JUDGE.invalidated_by_change -/
theorem invalidated_by_change_pinned :
    (∀ x1 x2, StillStands x1 x2 (stillApplies x1 x2)) ∧
    (∀ x1 x2 y1 y2, StillStands x1 x2 y1 → StillStands x1 x2 y2 → y1 = y2) := by
  constructor
  · intro x1 x2
    simp [StillStands, stillApplies]
  · intro x1 x2 y1 y2 h1 h2
    cases y1
    all_goals cases y2
    all_goals simp_all [StillStands]

end TraceLean.SpecJudge
