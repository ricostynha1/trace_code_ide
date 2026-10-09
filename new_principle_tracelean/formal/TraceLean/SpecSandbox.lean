import TraceLean.Effects

/-!
# What the sandbox laws must answer

Specifications for the checks in `TraceLean.Effects`, written from the
requirement text rather than from the functions, and the proofs that each
check meets its specification and is the only thing that does.
-/

namespace TraceLean.SpecSandbox

open TraceLean.Command
open TraceLean.Policy
open TraceLean.Effects

/-- A capability report names its state: the mechanism in use, or the reason
there is none. An empty name is no name. -/
def StateNamed : Capability → Prop
  | .contained mechanism => mechanism ≠ ""
  | .unavailable reason => reason ≠ ""

/-- `vs` is what the containment report `reported` owes: nothing when it names
its state, and the one complaint that containment went unreported when it names
nothing -- the silent fallback is never accepted.

@models REQ-SBX.capability_reported -/
def CapabilityReported (reported : Capability) (vs : List Violation) : Prop :=
  (StateNamed reported → vs = []) ∧
  (¬ StateNamed reported → vs = [Violation.containmentUnreported])

/-- `r` is what one entry of the real tree says about the copy the tool was
given. A path the policy does not mirror is not compared. A mirrored path must
be in the copy with the same content; otherwise it is named as missing from the
copy, or as differing from it.

@models REQ-OBS.workspace_is_a_copy -/
def CopiedFaithfully (workspace : Workspace) (entry : String × String)
    (r : Option Violation) : Prop :=
  (classify entry.1 ≠ Class.mirrored → r = none) ∧
  (classify entry.1 = Class.mirrored →
    (workspace.get entry.1 = none → r = some (Violation.missingFromCopy entry.1)) ∧
    (workspace.get entry.1 = some entry.2 → r = none) ∧
    (∀ c, workspace.get entry.1 = some c → c ≠ entry.2 →
      r = some (Violation.copyDiffers entry.1)))

/-- An option is empty or holds something: the split `cases o` gives, with the
equation kept, which `cases h : o` would give outside the grammar's subset. -/
theorem none_or_some {α : Type} (o : Option α) : o = none ∨ ∃ x, o = some x := by
  cases o
  case none => exact Or.inl rfl
  case some x => exact Or.inr ⟨x, rfl⟩

/-- The containment check meets its specification, and nothing else does.

@pins REQ-SBX.capability_reported -/
theorem capability_reported_pinned :
    (∀ x, CapabilityReported x (capabilityViolations x)) ∧
    (∀ x y1 y2, CapabilityReported x y1 → CapabilityReported x y2 → y1 = y2) := by
  -- `cases` on `Classical.em` rather than `by_cases`, and `case` arms rather
  -- than `with |`: the same splits, in the subset of Lean the annotation
  -- grammar reads (ADR-0008).
  constructor
  · intro x
    cases x
    case contained m =>
      cases Classical.em (m = "")
      case inl h => simp [CapabilityReported, StateNamed, capabilityViolations, h]
      case inr h => simp [CapabilityReported, StateNamed, capabilityViolations, h]
    case unavailable r =>
      cases Classical.em (r = "")
      case inl h => simp [CapabilityReported, StateNamed, capabilityViolations, h]
      case inr h => simp [CapabilityReported, StateNamed, capabilityViolations, h]
  · intro x y1 y2 h1 h2
    cases Classical.em (StateNamed x)
    case inl n => rw [h1.1 n, h2.1 n]
    case inr n => rw [h1.2 n, h2.2 n]

/-- The copy check meets its specification, and nothing else does.

@pins REQ-OBS.workspace_is_a_copy -/
theorem workspace_is_a_copy_pinned :
    (∀ x1 x2, CopiedFaithfully x1 x2 (copyCheck x1 x2)) ∧
    (∀ x1 x2 y1 y2, CopiedFaithfully x1 x2 y1 → CopiedFaithfully x1 x2 y2 → y1 = y2) := by
  constructor
  · intro w e
    cases Classical.em (classify e.1 = Class.mirrored)
    case inl m =>
      have hm : isMirrored e.1 = true := by simp [isMirrored, m]
      constructor
      · intro h
        exact absurd m h
      · intro _
        refine ⟨?_, ?_, ?_⟩
        · intro g
          simp [copyCheck, hm, g]
        · intro g
          simp [copyCheck, hm, g]
        · intro c g d
          simp [copyCheck, hm, g, d]
    case inr m =>
      have hm : isMirrored e.1 = false := by simp [isMirrored, m]
      constructor
      · intro _
        simp [copyCheck, hm]
      · intro h
        exact absurd h m
  · intro w e y1 y2 h1 h2
    cases Classical.em (classify e.1 = Class.mirrored)
    case inl m =>
      have a1 := h1.2 m
      have a2 := h2.2 m
      cases none_or_some (w.get e.1)
      case inl g => rw [a1.1 g, a2.1 g]
      case inr found =>
        rcases found with ⟨c, g⟩
        cases Classical.em (c = e.2)
        case inl d =>
          rw [d] at g
          rw [a1.2.1 g, a2.2.1 g]
        case inr d => rw [a1.2.2 c g d, a2.2.2 c g d]
    case inr m => rw [h1.1 m, h2.1 m]

end TraceLean.SpecSandbox
