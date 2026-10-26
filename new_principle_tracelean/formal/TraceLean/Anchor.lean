import TraceLean.Evidence
import Lean

namespace TraceLean.Anchor

open TraceLean.Evidence

open Lean (ToJson FromJson)

inductive AnchorKind where
  | decl (symbolPath : String)
  | region (start «end» : Nat)
  | file
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-ANCHOR.symbol_not_line
@models REQ-ANCHOR.stable_under_move -/
def anchorIdent (file : String) (kind : AnchorKind) : String :=
  match kind with
  | .decl symbolPath => file ++ "::" ++ symbolPath
  | .region start «end» => file ++ "@" ++ toString start ++ ".." ++ toString «end»
  | .file => file

/-- @models REQ-ANCHOR.imprecise_capped -/
def anchorCeiling (precise : Bool) (claimed : Level) : Level :=
  if precise then claimed else Level.L1

/-- @proves REQ-ANCHOR.stable_under_move -/
theorem moving_a_declaration_keeps_its_identity (file name : String) :
    anchorIdent file (.decl name) = file ++ "::" ++ name ∧
    anchorIdent file .file = file := ⟨rfl, rfl⟩

/-- @proves REQ-ANCHOR.imprecise_capped -/
theorem an_imprecise_anchor_is_capped_at_the_lowest_level (claimed : Level) :
    anchorCeiling false claimed = Level.L1 ∧
    (∀ other : Level, (anchorCeiling false claimed).toNat ≤ other.toNat) ∧
    anchorCeiling true claimed = claimed := by
  refine ⟨rfl, ?_, rfl⟩
  intro other
  cases other <;> simp [anchorCeiling, Level.toNat]

/-- @proves REQ-ANCHOR.symbol_not_line -/
theorem a_symbol_anchor_is_addressed_by_its_path (file a b : String) :
    anchorIdent file (.decl a) = anchorIdent file (.decl b) ↔ a = b := by
  constructor
  · intro h
    have d := congrArg String.data h
    simp only [anchorIdent, String.append_assoc] at d
    cases a
    cases b
    simp only [String.data_append] at d
    exact congrArg String.mk (List.append_cancel_left (List.append_cancel_left d))
  · intro h
    rw [h]

end TraceLean.Anchor
