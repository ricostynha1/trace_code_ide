import TraceLean.Nav
import TraceLean.SpecRollup

namespace TraceLean.SpecNav

open TraceLean.Evidence
open TraceLean.Nav

/-- @specifies REQ-EVID.structural_not_applicable -/
def NotApplicable (m out : Marks) : Prop :=
  out.annotated = m.annotated ∧ out.judged = m.judged ∧ out.tested = false ∧ out.proved = false
    ∧ out.structural = true

/-- @pins REQ-EVID.structural_not_applicable -/
theorem structural_not_applicable_pinned :
    (∀ x1, NotApplicable x1 (structural x1)) ∧
    (∀ x1 y1 y2, NotApplicable x1 y1 → NotApplicable x1 y2 → y1 = y2) := by
  constructor
  · intro x1
    exact ⟨rfl, rfl, rfl, rfl, rfl⟩
  · intro x1 y1 y2 h1 h2
    apply TraceLean.SpecRollup.marks_ext y1 y2
    · rw [h1.1, h2.1]
    · rw [h1.2.1, h2.2.1]
    · rw [h1.2.2.1, h2.2.2.1]
    · rw [h1.2.2.2.1, h2.2.2.2.1]
    · rw [h1.2.2.2.2, h2.2.2.2.2]

/-- @specifies REQ-NAV.session_recalled -/
def ReopensWhatExists (files : List String) (shown : Option String) (existing : List String)
    (out : List String) : Prop :=
  out = (files ++ shown.toList).filter (fun p => existing.contains p)

/-- @pins REQ-NAV.session_recalled -/
theorem session_recalled_pinned :
    (∀ x1 x2 x3, ReopensWhatExists x1 x2 x3 (reopened x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, ReopensWhatExists x1 x2 x3 y1 → ReopensWhatExists x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3
    rfl
  · intro x1 x2 x3 y1 y2 h1 h2
    rw [h1, h2]

end TraceLean.SpecNav
