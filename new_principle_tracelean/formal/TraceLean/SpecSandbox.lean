import TraceLean.Effects

namespace TraceLean.SpecSandbox

open TraceLean.Command
open TraceLean.Policy
open TraceLean.Effects

def StateNamed : Capability → Prop
  | .contained mechanism => mechanism ≠ ""
  | .unavailable reason => reason ≠ ""

/-- @specifies REQ-SBX.capability_reported -/
def CapabilityReported (reported : Capability) (vs : List Violation) : Prop :=
  (StateNamed reported → vs = []) ∧
  (¬ StateNamed reported → vs = [Violation.containmentUnreported])

/-- @specifies REQ-OBS.workspace_is_a_copy -/
def CopiedFaithfully (before workspace : Workspace) (path : String)
    (r : Option Violation) : Prop :=
  (classify path = Class.mirrored →
    (before.get path = none → workspace.get path = none → r = none) ∧
    (∀ a, before.get path = some a → workspace.get path = none →
      r = some (Violation.missingFromCopy path)) ∧
    (∀ a, before.get path = some a → workspace.get path = some a → r = none) ∧
    (∀ a b, before.get path = some a → workspace.get path = some b → a ≠ b →
      r = some (Violation.copyDiffers path)) ∧
    (∀ b, before.get path = none → workspace.get path = some b →
      r = some (Violation.extraInCopy path))) ∧
  (classify path = Class.«protected» →
    (∀ a, before.get path = some a → workspace.get path = none →
      r = some (Violation.missingFromCopy path)) ∧
    (before.get path = none → r = none) ∧
    (∀ b, workspace.get path = some b → r = none)) ∧
  (classify path ≠ Class.mirrored → classify path ≠ Class.«protected» → r = none)

theorem none_or_some {α : Type} (o : Option α) : o = none ∨ ∃ x, o = some x := by
  cases o
  case none => exact Or.inl rfl
  case some x => exact Or.inr ⟨x, rfl⟩

/-- @pins REQ-SBX.capability_reported -/
theorem capability_reported_pinned :
    (∀ x, CapabilityReported x (capabilityViolations x)) ∧
    (∀ x y1 y2, CapabilityReported x y1 → CapabilityReported x y2 → y1 = y2) := by
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

theorem mirrored_not_protected (p : String) (m : classify p = Class.mirrored) :
    classify p ≠ Class.«protected» := by
  intro h
  rw [m] at h
  cases h

theorem copy_check_meets (b w : Workspace) (p : String) :
    CopiedFaithfully b w p (copyCheck b w p) := by
  cases Classical.em (classify p = Class.mirrored)
  case inl m =>
    have hm : isMirrored p = true := by simp [isMirrored, m]
    refine ⟨?_, ?_, ?_⟩
    · intro _
      refine ⟨?_, ?_, ?_, ?_, ?_⟩
      · intro g1 g2
        simp [copyCheck, hm, g1, g2]
      · intro a g1 g2
        simp [copyCheck, hm, g1, g2]
      · intro a g1 g2
        simp [copyCheck, hm, g1, g2]
      · intro a c g1 g2 d
        simp [copyCheck, hm, g1, g2, d]
      · intro c g1 g2
        simp [copyCheck, hm, g1, g2]
    · intro h
      exact absurd h (mirrored_not_protected p m)
    · intro n _
      exact absurd m n
  case inr m =>
    have hm : isMirrored p = false := by simp [isMirrored, m]
    cases Classical.em (classify p = Class.«protected»)
    case inl pr =>
      refine ⟨?_, ?_, ?_⟩
      · intro h
        exact absurd h m
      · intro _
        refine ⟨?_, ?_, ?_⟩
        · intro a g1 g2
          simp [copyCheck, hm, pr, g1, g2]
        · intro g1
          simp [copyCheck, hm, pr, g1]
        · intro c g2
          simp [copyCheck, hm, pr, g2]
      · intro _ n
        exact absurd pr n
    case inr pr =>
      refine ⟨?_, ?_, ?_⟩
      · intro h
        exact absurd h m
      · intro h
        exact absurd h pr
      · intro _ _
        simp [copyCheck, hm, pr]

theorem copy_check_unique (b w : Workspace) (p : String) (y1 y2 : Option Violation)
    (h1 : CopiedFaithfully b w p y1) (h2 : CopiedFaithfully b w p y2) : y1 = y2 := by
  cases Classical.em (classify p = Class.mirrored)
  case inl m =>
    have a1 := h1.1 m
    have a2 := h2.1 m
    cases none_or_some (b.get p)
    case inl g1 =>
      cases none_or_some (w.get p)
      case inl g2 => rw [a1.1 g1 g2, a2.1 g1 g2]
      case inr found =>
        rcases found with ⟨c, g2⟩
        rw [a1.2.2.2.2 c g1 g2, a2.2.2.2.2 c g1 g2]
    case inr had =>
      rcases had with ⟨a, g1⟩
      cases none_or_some (w.get p)
      case inl g2 => rw [a1.2.1 a g1 g2, a2.2.1 a g1 g2]
      case inr found =>
        rcases found with ⟨c, g2⟩
        cases Classical.em (a = c)
        case inl d =>
          rw [← d] at g2
          rw [a1.2.2.1 a g1 g2, a2.2.2.1 a g1 g2]
        case inr d => rw [a1.2.2.2.1 a c g1 g2 d, a2.2.2.2.1 a c g1 g2 d]
  case inr m =>
    cases Classical.em (classify p = Class.«protected»)
    case inl pr =>
      have c1 := h1.2.1 pr
      have c2 := h2.2.1 pr
      cases none_or_some (b.get p)
      case inl g1 => rw [c1.2.1 g1, c2.2.1 g1]
      case inr had =>
        rcases had with ⟨a, g1⟩
        cases none_or_some (w.get p)
        case inl g2 => rw [c1.1 a g1 g2, c2.1 a g1 g2]
        case inr found =>
          rcases found with ⟨c, g2⟩
          rw [c1.2.2 c g2, c2.2.2 c g2]
    case inr pr => rw [h1.2.2 m pr, h2.2.2 m pr]

/-- @pins REQ-OBS.workspace_is_a_copy -/
theorem workspace_is_a_copy_pinned :
    (∀ x1 x2 x3, CopiedFaithfully x1 x2 x3 (copyCheck x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, CopiedFaithfully x1 x2 x3 y1 → CopiedFaithfully x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro b w p
    exact copy_check_meets b w p
  · intro b w p y1 y2 h1 h2
    exact copy_check_unique b w p y1 y2 h1 h2

end TraceLean.SpecSandbox
