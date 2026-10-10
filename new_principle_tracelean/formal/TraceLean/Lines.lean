import TraceLean.Strength

/-!
# Line coverage, test by test

Models `REQ-LINECOV`. Each test runs alone, so a line's coverage is a table of
which tests ran it and how often; a clause's summary is how many executable
lines of its implementing items some test ran, and by which tests.
-/

namespace TraceLean.Lines

open Lean (ToJson FromJson)
open TraceLean.Strength (sortedUnique)

/-- One test's counts for one file. -/
structure TestLines where
  test : String
  file : String
  lines : List (Nat × Nat)
  deriving Repr, Inhabited, ToJson, FromJson

/-- One executable line: how often it ran, and by which tests. -/
structure LineHits where
  line : Nat
  hits : Nat
  tests : List (String × Nat)
  deriving Repr, Inhabited, DecidableEq, ToJson, FromJson

/-- How much of a stretch of lines ran. -/
structure Reach where
  run : Nat
  all : Nat
  tests : List String
  deriving Repr, Inhabited, ToJson, FromJson

def sum (xs : List Nat) : Nat := xs.foldl (· + ·) 0

/-- What `test` contributed to `line`, over its runs. -/
def countOf (runs : List TestLines) (test : String) (line : Nat) : Nat :=
  sum (runs.filter (fun r => r.test == test) |>.map (fun r =>
    sum (r.lines.filter (fun p => p.1 == line) |>.map (·.2))))

/-- Every test's counts for `file`, folded into its executable lines.

@models REQ-LINECOV.per_test -/
def merged (runs : List TestLines) (file : String) : List LineHits :=
  let here := runs.filter (fun r => r.file == file)
  let lines := sortedUnique (fun a b => decide (a ≤ b)) (here.bind (fun r => r.lines.map (·.1)))
  lines.map (fun line =>
    let ran := here.filter (fun r => r.lines.any (fun p => p.1 == line && p.2 > 0))
    let names := sortedUnique (fun a b => decide (a ≤ b)) (ran.map (·.test))
    let tests := names.map (fun t => (t, countOf (here.map (fun r =>
      { r with lines := r.lines.filter (fun p => p.2 > 0) })) t line))
    { line := line, hits := sum (tests.map (·.2)), tests := tests })

/-- What is shown of a file's measured lines: them when they were measured
against the text the file has now, nothing when against other text.

@models REQ-LINECOV.stale_hidden -/
def visible (measured : Option (String × List LineHits)) (hash : String)
    : Option (List LineHits) :=
  match measured with
  | none => none
  | some m => if m.1 == hash then some m.2 else none

/-- How much of lines `start` to `stop` ran.

@models REQ-LINECOV.clause_summary -/
def spanCoverage (lines : List LineHits) (start stop : Nat) : Reach :=
  let inside := lines.filter (fun l => start ≤ l.line && l.line ≤ stop)
  { run := (inside.filter (fun l => l.hits > 0)).length
    all := inside.length
    tests := sortedUnique (fun a b => decide (a ≤ b)) (inside.bind (fun l => l.tests.map (·.1))) }

/-- What a line's marker says when pointed at. -/
def said (line : LineHits) : String :=
  if line.tests.isEmpty then "no test runs this line"
  else
    "run " ++ toString line.hits ++ " times by " ++
      ", ".intercalate (line.tests.map fun (t : String × Nat) => t.1 ++ " ×" ++ toString t.2)

/-- A measured line as the editor marks it: the zero-based buffer line, how
often tests ran it, and what pointing at it says. -/
structure Marker where
  line : Nat
  hits : Nat
  said : String
  deriving Repr, Inhabited, ToJson, FromJson

/-- The markers of the measured lines in a window of `height` buffer lines
from `top`; a line numbered 0 is no line and has none.

@models REQ-LINECOV.uncovered_shown -/
def markers (lines : List LineHits) (top height : Nat) : List Marker :=
  (lines.filter fun l => l.line ≥ 1 && top ≤ l.line - 1 && l.line - 1 < top + height).map fun l =>
    { line := l.line - 1, hits := l.hits, said := said l }

/-- A line nobody ran has no tests.

@proves REQ-LINECOV.per_test -/
theorem a_line_no_test_ran_has_nobody :
    merged [⟨"t", "a.rs", [(4, 0)]⟩] "a.rs" = [⟨4, 0, []⟩] := by
  native_decide

end TraceLean.Lines
