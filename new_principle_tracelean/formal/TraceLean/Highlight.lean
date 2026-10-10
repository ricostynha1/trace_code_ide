/-!
# Requirement names in a comment

Models `REQ-SHOW.references_are_links`: where a comment names a requirement, so
that the name can be marked and opened. A name is two or more capitals, then `-`
and capitals or digits as many times as it likes, then optionally `.` and a
lower-case clause key, and it runs into nothing alphanumeric.
-/

namespace TraceLean.Highlight

/-- The character at `i`; only asked below `stop`, which is never past the end. -/
def charAt (cs : List Char) (i : Nat) : Char :=
  cs.getD i ' '

/-- Past every character from `i` that `keep` accepts, before `stop`. -/
def runWhile (cs : List Char) (stop : Nat) (keep : Char → Bool) : Nat → Nat → Nat
  | 0, i => i
  | fuel + 1, i => if i < stop && keep (charAt cs i) then runWhile cs stop keep fuel (i + 1) else i

def upperOrDigit (c : Char) : Bool := c.isUpper || c.isDigit

def keyChar (c : Char) : Bool := c.isLower || c.isDigit || c == '_'

/-- `-` and a group of capitals or digits, as many times as they follow:
where they end, and how many there were. -/
def groups (cs : List Char) (stop : Nat) : Nat → Nat × Nat → Nat × Nat
  | 0, state => state
  | fuel + 1, state =>
    if state.1 + 1 < stop && charAt cs state.1 == '-' && upperOrDigit (charAt cs (state.1 + 1)) then
      groups cs stop fuel (runWhile cs stop upperOrDigit (stop + 1) (state.1 + 1), state.2 + 1)
    else state

/-- Where a requirement's name starting at `start` ends, if one does. -/
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

/-- Whether a name may start at `place`: the range's start, or after a
character that is not alphanumeric. -/
def boundary (cs : List Char) (first place : Nat) : Bool :=
  place == first || !(charAt cs (place - 1)).isAlphanum

/-- Every name from `place` on, before `stop`. -/
def namesFrom (cs : List Char) (first stop : Nat) : Nat → Nat → List (Nat × Nat)
  | 0, _ => []
  | fuel + 1, place =>
    if place < stop then
      match (if boundary cs first place then referenceAt cs place stop else none) with
      | some ended => (place, ended) :: namesFrom cs first stop fuel ended
      | none => namesFrom cs first stop fuel (place + 1)
    else []

/-- Every requirement name in characters `start` to `stop` of `text`, as
character ranges; a range past the end is cut at the end.

@models REQ-SHOW.references_are_links -/
def requirementNames (text : String) (start stop : Nat) : List (Nat × Nat) :=
  let cs := text.toList
  let last := min stop cs.length
  let first := min start last
  namesFrom cs first last (last + 1) first

end TraceLean.Highlight
