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

/-- @proves REQ-UNDO.preview_is_pure -/
theorem previewing_moves_nothing (base : Workspace) (script : List Step) :
    (runScript base script).currentAfterPreviews = (runScript base script).current := by
  rfl

theorem push_ids_grow (t : Tree) (c : Command) :
    (t.push c).ids = t.ids ∨ (t.push c).ids = t.ids ++ [t.nextId] :=
  (no_node_is_ever_dropped t c).1

theorem push_branches (t : Tree) (c : Command) :
    (t.push c).current = t.current ∨
      ∃ n ∈ (t.push c).nodes, (t.push c).current = some n.id ∧ n.parent = t.current := by
  unfold Tree.push
  split
  · left; rfl
  · split
    · left; rfl
    · right
      exact ⟨_, List.mem_append_right _ (List.mem_singleton_self _), rfl, rfl⟩

/-- @proves REQ-UNDO.no_loss_on_branch -/
theorem undo_then_edit_keeps_everything (base : Workspace) (script : List Step) (c : Command) :
    (∃ extra, (runScript base (script ++ [Step.move Movement.undo, Step.push c])).nodes =
      (runScript base script).nodes ++ extra) ∧
    (((fromScript base script).undo.push c).current = (fromScript base script).undo.current ∨
      ∃ n ∈ ((fromScript base script).undo.push c).nodes,
        ((fromScript base script).undo.push c).current = some n.id ∧
        n.parent = (fromScript base script).undo.current) := by
  have hscript : fromScript base (script ++ [Step.move Movement.undo, Step.push c]) =
      ((fromScript base script).undo).push c := by
    simp [fromScript, List.foldl_append]
  refine ⟨?_, push_branches _ c⟩
  simp only [runScript, hscript]
  have hu := (no_node_is_ever_dropped (fromScript base script) c).2.1
  rcases push_ids_grow ((fromScript base script).undo) c with h | h
  · exact ⟨[], by rw [h, hu]; simp⟩
  · exact ⟨_, by rw [h, hu]⟩

theorem sharedPrefix_common : ∀ (a b : List Nat),
    a.take (Tree.sharedPrefix a b) = b.take (Tree.sharedPrefix a b) ∧
    ∀ x y, a[Tree.sharedPrefix a b]? = some x → b[Tree.sharedPrefix a b]? = some y → x ≠ y
  | [], b => by simp [Tree.sharedPrefix]
  | _ :: _, [] => by simp [Tree.sharedPrefix]
  | x :: xs, y :: ys => by
    obtain ⟨h1, h2⟩ := sharedPrefix_common xs ys
    unfold Tree.sharedPrefix
    split
    · rename_i hxy
      have hxy : x = y := by simpa using hxy
      subst hxy
      rw [Nat.add_comm]
      refine ⟨by simp [h1], ?_⟩
      intro a b ha hb
      exact h2 a b (by simpa using ha) (by simpa using hb)
    · rename_i hxy
      refine ⟨by simp, ?_⟩
      intro a b ha hb
      simp at ha hb
      subst ha; subst hb
      simpa using hxy

/-- @proves REQ-UNDO.path_via_ancestor -/
theorem jump_via_nearest_common_ancestor (t : Tree) (target : Nat) :
    let from_ := match t.current with
      | none => []
      | some id => t.ancestry id
    let to := t.ancestry target
    let shared := Tree.sharedPrefix from_ to
    from_.take shared = to.take shared ∧
    (∀ x y, from_[shared]? = some x → to[shared]? = some y → x ≠ y) ∧
    (t.jumpTo target).1 =
      (to.drop shared).foldl
        (fun acc nodeId =>
          match acc with
          | .error e => .error e
          | .ok w => match t.node? nodeId with
            | none => .ok w
            | some n => apply w n.command)
        ((from_.drop shared).reverse.foldl
          (fun acc nodeId =>
            match acc with
            | .error e => .error e
            | .ok w => match t.node? nodeId with
              | none => .ok w
              | some n => apply w n.inverse)
          t.state) := by
  intro from_ to shared
  exact ⟨(sharedPrefix_common from_ to).1, (sharedPrefix_common from_ to).2, rfl⟩

end TraceLean.Tree
