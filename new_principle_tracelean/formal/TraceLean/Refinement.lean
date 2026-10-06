import Lean

/-!
# The refinement graph

Models the graph half of `REQ-REQDOC`. Requirements refine other requirements,
and two things can go wrong with that: a parent that does not exist, and a cycle.

They must not be confused. A parent that does not exist is a typo, reported as a
dangling reference; a cycle is a contradiction in what the project claims. An
implementation that treated a missing parent as a cycle would report a blocking
fault for a spelling mistake, and one that treated a cycle as merely dangling
would let a requirement justify itself.

The search is the implementation's: an explicit stack rather than recursion, and
the *first* cycle found in identifier order, because the answer is shown to a
person and has to be the same one on every run.
-/

namespace TraceLean.Refinement

open Lean (ToJson FromJson)

/-- Where the search has got to with a node. -/
inductive Mark where
  | open_ | done
  deriving Repr, DecidableEq, Inhabited

abbrev Graph := List (String × List String)

/-- Through a map, so a repeated identifier resolves the way the index resolves
it rather than the way a list happens to be ordered: later wins, sorted by
identifier. -/
def canonGraph (edges : Graph) : Graph :=
  (edges.foldl
    (fun acc kv =>
      if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
      else acc ++ [kv]) []).mergeSort (fun a b => decide (a.1 ≤ b.1))

def parentsOf (g : Graph) (node : String) : List String :=
  (g.find? (·.1 == node)).map (·.2) |>.getD []

def hasNode (g : Graph) (node : String) : Bool := g.any (·.1 == node)

def markOf (marks : List (String × Mark)) (node : String) : Option Mark :=
  (marks.find? (·.1 == node)).map (·.2)

def setMark (marks : List (String × Mark)) (node : String) (m : Mark) : List (String × Mark) :=
  if marks.any (·.1 == node) then marks.map (fun p => if p.1 == node then (node, m) else p)
  else marks ++ [(node, m)]

/-- The state of one depth-first walk: the path being explored, how far into
each node's parents the walk has got, and what every node is marked. -/
structure Walk where
  path : List String
  next : List Nat
  marks : List (String × Mark)

/-- One step of the walk, or the cycle it found. -/
def step (g : Graph) (w : Walk) : Walk ⊕ List String :=
  match w.path.reverse, w.next.reverse with
  | node :: _, index :: _ =>
    let parents := parentsOf g node
    if index ≥ parents.length then
      .inl { path := w.path.dropLast, next := w.next.dropLast,
             marks := setMark w.marks node .done }
    else
      let w := { w with next := w.next.dropLast ++ [index + 1] }
      let parent := parents.getD index ""
      -- A parent that does not exist is a dangling reference, reported
      -- separately; it is not a cycle.
      if !hasNode g parent then .inl w
      else
        match markOf w.marks parent with
        | some .open_ =>
          let start := (w.path.findIdx? (· == parent)).getD 0
          .inr ((w.path.drop start) ++ [parent])
        | some .done => .inl w
        | none =>
          .inl { path := w.path ++ [parent], next := w.next ++ [0],
                 marks := setMark w.marks parent .open_ }
  | _, _ => .inl w

/-- Walk until the path empties or a cycle appears. -/
def walk (g : Graph) : Nat → Walk → List (String × Mark) ⊕ List String
  | 0, w => .inl w.marks
  | fuel + 1, w =>
    if w.path.isEmpty then .inl w.marks
    else match step g w with
      | .inr cycle => .inr cycle
      | .inl w' => walk g fuel w'

/-- Every root, in identifier order. -/
def roots (g : Graph) : List String := g.map (·.1)

def searchFrom (g : Graph) (fuel : Nat)
    : List String → List (String × Mark) → Option (List String)
  | [], _ => none
  | root :: rest, marks =>
    if markOf marks root == some .done then searchFrom g fuel rest marks
    else
      match walk g fuel { path := [root], next := [0], marks := setMark marks root .open_ } with
      | .inr cycle => some cycle
      | .inl marks' => searchFrom g fuel rest marks'

/--
A cycle in the refinement graph, as the identifiers on it.

@models REQ-REQDOC.refines_dag
-/
def cycleIn (g : Graph) : Option (List String) :=
  -- Each node is opened once and closed once, and each edge is followed once,
  -- so the total number of steps is bounded by nodes plus edges.
  let budget := g.length * 2 + (g.foldl (fun n e => n + e.2.length) 0) + 1
  searchFrom g budget (roots g) []

/-- Identifiers named by `refines:` that no document declares, sorted and
without repeats.

@models REQ-REQDOC.refines_resolves -/
def danglingIn (g : Graph) : List (String × String) :=
  let pairs := g.foldl
    (fun acc e => acc ++ (e.2.filter (fun p => !hasNode g p)).map (fun p => (e.1, p))) []
  (pairs.foldl (fun acc p => if acc.contains p then acc else acc ++ [p]) []).mergeSort
    (fun a b => if a.1 != b.1 then decide (a.1 ≤ b.1) else decide (a.2 ≤ b.2))

/-- Both graph questions at once. -/
structure GraphReport where
  cycle : Option (List String)
  dangling : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-REQDOC.refines_dag
@models REQ-REQDOC.refines_resolves -/
def graphReport (edges : Graph) : GraphReport :=
  let g := canonGraph edges
  { cycle := cycleIn g, dangling := danglingIn g }

/-- A requirement that refines itself is a cycle of one.

@proves REQ-REQDOC.refines_dag -/
theorem self_refinement_is_a_cycle :
    (graphReport [("REQ-A", ["REQ-A"])]).cycle = some ["REQ-A", "REQ-A"] := by
  native_decide

/-- A parent nobody declares is dangling, and is not a cycle.

@proves REQ-REQDOC.refines_resolves -/
theorem a_missing_parent_is_dangling_not_a_cycle :
    graphReport [("REQ-A", ["REQ-GONE"])] =
      { cycle := none, dangling := [("REQ-A", "REQ-GONE")] } := by
  native_decide

end TraceLean.Refinement
