import TraceLean.RequirementView

/-!
# Which lines of a requirement's code tests run

Models `REQ-LINECOV.lines_listed`: what a coverage count opens. Item by item,
the tests that ran it with how often in all, and every executable line no test
ran, as a link to it with its source.
-/

namespace TraceLean.CoverageView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Layout
open TraceLean.Lines (LineHits)
open TraceLean.RequirementView (Covered coveredRole coveredText total)
open TraceLean.Strength (sortedUnique)

/-- One item implementing the requirement, measured against its current text. -/
structure Item where
  path : String
  start : Nat
  symbol : Option String
  clause : Option String
  lines : List LineHits
  text : List String
  deriving Repr, Inhabited, ToJson, FromJson

/-- A count as the page says it: whole in green, short in red, nothing to run
in neither. -/
def countLine (out : Lines) (lead : String) (c : Covered) : Lines :=
  let role := coveredRole c
  if lead == "" then line out [(coveredText c, role, [])]
  else line out [(lead, Role.plain, []), (coveredText c, role, [])]

/-- How often `test` ran any line of the item. -/
def ranBy (lines : List LineHits) (test : String) : Nat :=
  TraceLean.Lines.sum (lines.map (fun l => TraceLean.Lines.sum ((l.tests.filter (·.1 == test)).map (·.2))))

def testLine (lines : List LineHits) (out : Lines) (test : String) : Lines :=
  line out [("  ran by  ", Role.plain, []), (test, Role.token .function, []),
            (" ×" ++ toString (ranBy lines test), Role.plain, [])]

/-- The source of line `n` of an item from `start`; none before it. -/
def sourceAt (item : Item) (n : Nat) : String :=
  if n < item.start then "" else ((item.text.get? (n - item.start)).getD "").trim

def unrunLine (item : Item) (out : Lines) (l : LineHits) : Lines :=
  line out [("    ", Role.plain, []), (item.path ++ ":" ++ toString l.line, Role.path, ["file.open"]),
            ("  " ++ sourceAt item l.line, Role.plain, [])]

def itemLines (out : Lines) (item : Item) : Lines :=
  let link := (item.path ++ ":" ++ toString item.start, Role.path, ["file.open"])
  let head :=
    match item.symbol with
    | some s => [(s, Role.token .function, []), ("  ", Role.plain, []), link]
    | none => [link]
  let out := countLine (line (blank out) head) "  " (total [(item.path, item.lines)])
  let tests := sortedUnique (fun a b => decide (a ≤ b)) (item.lines.bind (fun l => l.tests.map (·.1)))
  let out := tests.foldl (testLine item.lines) out
  let unrun := item.lines.filter (fun l => l.hits == 0)
  let out := if unrun.isEmpty then out else line out [("  no test runs", Role.removed, [])]
  unrun.foldl (unrunLine item) out

/-- The coverage of `named`, as a record titled `coverage <named>`.

@models REQ-LINECOV.lines_listed -/
def coverageView (named : String) (items : List Item) (width : Nat) : Buffer :=
  let out := line (start width) [("Coverage of ", Role.heading, []), (named, Role.requirement, ["trace.requirement"])]
  if items.isEmpty then
    finish (wrapped out "" "Nothing implementing it was measured against the text it has now. Run `tracelean-trace . --coverage`." Role.plain []) ("coverage " ++ named)
  else
    let out := countLine out "" (total (items.map (fun i => (i.path, i.lines))))
    finish (items.foldl itemLines out) ("coverage " ++ named)

end TraceLean.CoverageView
