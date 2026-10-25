import TraceLean.Lockfile

namespace TraceLean.SpecLockfile

open TraceLean.Record
open TraceLean.Lockfile

/-- @specifies REQ-LOCK.evidence_preserved -/
def CarriedUnchanged (evidence carried : List Evidence) : Prop :=
  ∀ i, carried.get? i = evidence.get? i

/-- @pins REQ-LOCK.evidence_preserved -/
theorem evidence_preserved_pinned :
    (∀ x1, CarriedUnchanged x1 (carriedEvidence x1)) ∧
    (∀ x1 y1 y2, CarriedUnchanged x1 y1 → CarriedUnchanged x1 y2 → y1 = y2) := by
  constructor
  · intro x1 i
    rfl
  · intro x1 y1 y2 h1 h2
    apply List.ext_get?
    intro i
    rw [h1 i, h2 i]

end TraceLean.SpecLockfile
