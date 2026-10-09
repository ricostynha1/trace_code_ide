import Lean

/-!
# What an agent needs to see to change a requirement

Models `REQ-CONTEXT`: the neighbourhood of a requirement in the refinement
graph, and which parts of its context a person's choice puts in the copied
text.
-/

open Lean

namespace TraceLean.Context

/-- A requirement as the refinement graph sees it: its name and its parents. -/
structure Node where
  id : String
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What `id` refines, one step up. A name declared twice refines what both
declarations say. -/
def parentsOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (·.id == id)).bind (·.refines)

/-- What refines `id`, one step down. -/
def childrenOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (fun n => n.refines.contains id)).map (·.id)

/-- Every name `next` reaches from the queue, added to `seen` once each. The
fuel is spent one queued name at a time, and no name is queued twice. -/
def reach (next : String → List String) : Nat → List String → List String → List String
  | 0, _, seen => seen
  | _ + 1, [], seen => seen
  | fuel + 1, x :: rest, seen =>
    let fresh := ((next x).filter (fun y => !seen.contains y)).eraseDups
    reach next fuel (rest ++ fresh) (seen ++ fresh)

/-- Enough fuel to queue every name the graph mentions. -/
def fuelOf (nodes : List Node) : Nat :=
  nodes.length + (nodes.map (·.refines.length)).foldl (· + ·) 0 + 1

def closure (next : String → List String) (nodes : List Node) (id : String) : List String :=
  ((reach next (fuelOf nodes) [id] [id]).filter (· != id)).mergeSort (fun a b => decide (a ≤ b))

/-- Everything `id` refines, transitively, each once, in name order.

@models REQ-CONTEXT.neighbourhood_is_closed -/
def ancestors (nodes : List Node) (id : String) : List String :=
  closure (parentsOf nodes) nodes id

/-- Everything that refines `id`, transitively, each once, in name order. -/
def descendants (nodes : List Node) (id : String) : List String :=
  closure (childrenOf nodes) nodes id

/-- One part of a context. -/
inductive Part where
  | requirement | refines | refinedBy | code | tests | models | affected
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The parts, in the one order they are shown and copied in. -/
def allParts : List Part :=
  [.requirement, .refines, .refinedBy, .code, .tests, .models, .affected]

/-- The parts the copied text holds: those included that have something in
them, in the fixed order — whatever order they were chosen in.

@models REQ-CONTEXT.person_chooses -/
def partsShown (included : List Part) (filled : List Part) : List Part :=
  allParts.filter (fun p => included.contains p && filled.contains p)

/-- What the view calls a part. -/
def label : Part → String
  | .requirement => "requirement"
  | .refines => "refines"
  | .refinedBy => "refined by"
  | .code => "code"
  | .tests => "tests"
  | .models => "models"
  | .affected => "affected tests"

/-- The part a label names, if it names one.

@models REQ-CONTEXT.part_named -/
def partNamed (name : String) : Option Part :=
  allParts.find? (fun p => label p == name)

/-- A part is shown only if it was included.

@proves REQ-CONTEXT.person_chooses -/
theorem shown_were_included (included filled : List Part) (p : Part)
    (h : p ∈ partsShown included filled) : p ∈ included := by
  unfold partsShown at h
  have := (List.mem_filter.mp h).2
  simp [Bool.and_eq_true] at this
  exact this.1

/-- Every label names its own part back.

@proves REQ-CONTEXT.part_named -/
theorem labels_round_trip : allParts.all (fun p => partNamed (label p) == some p) = true := by
  native_decide

end TraceLean.Context
