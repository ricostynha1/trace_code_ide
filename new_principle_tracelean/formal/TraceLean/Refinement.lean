import Lean

namespace TraceLean.Refinement

open Lean (ToJson FromJson)

inductive Mark where
  | open_ | done
  deriving Repr, DecidableEq, Inhabited

abbrev Graph := List (String × List String)

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

structure Walk where
  path : List String
  next : List Nat
  marks : List (String × Mark)

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

def walk (g : Graph) : Nat → Walk → List (String × Mark) ⊕ List String
  | 0, w => .inl w.marks
  | fuel + 1, w =>
    if w.path.isEmpty then .inl w.marks
    else match step g w with
      | .inr cycle => .inr cycle
      | .inl w' => walk g fuel w'

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

def cycleIn (g : Graph) : Option (List String) :=
  let budget := g.length * 2 + (g.foldl (fun n e => n + e.2.length) 0) + 1
  searchFrom g budget (roots g) []

/-- @models REQ-REQDOC.refines_resolves -/
def danglingIn (g : Graph) : List (String × String) :=
  let pairs := g.foldl
    (fun acc e => acc ++ (e.2.filter (fun p => !hasNode g p)).map (fun p => (e.1, p))) []
  (pairs.foldl (fun acc p => if acc.contains p then acc else acc ++ [p]) []).mergeSort
    (fun a b => if a.1 != b.1 then decide (a.1 ≤ b.1) else decide (a.2 ≤ b.2))

structure GraphReport where
  cycle : Option (List String)
  dangling : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-REQDOC.refines_dag -/
def graphReport (edges : Graph) : GraphReport :=
  let g := canonGraph edges
  { cycle := cycleIn g, dangling := danglingIn g }

end TraceLean.Refinement
