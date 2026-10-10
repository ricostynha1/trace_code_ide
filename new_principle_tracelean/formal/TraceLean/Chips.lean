import TraceLean.Annotation

/-!
# Claims beside a requirement's clauses

Models `REQ-SHOW.claims_beside_code` where the text is a requirement's own
document: each clause's line in its frontmatter is marked with a letter for
each kind of claim on that clause, each mark opening the clause. Where the text
is code, which declaration a claim sits on is the annotation grammar's answer,
modelled under `REQ-ANNOT`.
-/

namespace TraceLean.Chips

open Lean (ToJson FromJson)
open TraceLean.Annotation

/-- One mark: the zero-based line, its letter, and the clause it opens. -/
structure Chip where
  line : Nat
  letter : String
  requirement : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The letter a role is marked with; a pin is a record, not a claim, and has
none, and a specification is the other half of a model. -/
def letter : Role → Option String
  | .models => some "M"
  | .specifies => some "M"
  | .implements => some "I"
  | .tests => some "T"
  | .drt => some "D"
  | .proves => some "P"
  | .pins => none

/-- A text's lines as Rust's `lines` reads them: no empty last line after a
final newline, and a carriage return before a newline dropped. -/
def textLines (text : String) : List String :=
  let pieces := text.splitOn "\n"
  let pieces := if pieces.getLast? == some "" then pieces.dropLast else pieces
  pieces.map fun line => if line.endsWith "\r" then line.dropRight 1 else line

/-- The clause key on a frontmatter line: indented by exactly two spaces, then
`key:`, the key one word. -/
def clauseKey (row : String) : Option String :=
  if row.startsWith "  " then
    let rest := row.drop 2
    if rest.startsWith " " || rest.startsWith "\t" then none
    else
      match rest.splitOn ":" with
      | key :: _ :: _ =>
        let key := key.trim
        if key.isEmpty || key.contains ' ' then none else some key
      | _ => none
  else none

def clausesFrom : List String → Nat → Nat → List (Nat × String)
  | [], _, _ => []
  | row :: rest, line, fences =>
    if row.trim == "---" then
      if fences + 1 == 2 then [] else clausesFrom rest (line + 1) (fences + 1)
    else
      match clauseKey row with
      | some key => (line, key) :: clausesFrom rest (line + 1) fences
      | none => clausesFrom rest (line + 1) fences

/-- Each clause of the frontmatter, as its zero-based line and key; reading
stops at the second `---`. -/
def clauseLines (text : String) : List (Nat × String) :=
  clausesFrom (textLines text) 0 0

/-- The marks of a requirement's own document: on each clause's line, one
letter for each kind of claim on that clause, in `M I T D P` order.

@models REQ-SHOW.claims_beside_code -/
def clauseChips (text id : String) (claims : List (Option String × Role)) : List Chip :=
  (clauseLines text).bind fun (pair : Nat × String) =>
    let letters := (claims.filter (·.1 == some pair.2)).filterMap (letter ·.2)
    (["M", "I", "T", "D", "P"].filter letters.contains).map fun l =>
      { line := pair.1, letter := l, requirement := id ++ "." ++ pair.2 }

/-- Each clause's line is marked with a letter for each kind of claim on it, in
one order, and each mark opens that clause.

@proves REQ-SHOW.claims_beside_code -/
theorem each_clause_line_carries_its_claims :
    clauseChips "---\nid: REQ-A\nclauses:\n  one: x\n  two: y\n---\n" "REQ-A"
        [(some "one", .tests), (some "one", .implements), (some "two", .models)]
      = [⟨3, "I", "REQ-A.one"⟩, ⟨3, "T", "REQ-A.one"⟩, ⟨4, "M", "REQ-A.two"⟩] := by
  native_decide

end TraceLean.Chips
