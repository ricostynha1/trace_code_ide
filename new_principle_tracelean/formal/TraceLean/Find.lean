import Lean

/-!
# Finding in a buffer

Models `REQ-NAV.find_by_glob` and `REQ-NAV.find_lists_lines`: a text with `*`
and `?` matched within a line, and the lines that hold a match.

Case is folded for the letters a to z only, so that offsets stay the offsets
of the text itself and the two sides agree on every character.

Written in the subset the grammar reads (ADR-0008): no brace in a string, no
`where`, no `·` shorthand, no named patterns.
-/

namespace TraceLean.Find

/-- A character folded for matching: a to z only. -/
def fold (c : Char) : Char := c.toLower

/-- `**` is `*`. -/
def dedupStars (xs : List Char) : List Char :=
  (xs.foldl (fun (acc : List Char) (c : Char) =>
    if c == '*' && acc.head? == some '*' then acc else c :: acc) []).reverse

/-- How many characters the shortest match of the pattern at the start of the
text takes, if there is one. The fuel is more than the pattern and the text
together, which is as deep as it goes. -/
def shortest : Nat → List Char → List Char → Option Nat
  | 0, _, _ => none
  | fuel + 1, [], _ => some 0
  | fuel + 1, p :: rest, text =>
    if p == '*' then
      match shortest fuel rest text with
      | some n => some n
      | none =>
        match text with
        | [] => none
        | t :: tail => (shortest fuel (p :: rest) tail).map (fun n => n + 1)
    else
      match text with
      | [] => none
      | t :: tail =>
        if p == '?' || p == t then (shortest fuel rest tail).map (fun n => n + 1) else none

/-- The matches in one line, left to right and without overlap, as the offsets
of their starts and ends in the whole text. A match is never empty. -/
def scanLine (pattern : List Char) (lineStart : Nat) : Nat → Nat → List Char → List (Nat × Nat)
  | 0, _, _ => []
  | fuel + 1, at_, [] => []
  | fuel + 1, at_, text =>
    match shortest (pattern.length + text.length + 1) pattern text with
    | some length =>
      if length > 0 then
        (lineStart + at_, lineStart + at_ + length) :: scanLine pattern lineStart fuel (at_ + length) (text.drop length)
      else scanLine pattern lineStart fuel (at_ + 1) text.tail
    | none => scanLine pattern lineStart fuel (at_ + 1) text.tail

/-- Every place the pattern matches in the text: `*` is any run of characters
and `?` any one, within a line; case of a to z is ignored.

@models REQ-NAV.find_by_glob -/
def globMatches (text pattern : String) : List (Nat × Nat) :=
  let wanted := dedupStars (pattern.toList.map fold)
  if wanted.isEmpty then []
  else
    let lines := text.splitOn "\n"
    (lines.foldl (fun (acc : List (Nat × Nat) × Nat) (line : String) =>
      let chars := line.toList.map fold
      (acc.1 ++ scanLine wanted acc.2 chars.length 0 chars, acc.2 + chars.length + 1)) ([], 0)).1

/-- The lines of the text holding a match, each with its number from 1.

@models REQ-NAV.find_lists_lines -/
def matchingLines (text pattern : String) : List (Nat × String) :=
  ((text.splitOn "\n").enum.filter (fun (p : Nat × String) => !(globMatches p.2 pattern).isEmpty)).map
    (fun (p : Nat × String) => (p.1 + 1, p.2))

end TraceLean.Find
