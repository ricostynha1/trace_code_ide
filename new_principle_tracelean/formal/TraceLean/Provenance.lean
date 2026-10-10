import TraceLean.Tree

/-!
# Where a position came from

Models `REQ-PROV`. "Who wrote this?" answered from the recorded history rather
than from a version-control system: put a cursor somewhere, walk the commands
backwards, and stop at the one whose replacement the position falls inside.

The property that makes it worth having is that it is **exact**. There is no
similarity scoring and no dependence on what the buffer currently contains: a
position either lands inside what a command inserted or it does not, and the
three cases -- before the span, inside it, after it -- partition every position.
A heuristic version would be right most of the time and wrong without saying so,
which is worse than not answering.
-/

namespace TraceLean.Provenance

open TraceLean.Command
open TraceLean.Tree

open Lean (ToJson FromJson)

/-- Where a position came from. -/
inductive Origin where
  /-- The command that wrote the text at that position. -/
  | node (node : Nat)
  /-- The text arrived with the file rather than being written by a recorded
  command. The honest answer rather than a plausible wrong one. -/
  | base
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One step of the backward map. -/
inductive BackStep where
  | moved (position : Nat)
  | writtenHere
  | renamed (from_ : String)
  | fileCreatedHere
  deriving Repr, DecidableEq, Inhabited

/-- Byte length, which is what an offset counts. -/
def byteLen (s : String) : Nat := s.utf8ByteSize

mutual

/--
Map a position in the state *after* a command back to the state before it.

A deletion writes nothing, so nothing maps into it.

@models REQ-PROV.mapping_before
@models REQ-PROV.mapping_after
@models REQ-PROV.mapping_inside
@models REQ-PROV.exact_not_heuristic
-/
def back (command : Command) (file : String) (position : Nat) : BackStep :=
  match command with
  | .insert target offset text =>
    if target != file then .moved position
    else if position < offset then .moved position
    else if position < offset + byteLen text then .writtenHere
    else .moved (position - byteLen text)
  | .delete target offset deleted =>
    if target != file then .moved position
    else if position < offset then .moved position
    else .moved (position + byteLen deleted)
  | .createFile path => if path == file then .fileCreatedHere else .moved position
  | .deleteFile path _ => if path == file then .fileCreatedHere else .moved position
  | .renameFile from_ to => if to == file then .renamed from_ else .moved position
  | .batch commands =>
    match backMembers commands file position with
    | .inr done => done
    | .inl (name, current) => if name == file then .moved current else .renamed name

/-- A batch is undone in reverse, so it is read in reverse: the tail is mapped
back before the head. -/
def backMembers : List Command → String → Nat → Sum (String × Nat) BackStep
  | [], name, current => .inl (name, current)
  | c :: rest, name, current =>
    match backMembers rest name current with
    | .inr done => .inr done
    | .inl (name', current') =>
      match back c name' current' with
      | .moved next => .inl (name', next)
      | .writtenHere => .inr .writtenHere
      | .fileCreatedHere => .inr .fileCreatedHere
      | .renamed previous => .inl (previous, current')

end


/-- One step of the walk `origin` folds over a history.

A typed parameter list rather than a typed lambda binder, which the grammar that
reads these annotations cannot read (ADR-0008). -/
private def originStep (t : Tree) (acc : Sum (String × Nat) Origin) (nodeId : Nat)
    : Sum (String × Nat) Origin :=
  match acc with
  | .inr found => .inr found
  | .inl (name, current) =>
    match t.node? nodeId with
    | none => .inr .base
    | some n =>
      match back n.command name current with
      | .writtenHere => .inr (.node nodeId)
      | .fileCreatedHere => .inr (.node nodeId)
      | .moved next => .inl (name, next)
      | .renamed previous => .inl (previous, current)

/--
Which recorded edit wrote the text at `position` in `file`, as of node `at`;
`base` when none did.

@models REQ-PROV.position_question
@models REQ-PROV.base_is_honest
-/
def origin (t : Tree) (atNode : Nat) (file : String) (position : Nat) : Origin :=
  match (t.ancestry atNode).reverse.foldl (originStep t) (Sum.inl (file, position)) with
  | .inr found => found
  | .inl _ => .base

/-- `origin` for every node of a history built by a script. -/
def origins (base : Workspace) (script : List Step) (file : String) (position : Nat)
    : List Origin :=
  let t := fromScript base script
  t.ids.map (fun id => origin t id file position)

/-- An empty history attributes everything to the file it arrived in.

@proves REQ-PROV.base_is_honest -/
theorem nothing_recorded_means_nothing_written (base : Workspace) (file : String) (p : Nat) :
    origins base [] file p = [] := by
  rfl

/-- A position before an insertion maps back unchanged.

@proves REQ-PROV.mapping_before -/
theorem before_an_edit_is_unchanged (file text : String) (offset position : Nat)
    (h : position < offset) :
    back (.insert file offset text) file position = .moved position ∧
    back (.delete file offset text) file position = .moved position := by
  constructor <;> simp [back, h]

/-- A position after an insertion maps back by the length it added; after a
deletion, by the length it removed.

@proves REQ-PROV.mapping_after -/
theorem after_an_edit_is_shifted (file text : String) (offset position : Nat)
    (h : offset + byteLen text ≤ position) :
    back (.insert file offset text) file position = .moved (position - byteLen text) ∧
    back (.delete file offset text) file position = .moved (position + byteLen text) := by
  have h1 : ¬ position < offset := by omega
  have h2 : ¬ position < offset + byteLen text := by omega
  constructor <;> simp [back, h1, h2]

/-- A position inside what an insertion wrote ends the search there.

@proves REQ-PROV.mapping_inside -/
theorem inside_an_edit_is_written_there (file text : String) (offset position : Nat)
    (h1 : offset ≤ position) (h2 : position < offset + byteLen text) :
    back (.insert file offset text) file position = .writtenHere := by
  have h3 : ¬ position < offset := by omega
  simp [back, h3, h2]

/-- Two insertions and a deletion over `hello`. -/
def edits : List Step :=
  [.push (.insert "a.rs" 0 "xy"), .push (.insert "a.rs" 1 "Q"), .push (.delete "a.rs" 3 "he")]

/-- Each position is answered with the edit that wrote it, at every point of the
history: `x` and `y` by the first edit, `Q` between them by the second, and the
text the file arrived with by none.

@proves REQ-PROV.position_question -/
theorem each_position_names_its_writer :
    [0, 1, 2, 3].map (fun p => origins ⟨[("a.rs", "hello")]⟩ edits "a.rs" p)
      = [[.node 0, .node 0, .node 0], [.node 0, .node 1, .node 1],
         [.base, .node 0, .node 0], [.base, .base, .base]] := by
  native_decide

/-- The answer comes from inverting the edit, never from the text: its offset
and length alone place every position before it, inside it or after it, with
nothing left to estimate.

@proves REQ-PROV.exact_not_heuristic -/
theorem the_answer_is_exact (file text : String) (offset position : Nat) :
    back (.insert file offset text) file position = .moved position ∨
    back (.insert file offset text) file position = .writtenHere ∨
    back (.insert file offset text) file position = .moved (position - byteLen text) := by
  simp only [back, bne_self_eq_false, Bool.false_eq_true, ite_false]
  split
  · exact Or.inl rfl
  · split
    · exact Or.inr (Or.inl rfl)
    · exact Or.inr (Or.inr rfl)

end TraceLean.Provenance
