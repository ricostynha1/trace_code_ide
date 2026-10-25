import TraceLean.Command

namespace TraceLean.Tree

open TraceLean.Command

open Lean (ToJson FromJson)

structure Node where
  id : Nat
  command : Command
  inverse : Command
  parent : Option Nat
  children : List Nat
  deriving Repr, Inhabited, ToJson, FromJson

structure Tree where
  base : Workspace
  nodes : List Node
  current : Option Nat
  nextId : Nat
  deriving Repr, Inhabited, ToJson, FromJson

namespace Tree

def mk' (base : Workspace) : Tree :=
  { base := base.canon, nodes := [], current := none, nextId := 0 }

def node? (t : Tree) (id : Nat) : Option Node :=
  t.nodes.find? (·.id == id)

def ids (t : Tree) : List Nat := t.nodes.map (·.id)

def ancestryFrom : Nat → Tree → Option Nat → List Nat
  | 0, _, _ => []
  | _ + 1, _, none => []
  | fuel + 1, t, some id =>
    match t.node? id with
    | none => []
    | some n => ancestryFrom fuel t n.parent ++ [id]

def ancestry (t : Tree) (id : Nat) : List Nat :=
  ancestryFrom (t.nodes.length + 1) t (some id)

def stateAt (t : Tree) (id : Nat) : Except Refusal Workspace :=
  (t.ancestry id).foldl
    (fun acc nodeId =>
      match acc with
      | .error e => .error e
      | .ok w =>
        match t.node? nodeId with
        | none => .ok w
        | some n => apply w n.command)
    (.ok t.base)

def preview (t : Tree) (id : Nat) : Except Refusal (Workspace × Workspace) :=
  match t.node? id with
  | none => .error (.noSuchFile "")
  | some n =>
    let before :=
      match n.parent with
      | none => Except.ok t.base
      | some parent => t.stateAt parent
    match before with
    | .error e => .error e
    | .ok w =>
      match apply w n.command with
      | .error e => .error e
      | .ok after => .ok (w, after)

def state (t : Tree) : Except Refusal Workspace :=
  match t.current with
  | none => .ok t.base
  | some id => t.stateAt id

def push (t : Tree) (command : Command) : Tree :=
  match t.state with
  | .error _ => t
  | .ok w =>
    match apply w command with
    | .error _ => t
    | .ok _ =>
      let id := t.nextId
      let parent := t.current
      let node : Node :=
        { id := id, command := command, inverse := inverse command,
          parent := parent, children := [] }
      let nodes :=
        t.nodes.map (fun n =>
          if some n.id == parent then { n with children := n.children ++ [id] } else n)
      { t with nodes := nodes ++ [node], current := some id, nextId := id + 1 }

def undo (t : Tree) : Tree :=
  match t.current with
  | none => t
  | some id =>
    match t.node? id with
    | none => t
    | some n => { t with current := n.parent }

def roots (t : Tree) : List Nat :=
  (t.nodes.filter (·.parent.isNone)).map (·.id)

def maximum? : List Nat → Option Nat
  | [] => none
  | x :: rest => some (rest.foldl Nat.max x)

def redo (t : Tree) : Tree :=
  let children :=
    match t.current with
    | none => t.roots
    | some id => match t.node? id with
      | none => []
      | some n => n.children
  match maximum? children with
  | none => t
  | some target => { t with current := some target }

def sharedPrefix : List Nat → List Nat → Nat
  | a :: as, b :: bs => if a == b then 1 + sharedPrefix as bs else 0
  | _, _ => 0

def jumpTo (t : Tree) (target : Nat) : Except Refusal Workspace × Tree :=
  let from_ := match t.current with
    | none => []
    | some id => t.ancestry id
  let to := t.ancestry target
  let shared := sharedPrefix from_ to
  let up := (from_.drop shared).reverse
  let down := to.drop shared
  let start := t.state
  let afterUp := up.foldl
    (fun acc nodeId =>
      match acc with
      | .error e => .error e
      | .ok w => match t.node? nodeId with
        | none => .ok w
        | some n => apply w n.inverse)
    start
  let reached := down.foldl
    (fun acc nodeId =>
      match acc with
      | .error e => .error e
      | .ok w => match t.node? nodeId with
        | none => .ok w
        | some n => apply w n.command)
    afterUp
  let moved :=
    if (t.nodes.any (·.id == target)) then { t with current := some target }
    else { t with current := none }
  (reached, moved)

end Tree

inductive Movement where
  | undo
  | redo
  | jump (node : Nat)
  deriving Repr, Inhabited, ToJson, FromJson

inductive Step where
  | push (command : Command)
  | move (movement : Movement)
  deriving Repr, Inhabited, ToJson, FromJson

structure Report where
  nodes : List Nat
  current : Option Nat
  travelled : List Outcome
  replayed : List Outcome
  previews : List (Outcome × Outcome)
  currentAfterPreviews : Option Nat
  deriving Repr, Inhabited, ToJson, FromJson

def outcomeOf : Except Refusal Workspace → Outcome
  | .ok w => .ok w
  | .error r => .refused r.kind

def fromScript (base : Workspace) (script : List Step) : Tree :=
  script.foldl
    (fun t step =>
      match step with
      | .push c => t.push c
      | .move .undo => t.undo
      | .move .redo => t.redo
      | .move (.jump n) => (t.jumpTo n).2)
    (Tree.mk' base)

/-- @models REQ-UNDO.jump_equivalence
@models REQ-UNDO.path_via_ancestor
@models REQ-UNDO.reachable
@models REQ-UNDO.no_loss_on_branch
@models REQ-UNDO.preview_is_pure -/
def runScript (base : Workspace) (script : List Step) : Report :=
  let final := fromScript base script
  let ids := final.ids
  { nodes := ids,
    current := final.current,
    travelled := ids.map (fun id => outcomeOf (final.jumpTo id).1),
    replayed := ids.map (fun id => outcomeOf (final.stateAt id)),
    previews := ids.map (fun id =>
      match final.preview id with
      | .ok (before, after) => (Outcome.ok before, Outcome.ok after)
      | .error r => (Outcome.refused r.kind, Outcome.refused r.kind)),
    currentAfterPreviews := final.current }

/-- @proves REQ-UNDO.jump_equivalence -/
theorem an_empty_history_has_nothing_to_disagree_about (base : Workspace) :
    (runScript base []).travelled = (runScript base []).replayed := by
  rfl

def one : Workspace := ⟨[("a.rs", "hello")]⟩

def branching : List Step :=
  [.push (.insert "a.rs" 0 "x"), .move .undo, .push (.insert "a.rs" 0 "y")]

/-- @proves REQ-UNDO.no_loss_on_branch -/
theorem editing_after_undo_branches :
    (runScript one branching).nodes = [0, 1] ∧
    (runScript one branching).current = some 1 ∧
    (((fromScript one branching).node? 1).map (·.parent)) = some none ∧
    (runScript one branching).replayed
      = [.ok ⟨[("a.rs", "xhello")]⟩, .ok ⟨[("a.rs", "yhello")]⟩] := by
  native_decide

/-- @proves REQ-UNDO.reachable -/
theorem no_node_is_ever_dropped (t : Tree) (c : Command) :
    ((t.push c).ids = t.ids ∨ (t.push c).ids = t.ids ++ [t.nextId]) ∧
    t.undo.ids = t.ids ∧ t.redo.ids = t.ids ∧ (t.jumpTo 0).2.ids = t.ids := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · unfold Tree.push
    split
    · exact Or.inl rfl
    · split
      · exact Or.inl rfl
      · apply Or.inr
        simp only [Tree.ids, List.map_append, List.map_map]
        congr 1
        apply List.map_congr_left
        intro n _
        simp only [Function.comp]
        split <;> rfl
  · unfold Tree.undo
    split
    · rfl
    · split <;> rfl
  · unfold Tree.redo
    simp only
    split <;> rfl
  · unfold Tree.jumpTo
    simp only
    split <;> rfl

/-- @proves REQ-UNDO.path_via_ancestor -/
theorem a_jump_goes_through_the_common_ancestor :
    outcomeOf ((fromScript one branching).jumpTo 0).1 = .ok ⟨[("a.rs", "xhello")]⟩ ∧
    Tree.sharedPrefix [0, 2] [0, 1] = 1 ∧
    (let t := fromScript one [.push (.insert "a.rs" 0 "a"), .push (.insert "a.rs" 0 "b"),
                              .move .undo, .push (.insert "a.rs" 0 "c")]
     t.ancestry 2 = [0, 2] ∧ outcomeOf (t.jumpTo 1).1 = .ok ⟨[("a.rs", "bahello")]⟩) := by
  native_decide

/-- @proves REQ-UNDO.preview_is_pure -/
theorem previewing_moves_nothing (base : Workspace) (script : List Step) :
    (runScript base script).currentAfterPreviews = (runScript base script).current := by
  rfl

/-- @proves REQ-UNDO.preview_is_pure -/
theorem a_preview_is_before_and_after :
    (runScript one branching).previews
      = [(.ok one, .ok ⟨[("a.rs", "xhello")]⟩), (.ok one, .ok ⟨[("a.rs", "yhello")]⟩)] := by
  native_decide

end TraceLean.Tree
