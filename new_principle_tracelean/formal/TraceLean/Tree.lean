import TraceLean.Command

/-!
# History as a tree

Models `REQ-UNDO`. A linear undo stack destroys work: undo three steps, type one
character, and the three are gone. They were an alternative, not a mistake.

The load-bearing property is that **jumping and replaying agree**. A node's
state is *defined* by applying its ancestry from the base; `jumpTo` is an
optimisation that travels by way of the nearest common ancestor, inverting on
the way up and applying on the way down. Two definitions of one thing is exactly
the shape that silently drifts, and the drift shows up as a buffer that is
subtly wrong after moving between branches -- reported, if at all, as "undo is
broken sometimes".
-/

namespace TraceLean.Tree

open TraceLean.Command

open Lean (ToJson FromJson)

/-- Node identity: a counter, never anything that varies between runs.

Deliberately not annotated as modelling `ARCH-DETERMINISM.no_ambient_time`. A
datatype cannot model the absence of a call; that clause is about what the
implementation does not do, and is checked by reading the source rather than by
running it. -/
structure Node where
  id : Nat
  command : Command
  inverse : Command
  parent : Option Nat
  children : List Nat
  deriving Repr, Inhabited, ToJson, FromJson

/-- The tree, and where in it we are. -/
structure Tree where
  base : Workspace
  nodes : List Node
  /-- `none` means "at the base". -/
  current : Option Nat
  nextId : Nat
  deriving Repr, Inhabited, ToJson, FromJson

namespace Tree

def mk' (base : Workspace) : Tree :=
  { base := base.canon, nodes := [], current := none, nextId := 0 }

def node? (t : Tree) (id : Nat) : Option Node :=
  t.nodes.find? (·.id == id)

def ids (t : Tree) : List Nat := t.nodes.map (·.id)

/-- Node identifiers from the root down to `id`, inclusive.

`fuel` bounds the walk at the number of nodes: a parent chain in a tree cannot
be longer than that, and a cycle -- which construction makes impossible -- would
otherwise not terminate. -/
def ancestryFrom : Nat → Tree → Option Nat → List Nat
  | 0, _, _ => []
  | _ + 1, _, none => []
  | fuel + 1, t, some id =>
    match t.node? id with
    | none => []
    | some n => ancestryFrom fuel t n.parent ++ [id]

def ancestry (t : Tree) (id : Nat) : List Nat :=
  ancestryFrom (t.nodes.length + 1) t (some id)

/-- The state at a node, by replaying its ancestry from the base.

This is the *definition* of a node's state. -/
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

/-- The diff a node represents: the state before its command and the state
after.

Purely a question about the tree — it computes two states and returns them.
Nothing moves, which in a value-typed model is free and is exactly the thing the
implementation has to be checked for. -/
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

/-- The state at the current position. -/
def state (t : Tree) : Except Refusal Workspace :=
  match t.current with
  | none => .ok t.base
  | some id => t.stateAt id

/-- Record a command, applying it to the current state.

A command recorded after an undo becomes a *sibling* of what was undone, so the
abandoned path is still there; a refused command leaves no node at all. -/
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

/-- Move to the parent, undoing the current command. -/
def undo (t : Tree) : Tree :=
  match t.current with
  | none => t
  | some id =>
    match t.node? id with
    | none => t
    | some n => { t with current := n.parent }

/-- Every node with no parent. -/
def roots (t : Tree) : List Nat :=
  (t.nodes.filter (·.parent.isNone)).map (·.id)

/-- The largest identifier in a list, if there is one. -/
def maximum? : List Nat → Option Nat
  | [] => none
  | x :: rest => some (rest.foldl Nat.max x)

/-- Move to the most recently created child.

The *latest* branch, so redo after undo-and-branch follows the newest work
rather than silently resurrecting the abandoned one. With no child at all the
position does not move. -/
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

/-- How many leading entries two paths share. -/
def sharedPrefix : List Nat → List Nat → Nat
  | a :: as, b :: bs => if a == b then 1 + sharedPrefix as bs else 0
  | _, _ => 0

/-- Travel to a node by way of the nearest common ancestor: invert on the way
up, apply on the way down. -/
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

/-- Moving through a history, as opposed to adding to it. -/
inductive Movement where
  | undo
  | redo
  | jump (node : Nat)
  deriving Repr, Inhabited, ToJson, FromJson

/-- One step of a script driving a tree: an edit, or a move over edits already
recorded. -/
inductive Step where
  | push (command : Command)
  | move (movement : Movement)
  deriving Repr, Inhabited, ToJson, FromJson

/-- What running a script produced. -/
structure Report where
  nodes : List Nat
  current : Option Nat
  travelled : List Outcome
  replayed : List Outcome
  /-- For each node, the diff it represents: the state before its command and
  the state after. -/
  previews : List (Outcome × Outcome)
  /-- Where the tree sits after every node has been previewed.

  Equal to `current`, or the preview moved the position -- which is what
  `preview_is_pure` forbids. A diff view that navigated as a side effect of
  being drawn would leave the user somewhere they did not ask to be, and the
  *next* command would branch from there. -/
  currentAfterPreviews : Option Nat
  deriving Repr, Inhabited, ToJson, FromJson

def outcomeOf : Except Refusal Workspace → Outcome
  | .ok w => .ok w
  | .error r => .refused r.kind

/-- The tree a script builds.

Separate from `runScript` because more than one question is asked of a generated
history -- what each node's state is, and where a position came from -- and both
need the same tree built the same way. -/
def fromScript (base : Workspace) (script : List Step) : Tree :=
  script.foldl
    (fun t step =>
      match step with
      | .push c => t.push c
      | .move .undo => t.undo
      | .move .redo => t.redo
      | .move (.jump n) => (t.jumpTo n).2)
    (Tree.mk' base)

/--
Run a script against a fresh tree, then ask every node the same question two
ways.

Each jump runs on a copy of the post-script tree, so the answers do not depend
on the order they are asked in.

@models REQ-UNDO.jump_equivalence
@models REQ-UNDO.path_via_ancestor
@models REQ-UNDO.reachable
@models REQ-UNDO.no_loss_on_branch
@models REQ-UNDO.preview_is_pure
-/
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
    -- Previewing returns a value and changes nothing, so the position is
    -- wherever the script left it.
    currentAfterPreviews := final.current }

/-- The two answers agree on an empty history, trivially -- and the statement is
what the differential test generalises.

@proves REQ-UNDO.jump_equivalence -/
theorem an_empty_history_has_nothing_to_disagree_about (base : Workspace) :
    (runScript base []).travelled = (runScript base []).replayed := by
  rfl

end TraceLean.Tree
