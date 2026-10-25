import TraceLean.Rollup

namespace TraceLean.SpecRollup

open TraceLean.Evidence
open TraceLean.Rollup

/-- @specifies REQ-ROLLUP.every_part_holds
@specifies ARCH-HONEST.weakest_link -/
def EveryPartHolds (parts : List Marks) (y : Marks) : Prop :=
  (parts = [] → y = Marks.empty) ∧
  (Not (parts = []) →
    y.annotated = parts.all (·.annotated) ∧ y.judged = parts.all (·.judged) ∧
    y.structural = parts.all (·.structural) ∧
    y.tested = (!parts.all (·.structural) && parts.all (fun m => m.tested || m.structural)) ∧
    y.proved = (!parts.all (·.structural) && parts.all (fun m => m.proved || m.structural)))

def Normal (m : Marks) : Prop := m.structural = true → m.tested = false ∧ m.proved = false

theorem both_normal (a b : Marks) : Normal (Marks.both a b) := by
  intro h
  simp only [Marks.both] at h ⊢
  simp [h]

theorem fold_both (rest : List Marks) : ∀ acc : Marks, Normal acc →
    (rest.foldl Marks.both acc).annotated = (acc.annotated && rest.all (·.annotated)) ∧
    (rest.foldl Marks.both acc).judged = (acc.judged && rest.all (·.judged)) ∧
    (rest.foldl Marks.both acc).structural = (acc.structural && rest.all (·.structural)) ∧
    (rest.foldl Marks.both acc).tested = (!(acc.structural && rest.all (·.structural)) &&
      (acc.tested || acc.structural) && rest.all (fun m => m.tested || m.structural)) ∧
    (rest.foldl Marks.both acc).proved = (!(acc.structural && rest.all (·.structural)) &&
      (acc.proved || acc.structural) && rest.all (fun m => m.proved || m.structural)) := by
  induction rest with
  | nil =>
    intro acc hn
    rcases acc with ⟨a, j, t, p, s⟩
    cases s
    · simp
    · have := hn rfl
      simp_all
  | cons x xs ih =>
    intro acc _
    have h := ih (Marks.both acc x) (both_normal acc x)
    simp only [List.foldl]
    rw [h.1, h.2.1, h.2.2.1, h.2.2.2.1, h.2.2.2.2]
    rcases acc with ⟨a, j, t, p, s⟩
    rcases x with ⟨xa, xj, xt, xp, xs'⟩
    simp only [Marks.both, List.all_cons]
    generalize xs.all (·.annotated) = A
    generalize xs.all (·.judged) = J
    generalize xs.all (·.structural) = S
    generalize xs.all (fun m => m.tested || m.structural) = T
    generalize xs.all (fun m => m.proved || m.structural) = P
    cases a <;> cases j <;> cases t <;> cases p <;> cases s <;> cases xs' <;> cases xt <;> cases xp <;>
      simp

theorem marks_ext (a b : Marks) (h1 : a.annotated = b.annotated) (h2 : a.judged = b.judged)
    (h3 : a.tested = b.tested) (h4 : a.proved = b.proved) (h5 : a.structural = b.structural) :
    a = b := by
  cases a
  cases b
  simp only at h1 h2 h3 h4 h5
  rw [h1, h2, h3, h4, h5]

theorem combine_cons (x : Marks) (rest : List Marks) :
    let y := combine (x :: rest)
    y.annotated = (x :: rest).all (·.annotated) ∧ y.judged = (x :: rest).all (·.judged) ∧
    y.structural = (x :: rest).all (·.structural) ∧
    y.tested = (!(x :: rest).all (·.structural) &&
      (x :: rest).all (fun m => m.tested || m.structural)) ∧
    y.proved = (!(x :: rest).all (·.structural) &&
      (x :: rest).all (fun m => m.proved || m.structural)) := by
  have h := fold_both rest (Marks.both Marks.neutral x) (both_normal _ _)
  simp only [combine, List.foldl]
  rw [h.1, h.2.1, h.2.2.1, h.2.2.2.1, h.2.2.2.2]
  rcases x with ⟨xa, xj, xt, xp, xs'⟩
  simp only [Marks.both, Marks.neutral, List.all_cons]
  cases xs' <;> cases xt <;> cases xp <;> simp

/-- @pins REQ-ROLLUP.every_part_holds
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
        exact combine_cons x rest
  · intro x1 y1 y2 h1 h2
    cases x1
    case nil =>
      rw [h1.1 rfl, h2.1 rfl]
    case cons x rest =>
      have n : Not (x :: rest = []) := fun h => (nomatch h)
      have m1 := h1.2 n
      have m2 := h2.2 n
      apply marks_ext y1 y2
      · rw [m1.1, m2.1]
      · rw [m1.2.1, m2.2.1]
      · rw [m1.2.2.2.1, m2.2.2.2.1]
      · rw [m1.2.2.2.2, m2.2.2.2.2]
      · rw [m1.2.2.1, m2.2.2.1]

/-- @proves REQ-ROLLUP.counted_once
@proves ARCH-HONEST.weakest_link -/
theorem rollUp_holds_what_every_clause_holds (nodes : List Node)
    (held : List ((String × Option String) × Marks)) (root : String) (floor : Marks) :
    EveryPartHolds (reachMarks (canonNodes nodes) (canonHeld held) root)
      (rollUp nodes held root floor).marks := by
  rw [marks_are_over_the_reachable_set]
  exact every_part_holds_pinned.1 _

end TraceLean.SpecRollup
