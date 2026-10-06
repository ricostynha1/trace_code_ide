import TraceLean.Evidence

open Lean (Json ToJson FromJson toJson)

/-!
# Roll-up

Models `REQ-ROLLUP`. Where a wrong number does the most damage, because a
percentage is the one output people quote without reading what produced it.
-/

namespace TraceLean.Rollup

open TraceLean.Evidence

/--
Aggregate over children by taking the minimum.

An average invents a level nothing established; a minimum is always a level
something did.

@models REQ-ROLLUP.min_not_mean
@models ARCH-HONEST.weakest_link
-/
def combine (levels : List Level) : Level :=
  levels.foldl Level.min Level.L4 |> fun result =>
    if levels.isEmpty then Level.L1 else result

/-- Nothing to aggregate is the lowest, not the highest: an empty list is an
absence of evidence, and the top of the ladder is the last thing it should
report.

@proves REQ-ROLLUP.min_not_mean -/
theorem combine_empty : combine [] = Level.L1 := by
  simp [combine]

/-- A single level aggregates to itself. -/
theorem combine_singleton (l : Level) : combine [l] = Level.min Level.L4 l := by
  simp [combine]

/-! ## Rolling a declared graph up

Three properties, and each exists because the obvious implementation gets it
wrong in a way nobody notices.

**A decomposition nobody claimed complete yields a lower bound.** If half the
clauses of a requirement are written and both are met, the honest figure is
"at least 100% of what has been written down", not "100%". A figure that could
not be distinguished from an exact one would read as finished.

**An exemption leaves the denominator; a partial does not.** Exempting a clause
is saying it does not apply, so counting it would permanently cap the figure.
Marking one partial is saying it is half done, so removing it would raise the
figure by hiding work.

**The result does not depend on the order children are visited.** A requirement
reached twice through a diamond is visited once, and a cycle cannot loop.
-/

/-- A percentage is a rendering, so the figure itself is a fraction. Two
languages rounding one number differently would be a divergence about nothing.

@models REQ-ROLLUP.open_is_lower_bound
@models ARCH-HONEST.lower_bound_marked -/
structure Figure where
  met : Nat
  total : Nat
  /-- The author claimed the clauses exhaust the requirement. -/
  exact : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A requirement whose decomposition is unclaimed can never read as done.

@models REQ-ROLLUP.never_complete_when_open -/
def Figure.isComplete (f : Figure) : Bool := f.exact && f.total > 0 && f.met == f.total

/-- A requirement reduced to what a roll-up depends on. -/
structure Node where
  id : String
  complete : Bool
  clauses : List (Option String)
  exempt : List (Option String)
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Assurance of one node, and of everything under it. -/
inductive RollUp where
  | mk (id : String) (assurance : Level) (covered : Figure) (children : List RollUp)
  deriving Repr, Inhabited

def RollUp.assurance : RollUp → Level
  | .mk _ a _ _ => a

/-- Written by hand because a derived encoding for a recursive type would nest
its own constructor name at every level, and the implementation's is flat. -/
partial def RollUp.toJsonValue : RollUp → Json
  | .mk id assurance covered children =>
    Json.mkObj [("id", Json.str id), ("assurance", toJson assurance),
                ("covered", toJson covered),
                ("children", Json.arr (children.map RollUp.toJsonValue).toArray)]

instance : ToJson RollUp := ⟨RollUp.toJsonValue⟩

def levelAt (levels : List ((String × Option String) × Level)) (key : String × Option String)
    : Option Level :=
  (levels.find? (·.1 == key)).map (·.2)

/-- Through a map: a repeated identifier resolves the way the index resolves it,
not the way a list happens to be ordered. -/
def canonNodes (nodes : List Node) : List Node :=
  (nodes.foldl
    (fun acc n => if acc.any (·.id == n.id) then acc.map (fun m => if m.id == n.id then n else m)
                  else acc ++ [n]) []).mergeSort (fun a b => decide (a.id ≤ b.id))

def canonLevels (levels : List ((String × Option String) × Level))
    : List ((String × Option String) × Level) :=
  levels.foldl
    (fun acc kv => if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
                   else acc ++ [kv]) []

def figureFor (node : Option Node) (levels : List ((String × Option String) × Level))
    (floor : Level) : Figure :=
  match node with
  | none => { met := 0, total := 0, exact := false }
  | some n =>
    let counted := n.clauses.filter (fun c => !n.exempt.contains c)
    if counted.isEmpty then { met := 0, total := 0, exact := n.complete }
    else
      let met := counted.filter (fun c =>
        match levelAt levels (n.id, c) with
        | some l => decide (floor ≤ l)
        | none => false)
      { met := met.length, total := counted.length, exact := n.complete }

/-- The walk `rollUp` folds over the refinement graph, carrying the nodes it has
already visited so a diamond is counted once.

A top-level definition rather than a `let rec`, which the grammar that reads
these annotations cannot read (ADR-0008). -/
private def rollUpWalk (graph : List Node) (levels : List ((String × Option String) × Level))
    (floor : Level) : Nat → String → List String → RollUp × List String
  | 0, id, seen => (.mk id Level.L1 { met := 0, total := 0, exact := false } [], seen)
  | fuel + 1, id, seen =>
    let node := graph.find? (·.id == id)
    let own := match node with
      | none => []
      | some n => n.clauses.map (fun c => (levelAt levels (id, c)).getD Level.L1)
    -- Pairs are taken apart with `.1` and `.2` and the binder is left
    -- untyped: the grammar that reads these annotations reads neither a typed
    -- lambda binder nor a destructuring `let` in a branch (ADR-0008).
    let kids := graph.foldl
      (fun acc n =>
        if n.refines.contains id && !acc.2.contains n.id then
          let kid := rollUpWalk graph levels floor fuel n.id (acc.2 ++ [n.id])
          (acc.1 ++ [kid.1], kid.2)
        else acc)
      (([] : List RollUp), seen)
    (.mk id (combine (own ++ kids.1.map (·.assurance))) (figureFor node levels floor) kids.1,
      kids.2)

/--
Roll a declared graph up.

@models REQ-ROLLUP.min_not_mean
@models ARCH-HONEST.weakest_link
@models REQ-ROLLUP.open_is_lower_bound
@models REQ-ROLLUP.never_complete_when_open
@models ARCH-HONEST.lower_bound_marked
@models REQ-ROLLUP.exempt_leaves_denominator
@models REQ-ROLLUP.deterministic_order
-/
def rollUp (nodes : List Node) (levels : List ((String × Option String) × Level))
    (root : String) (floor : Level) : RollUp :=
  let graph := canonNodes nodes
  let levels := canonLevels levels
  (rollUpWalk graph levels floor (graph.length + 1) root [root]).1

/-- An unclaimed decomposition never renders as finished, whatever its figure.

@proves REQ-ROLLUP.never_complete_when_open -/
theorem an_unclaimed_decomposition_is_never_complete (met total : Nat) :
    Figure.isComplete { met := met, total := total, exact := false } = false := by
  simp [Figure.isComplete]

/-! ## Coverage over files

`untraced_counted`. A file nothing claims is in the denominator. The
alternative — counting only annotated files — is the figure that rises when
somebody deletes an annotation, and a coverage number that improves when you do
less work is worse than no number at all.

Exact, because the denominator is not a claim anybody made: it is the set of
files that were scanned, which is known.
-/

def dedupe (xs : List String) : List String :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

/-- How many of the files scanned are claimed by at least one annotation.

@models REQ-ROLLUP.untraced_counted -/
def fileCoverage (scanned claimed : List String) : Figure :=
  let scanned := dedupe scanned
  -- A claim about a file nothing scanned is not coverage of anything, so the
  -- numerator is an intersection rather than a count of claims.
  let claimed := (dedupe claimed).filter (scanned.contains ·)
  { met := claimed.length, total := scanned.length, exact := true }

/-- Scanning nothing covers nothing, and the figure says so rather than
reporting a vacuous hundred per cent.

@proves REQ-ROLLUP.untraced_counted -/
theorem nothing_scanned_is_not_complete :
    (fileCoverage [] []).isComplete = false := by
  native_decide

/-- A file nothing claims lowers the figure, which is the whole point.

@proves REQ-ROLLUP.untraced_counted -/
theorem an_unclaimed_file_is_counted :
    fileCoverage ["a.rs", "b.rs"] ["a.rs"] = { met := 1, total := 2, exact := true } := by
  native_decide

end TraceLean.Rollup
