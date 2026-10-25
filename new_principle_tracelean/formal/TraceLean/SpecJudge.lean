import TraceLean.Judge

namespace TraceLean.SpecJudge

open TraceLean.Judge

/-- @specifies REQ-JUDGE.invalidated_by_change -/
def StillStands (judgement : Judgement) (material : Material) (applies : Bool) : Prop :=
  applies = true ↔
    (judgement.requirementHash = material.requirementHash ∧
      judgement.modelHash = material.modelHash)

/-- @pins REQ-JUDGE.invalidated_by_change -/
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

/-- @proves REQ-JUDGE.invalidated_by_change -/
theorem a_judgement_stands_until_either_changes (judgement : Judgement) (material : Material) :
    StillStands judgement material (stillApplies judgement material) :=
  invalidated_by_change_pinned.1 judgement material

end TraceLean.SpecJudge
