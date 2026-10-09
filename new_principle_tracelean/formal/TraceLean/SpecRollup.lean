import TraceLean.Rollup

/-!
# Specifications for the roll-up

What `REQ-ROLLUP.min_not_mean` and `ARCH-HONEST.weakest_link` ask of an
aggregate, written from the clauses rather than from `combine`, and the proof
that they leave `combine` no freedom.
-/

namespace TraceLean.SpecRollup

open TraceLean.Evidence
open TraceLean.Rollup

/-- Reading a level back from its rung. -/
def rungLevel : Nat → Level
  | 1 => Level.L1 | 2 => Level.L2 | 3 => Level.L3 | _ => Level.L4

theorem rungLevel_toNat (a : Level) : rungLevel a.toNat = a := by
  cases a
  all_goals rfl

/-- Each below the other is the same level. -/
theorem level_le_both_eq {a b : Level} (h1 : a ≤ b) (h2 : b ≤ a) : a = b := by
  have h : a.toNat = b.toNat := Nat.le_antisymm h1 h2
  rw [← rungLevel_toNat a, ← rungLevel_toNat b, h]

/--
`y` is the aggregate of `levels`: the minimum of them, which is one of the
levels themselves and above none of them. A mean would in general be neither.

With no children there is nothing to take a minimum of, and no evidence is the
lowest level (`REQ-EVID.absent_is_lowest`); the top of the ladder is the last
thing an absence should report.

@models REQ-ROLLUP.min_not_mean
@models ARCH-HONEST.weakest_link
-/
def MinimumOf (levels : List Level) (y : Level) : Prop :=
  (levels = [] → y = Level.L1) ∧
  (Not (levels = []) → List.Mem y levels ∧ ∀ l, List.Mem l levels → y ≤ l)

/-- What a left fold of `Level.min` from a start value comes to: the start or
one of the list, and no higher than either. -/
theorem fold_min (levels : List Level) : ∀ a : Level,
    (List.Mem (levels.foldl Level.min a) levels ∨ levels.foldl Level.min a = a) ∧
    levels.foldl Level.min a ≤ a ∧
    (∀ l, List.Mem l levels → levels.foldl Level.min a ≤ l) := by
  induction levels
  case nil =>
    intro a
    exact ⟨Or.inr rfl, Level.le_refl a, (fun l h => (nomatch h))⟩
  case cons x rest ih =>
    intro a
    have h := ih (Level.min a x)
    simp only [List.foldl]
    refine ⟨?_, ?_, ?_⟩
    · refine h.1.elim (fun hm => Or.inl (List.Mem.tail x hm)) (fun he => ?_)
      rw [he]
      refine (Level.min_eq a x).elim (fun ha => Or.inr ha) (fun hx => ?_)
      rw [hx]
      exact Or.inl (List.Mem.head rest)
    · exact Level.le_trans h.2.1 (Level.min_le_left a x)
    · intro l hl
      cases hl
      case head => exact Level.le_trans h.2.1 (Level.min_le_right a x)
      case tail hr => exact h.2.2 l hr

/-- The aggregate meets the clause, and nothing else does.

@pins REQ-ROLLUP.min_not_mean
@pins ARCH-HONEST.weakest_link -/
theorem min_not_mean_pinned :
    (∀ x1, MinimumOf x1 (combine x1)) ∧
    (∀ x1 y1 y2, MinimumOf x1 y1 → MinimumOf x1 y2 → y1 = y2) := by
  constructor
  · intro x1
    cases x1
    case nil =>
      constructor
      · intro _
        exact combine_empty
      · intro h
        exact absurd rfl h
    case cons x rest =>
      refine ⟨(fun h => (nomatch h)), (fun _ => ?_)⟩
      have h := fold_min (x :: rest) Level.L4
      have hc : combine (x :: rest) = (x :: rest).foldl Level.min Level.L4 := rfl
      rw [hc]
      refine ⟨?_, h.2.2⟩
      refine h.1.elim id (fun he => ?_)
      have hx : Level.L4 ≤ x := by
        rw [← he]
        exact h.2.2 x (List.Mem.head rest)
      have top : x = Level.L4 := Level.eq_L4_of_four_le hx
      rw [he, ← top]
      exact List.Mem.head rest
  · intro x1 y1 y2 h1 h2
    cases x1
    case nil =>
      rw [h1.1 rfl, h2.1 rfl]
    case cons x rest =>
      have n : Not (x :: rest = []) := fun h => nomatch h
      have m1 := h1.2 n
      have m2 := h2.2 n
      exact level_le_both_eq (m1.2 y2 m2.1) (m2.2 y1 m1.1)

end TraceLean.SpecRollup
