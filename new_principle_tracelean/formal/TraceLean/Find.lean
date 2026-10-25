import Lean

namespace TraceLean.Find

def fold (c : Char) : Char := c.toLower

def dedupStars (xs : List Char) : List Char :=
  (xs.foldl (fun (acc : List Char) (c : Char) =>
    if c == '*' && acc.head? == some '*' then acc else c :: acc) []).reverse

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

/-- @models REQ-NAV.find_by_glob -/
def globMatches (text pattern : String) : List (Nat × Nat) :=
  let wanted := dedupStars (pattern.toList.map fold)
  if wanted.isEmpty then []
  else
    let lines := text.splitOn "\n"
    (lines.foldl (fun (acc : List (Nat × Nat) × Nat) (line : String) =>
      let chars := line.toList.map fold
      (acc.1 ++ scanLine wanted acc.2 chars.length 0 chars, acc.2 + chars.length + 1)) ([], 0)).1

/-- @models REQ-NAV.find_lists_lines -/
def matchingLines (text pattern : String) : List (Nat × String) :=
  ((text.splitOn "\n").enum.filter (fun (p : Nat × String) => !(globMatches p.2 pattern).isEmpty)).map
    (fun (p : Nat × String) => (p.1 + 1, p.2))

end TraceLean.Find
