import TraceLean.Lockfile

/-!
# Specifications for the committed index

What `REQ-LOCK.evidence_preserved` asks of rendering, written from the clause
rather than from `carriedEvidence`.
-/

namespace TraceLean.SpecLockfile

open TraceLean.Record
open TraceLean.Lockfile

/-- `carried` is `evidence` carried through unchanged: at every position the
record that went in at that position, and nothing past the last of them. No
record dropped, added, reordered or altered.

@models REQ-LOCK.evidence_preserved -/
def CarriedUnchanged (evidence carried : List Evidence) : Prop :=
  ∀ i, carried.get? i = evidence.get? i

/-- Rendering meets the clause, and nothing else does.

@pins REQ-LOCK.evidence_preserved -/
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
