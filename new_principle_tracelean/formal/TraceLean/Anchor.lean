import TraceLean.Evidence
import Lean

/-!
# Anchor identity

Models the identity half of `REQ-ANCHOR`. Finding a declaration needs a parser
the model cannot run; saying what the identity of one *is*, and what a link
through an imprecise one may claim, needs nothing but the answer the parser
gave.
-/

namespace TraceLean.Anchor

open TraceLean.Evidence

open Lean (ToJson FromJson)

/-- What an anchor names. -/
inductive AnchorKind where
  /-- A named declaration, addressed by its path within the file. -/
  | decl (symbolPath : String)
  /-- An explicit `begin` … `end` byte range. -/
  | region (start «end» : Nat)
  /-- The whole file — when nothing named follows, or the language has no
  grammar here. -/
  | file
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
An anchor's identity, from the file it is in and what it names.

No line number appears anywhere in it. A line number is the identity that rots
on the first edit above it, and an evidence record keyed on one is invalidated
by inserting a blank line.

The file *is* part of the identity, deliberately: evidence earned for a function
in one file is not inherited by the same text in another. What
`stable_under_move` asks is that the identity survives edits within the file —
moving a function down, reindenting it, rewriting the comment above it — which
it does, because none of those changes the path or the normalised body.

@models REQ-ANCHOR.symbol_not_line
@models REQ-ANCHOR.stable_under_move
-/
def anchorIdent (file : String) (kind : AnchorKind) : String :=
  match kind with
  | .decl symbolPath => file ++ "::" ++ symbolPath
  | .region start «end» => file ++ "@" ++ toString start ++ ".." ++ toString «end»
  | .file => file

/--
The highest level a link through this anchor may reach.

A file with no available grammar anchors to the whole file, and everything
claimed through it is capped at the bottom of the ladder. Not reported as broken
— an unsupported language is not a fault in the project — but not allowed to
carry a proof either, because nothing knows which declaration the claim was
about.

@models REQ-ANCHOR.imprecise_capped
-/
def anchorCeiling (precise : Bool) (claimed : Level) : Level :=
  if precise then claimed else Level.L1

/-- An imprecise anchor caps everything, including a proof.

@proves REQ-ANCHOR.imprecise_capped -/
theorem an_imprecise_anchor_caps_a_proof :
    anchorCeiling false Level.L4 = Level.L1 := by
  native_decide

/-- A region's identity is its byte range, and a declaration's is its path.
Neither mentions a line.

@proves REQ-ANCHOR.symbol_not_line -/
theorem an_identity_never_mentions_a_line :
    anchorIdent "a.rs" (.decl "Foo::bar") = "a.rs::Foo::bar"
    ∧ anchorIdent "a.rs" (.region 3 9) = "a.rs@3..9"
    ∧ anchorIdent "a.rs" .file = "a.rs" := by
  native_decide

/-- A declaration's identity is its file and its name, whatever lines it sits on
and however its body is laid out; a region's moves with its markers.

@proves REQ-ANCHOR.stable_under_move -/
theorem moving_a_declaration_keeps_its_identity (file name : String) :
    anchorIdent file (.decl name) = file ++ "::" ++ name ∧
    anchorIdent file .file = file := ⟨rfl, rfl⟩

/-- And an edit above a region, which moves its markers down a line, changes it.

@proves REQ-ANCHOR.stable_under_move -/
theorem an_edit_above_a_region_moves_it :
    anchorIdent "a.rs" (.region 3 9) ≠ anchorIdent "a.rs" (.region 4 10) := by
  native_decide

end TraceLean.Anchor
