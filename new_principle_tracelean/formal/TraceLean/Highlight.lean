namespace TraceLean.Highlight

def charAt (cs : List Char) (i : Nat) : Char :=
  cs.getD i ' '

def runWhile (cs : List Char) (stop : Nat) (keep : Char → Bool) : Nat → Nat → Nat
  | 0, i => i
  | fuel + 1, i => if i < stop && keep (charAt cs i) then runWhile cs stop keep fuel (i + 1) else i

def upperOrDigit (c : Char) : Bool := c.isUpper || c.isDigit

def keyChar (c : Char) : Bool := c.isLower || c.isDigit || c == '_'

def groups (cs : List Char) (stop : Nat) : Nat → Nat × Nat → Nat × Nat
  | 0, state => state
  | fuel + 1, state =>
    if state.1 + 1 < stop && charAt cs state.1 == '-' && upperOrDigit (charAt cs (state.1 + 1)) then
      groups cs stop fuel (runWhile cs stop upperOrDigit (stop + 1) (state.1 + 1), state.2 + 1)
    else state

def referenceAt (cs : List Char) (start stop : Nat) : Option Nat :=
  let capitals := runWhile cs stop Char.isUpper (stop + 1) start
  if capitals - start < 2 then none
  else
    let grouped := groups cs stop (stop + 1) (capitals, 0)
    if grouped.2 == 0 then none
    else
      let past := grouped.1
      let ended :=
        if past + 1 < stop && charAt cs past == '.' && (charAt cs (past + 1)).isLower then
          runWhile cs stop keyChar (stop + 1) (past + 1)
        else past
      if ended < stop && (charAt cs ended).isAlphanum then none else some ended

def boundary (cs : List Char) (first place : Nat) : Bool :=
  place == first || !(charAt cs (place - 1)).isAlphanum

def namesFrom (cs : List Char) (first stop : Nat) : Nat → Nat → List (Nat × Nat)
  | 0, _ => []
  | fuel + 1, place =>
    if place < stop then
      match (if boundary cs first place then referenceAt cs place stop else none) with
      | some ended => (place, ended) :: namesFrom cs first stop fuel ended
      | none => namesFrom cs first stop fuel (place + 1)
    else []

/-- @models REQ-SHOW.references_are_links -/
def requirementNames (text : String) (start stop : Nat) : List (Nat × Nat) :=
  let cs := text.toList
  let last := min stop cs.length
  let first := min start last
  namesFrom cs first last (last + 1) first

end TraceLean.Highlight
