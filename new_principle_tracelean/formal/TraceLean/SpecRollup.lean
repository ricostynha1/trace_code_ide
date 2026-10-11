import TraceLean.Rollup

/-!
# Specifications for the roll-up

What `REQ-ROLLUP.every_part_holds` and `ARCH-HONEST.weakest_link` ask of an
aggregate, written from the clauses rather than from `combine`, and the proof
that they leave `combine` no freedom.
-/

namespace TraceLean.SpecRollup

open TraceLean.Evidence
open TraceLean.Rollup

/--
`y` is the aggregate of `parts`: each mark held exactly when every part holds
it. A majority or a mean would hold marks some part does not.

With no parts there is nothing to hold a mark, so none is held; every mark
held vacuously is the comfort `ARCH-HONEST` refuses.

@specifies REQ-ROLLUP.every_part_holds
@specifies ARCH-HONEST.weakest_link
-/
def EveryPartHolds (parts : List Marks) (y : Marks) : Prop :=
  (parts = [] → y = Marks.empty) ∧
  (Not (parts = []) →
    y.annotated = parts.all (·.annotated) ∧ y.judged = parts.all (·.judged) ∧
    y.tested = parts.all (·.tested) ∧ y.proved = parts.all (·.proved))

/-- What a left fold of `Marks.both` from a first part comes to: each mark held
exactly when the first part and every later one hold it. -/
theorem fold_both (rest : List Marks) : ∀ first : Marks,
    (rest.foldl Marks.both first).annotated = (first :: rest).all (·.annotated) ∧
    (rest.foldl Marks.both first).judged = (first :: rest).all (·.judged) ∧
    (rest.foldl Marks.both first).tested = (first :: rest).all (·.tested) ∧
    (rest.foldl Marks.both first).proved = (first :: rest).all (·.proved) := by
  induction rest
  case nil =>
    intro first
    simp
  case cons x xs ih =>
    intro first
    have h := ih (Marks.both first x)
    simp only [List.foldl]
    refine ⟨h.1.trans ?_, h.2.1.trans ?_, h.2.2.1.trans ?_, h.2.2.2.trans ?_⟩ <;>
      simp [Marks.both, Bool.and_assoc]

/-- Two marks agreeing on every field are the same marks. -/
theorem marks_ext (a b : Marks) (h1 : a.annotated = b.annotated) (h2 : a.judged = b.judged)
    (h3 : a.tested = b.tested) (h4 : a.proved = b.proved) : a = b := by
  cases a
  cases b
  simp only at h1 h2 h3 h4
  rw [h1, h2, h3, h4]

/-- The aggregate meets the clause, and nothing else does.

@pins REQ-ROLLUP.every_part_holds
@pins ARCH-HONEST.weakest_link -/
theorem every_part_holds_pinned :
    (∀ x1, EveryPartHolds x1 (combine x1)) ∧
    (∀ x1 y1 y2, EveryPartHolds x1 y1 → EveryPartHolds x1 y2 → y1 = y2) := by
  constructor
  · intro x1
    cases x1
    case nil =>
      constructor
      · intro _
        rfl
      · intro h
        exact absurd rfl h
    case cons x rest =>
      constructor
      · intro h
        exact (nomatch h)
      · intro _
        exact fold_both rest x
  · intro x1 y1 y2 h1 h2
    cases x1
    case nil =>
      rw [h1.1 rfl, h2.1 rfl]
    case cons x rest =>
      have n : Not (x :: rest = []) := fun h => (nomatch h)
      have m1 := h1.2 n
      have m2 := h2.2 n
      have a1 := m1.2
      have a2 := m2.2
      have b1 := a1.2
      have b2 := a2.2
      apply marks_ext y1 y2
      · rw [m1.1, m2.1]
      · rw [a1.1, a2.1]
      · rw [b1.1, b2.1]
      · rw [b1.2, b2.2]

/-- A requirement's marks are what every counted clause over its reachable set
holds — whichever path reaches each.

@proves REQ-ROLLUP.counted_once
@proves ARCH-HONEST.weakest_link -/
theorem rollUp_holds_what_every_clause_holds (nodes : List Node)
    (held : List ((String × Option String) × Marks)) (root : String) (floor : Marks) :
    EveryPartHolds (reachMarks (canonNodes nodes) (canonHeld held) root)
      (rollUp nodes held root floor).marks := by
  rw [marks_are_over_the_reachable_set]
  exact every_part_holds_pinned.1 _

end TraceLean.SpecRollup
