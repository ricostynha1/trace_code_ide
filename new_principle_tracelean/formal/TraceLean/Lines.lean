import TraceLean.Strength

namespace TraceLean.Lines

open Lean (ToJson FromJson)
open TraceLean.Strength (sortedUnique)

structure TestLines where
  test : String
  file : String
  lines : List (Nat × Nat)
  deriving Repr, Inhabited, ToJson, FromJson

structure LineHits where
  line : Nat
  hits : Nat
  tests : List (String × Nat)
  deriving Repr, Inhabited, DecidableEq, ToJson, FromJson

structure Reach where
  run : Nat
  all : Nat
  tests : List String
  deriving Repr, Inhabited, ToJson, FromJson

def sum (xs : List Nat) : Nat := xs.foldl (· + ·) 0

def countOf (runs : List TestLines) (test : String) (line : Nat) : Nat :=
  sum (runs.filter (fun r => r.test == test) |>.map (fun r =>
    sum (r.lines.filter (fun p => p.1 == line) |>.map (·.2))))

/-- @models REQ-LINECOV.per_test -/
def merged (runs : List TestLines) (file : String) : List LineHits :=
  let here := runs.filter (fun r => r.file == file)
  let lines := sortedUnique (fun a b => decide (a ≤ b)) (here.bind (fun r => r.lines.map (·.1)))
  lines.map (fun line =>
    let ran := here.filter (fun r => r.lines.any (fun p => p.1 == line && p.2 > 0))
    let names := sortedUnique (fun a b => decide (a ≤ b)) (ran.map (·.test))
    let tests := names.map (fun t => (t, countOf (here.map (fun r =>
      { r with lines := r.lines.filter (fun p => p.2 > 0) })) t line))
    { line := line, hits := sum (tests.map (·.2)), tests := tests })

/-- @models REQ-LINECOV.stale_hidden -/
def visible (measured : Option (String × List LineHits)) (hash : String)
    : Option (List LineHits) :=
  match measured with
  | none => none
  | some m => if m.1 == hash then some m.2 else none

/-- @models REQ-LINECOV.clause_summary -/
def spanCoverage (lines : List LineHits) (start stop : Nat) : Reach :=
  let inside := lines.filter (fun l => start ≤ l.line && l.line ≤ stop)
  { run := (inside.filter (fun l => l.hits > 0)).length
    all := inside.length
    tests := sortedUnique (fun a b => decide (a ≤ b)) (inside.bind (fun l => l.tests.map (·.1))) }

def said (line : LineHits) : String :=
  if line.tests.isEmpty then "no test runs this line"
  else
    "run " ++ toString line.hits ++ " times by " ++
      ", ".intercalate (line.tests.map fun (t : String × Nat) => t.1 ++ " ×" ++ toString t.2)

structure Marker where
  line : Nat
  hits : Nat
  said : String
  deriving Repr, Inhabited, ToJson, FromJson

/-- @models REQ-LINECOV.uncovered_shown -/
def markers (lines : List LineHits) (top height : Nat) : List Marker :=
  (lines.filter fun l => l.line ≥ 1 && top ≤ l.line - 1 && l.line - 1 < top + height).map fun l =>
    { line := l.line - 1, hits := l.hits, said := said l }

/-- @proves REQ-LINECOV.stale_hidden -/
theorem only_coverage_of_this_text_is_shown (hash other : String) (lines : List LineHits) :
    visible (some (hash, lines)) hash = some lines ∧
    (other ≠ hash → visible (some (other, lines)) hash = none) ∧
    visible none hash = none := by
  refine ⟨?_, ?_, ?_⟩
  · simp [visible]
  · intro moved
    simp [visible, moved]
  · simp [visible]

end TraceLean.Lines
