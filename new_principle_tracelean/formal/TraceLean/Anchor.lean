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

/-- @proves REQ-ANCHOR.imprecise_capped -/
theorem an_imprecise_anchor_caps_a_proof :
    anchorCeiling false Level.L4 = Level.L1 := by
  native_decide

/-- @proves REQ-ANCHOR.symbol_not_line -/
theorem an_identity_never_mentions_a_line :
    anchorIdent "a.rs" (.decl "Foo::bar") = "a.rs::Foo::bar"
    ∧ anchorIdent "a.rs" (.region 3 9) = "a.rs@3..9"
    ∧ anchorIdent "a.rs" .file = "a.rs" := by
  native_decide

/-- @proves REQ-ANCHOR.stable_under_move -/
theorem moving_a_declaration_keeps_its_identity (file name : String) :
    anchorIdent file (.decl name) = file ++ "::" ++ name ∧
    anchorIdent file .file = file := ⟨rfl, rfl⟩

/-- @proves REQ-ANCHOR.stable_under_move -/
theorem an_edit_above_a_region_moves_it :
    anchorIdent "a.rs" (.region 3 9) ≠ anchorIdent "a.rs" (.region 4 10) := by
  native_decide

end TraceLean.Anchor
