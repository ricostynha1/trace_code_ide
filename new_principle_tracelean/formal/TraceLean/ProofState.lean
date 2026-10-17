import Lean

/-!
# The proof state shown

Models `REQ-LSP.proof_state_shown`, the part that is a value: what the panel
says of the goals Lean's server reported. Which goals, and where, is the
server's; what is shown of them is this.

Written in the subset the grammar reads (ADR-0008).
-/

namespace TraceLean.ProofState

/-- What the panel says. `answered` is whether the server gave a result at all.
Each goal, a blank line between them; that there is none where the cursor is;
or, when the server gave no list of goals, its own rendering.

@models REQ-LSP.proof_state_shown -/
def goalsShown (answered : Bool) (goals : Option (List String)) (rendered : Option String) : String :=
  if answered then
    match goals with
    | some gs =>
      if gs.isEmpty then "no goals" else String.intercalate "\n\n" gs
    | none =>
      match rendered with
      | some text => text
      | none => "no goals here"
  else "no goals here"

end TraceLean.ProofState
