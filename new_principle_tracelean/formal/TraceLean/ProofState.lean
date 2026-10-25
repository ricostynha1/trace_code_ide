import Lean

namespace TraceLean.ProofState

/-- @models REQ-LSP.proof_state_shown -/
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
