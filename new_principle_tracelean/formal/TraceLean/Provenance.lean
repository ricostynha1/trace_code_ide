import TraceLean.Tree

namespace TraceLean.Provenance

open TraceLean.Command
open TraceLean.Tree

open Lean (ToJson FromJson)

inductive Origin where
  | node (node : Nat)
  | base
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive BackStep where
  | moved (position : Nat)
  | writtenHere
  | renamed (from_ : String)
  | fileCreatedHere
  deriving Repr, DecidableEq, Inhabited

def byteLen (s : String) : Nat := s.utf8ByteSize

mutual

/-- @models REQ-PROV.mapping_before
@models REQ-PROV.mapping_after
@models REQ-PROV.mapping_inside
@models REQ-PROV.exact_not_heuristic -/
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

/-- @models REQ-PROV.position_question
@models REQ-PROV.base_is_honest -/
def origin (t : Tree) (atNode : Nat) (file : String) (position : Nat) : Origin :=
  match (t.ancestry atNode).reverse.foldl (originStep t) (Sum.inl (file, position)) with
  | .inr found => found
  | .inl _ => .base

def origins (base : Workspace) (script : List Step) (file : String) (position : Nat)
    : List Origin :=
  let t := fromScript base script
  t.ids.map (fun id => origin t id file position)

/-- @proves REQ-PROV.base_is_honest -/
theorem nothing_recorded_means_nothing_written (base : Workspace) (file : String) (p : Nat) :
    origins base [] file p = [] := by
  rfl

/-- @proves REQ-PROV.mapping_before -/
theorem before_an_edit_is_unchanged (file text : String) (offset position : Nat)
    (h : position < offset) :
    back (.insert file offset text) file position = .moved position ∧
    back (.delete file offset text) file position = .moved position := by
  constructor <;> simp [back, h]

/-- @proves REQ-PROV.mapping_after -/
theorem after_an_edit_is_shifted (file text : String) (offset position : Nat)
    (h : offset + byteLen text ≤ position) :
    back (.insert file offset text) file position = .moved (position - byteLen text) ∧
    back (.delete file offset text) file position = .moved (position + byteLen text) := by
  have h1 : ¬ position < offset := by omega
  have h2 : ¬ position < offset + byteLen text := by omega
  constructor <;> simp [back, h1, h2]

/-- @proves REQ-PROV.mapping_inside -/
theorem inside_an_edit_is_written_there (file text : String) (offset position : Nat)
    (h1 : offset ≤ position) (h2 : position < offset + byteLen text) :
    back (.insert file offset text) file position = .writtenHere := by
  have h3 : ¬ position < offset := by omega
  simp [back, h3, h2]

def edits : List Step :=
  [.push (.insert "a.rs" 0 "xy"), .push (.insert "a.rs" 1 "Q"), .push (.delete "a.rs" 3 "he")]

/-- @proves REQ-PROV.exact_not_heuristic -/
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


def Wrote (t : Tree) (n : Nat) : Prop :=
  ∃ nd name p, t.node? n = some nd ∧ (back nd.command name p = .writtenHere ∨ back nd.command name p = .fileCreatedHere)

theorem origin_found_stays (t : Tree) (o : Origin) :
    ∀ l : List Nat, l.foldl (originStep t) (.inr o) = .inr o
  | [] => rfl
  | _ :: rest => origin_found_stays t o rest

theorem origin_fold_sound (t : Tree) (L : List Nat) :
    ∀ (l : List Nat) (acc : Sum (String × Nat) Origin), (∀ x ∈ l, x ∈ L) →
      (∀ n, acc = .inr (.node n) → n ∈ L ∧ Wrote t n) →
      ∀ n, l.foldl (originStep t) acc = .inr (.node n) → n ∈ L ∧ Wrote t n
  | [], _, _, h => h
  | x :: rest, acc, hl, h => by
    apply origin_fold_sound t L rest _ (fun y hy => hl y (List.mem_cons_of_mem _ hy))
    intro n hn
    have hx : x ∈ L := hl x (List.mem_cons_self _ _)
    unfold originStep at hn
    split at hn
    · exact h n (by simp_all)
    · rename_i name current
      split at hn
      · cases hn
      · rename_i nd hnd
        split at hn
        · rename_i hb
          cases hn
          exact ⟨hx, nd, name, current, hnd, Or.inl hb⟩
        · rename_i hb
          cases hn
          exact ⟨hx, nd, name, current, hnd, Or.inr hb⟩
        · cases hn
        · cases hn

/-- @proves REQ-PROV.position_question -/
theorem the_writer_of_a_position_is_named (t : Tree) (atNode : Nat) (file : String) (position : Nat) :
    (∀ n, origin t atNode file position = .node n →
      n ∈ t.ancestry atNode ∧ Wrote t n) ∧
    (∀ nd, t.node? atNode = some nd → back nd.command file position = .writtenHere →
      origin t atNode file position = .node atNode) := by
  constructor
  · intro n h
    unfold origin at h
    split at h
    · rename_i r hr
      subst h
      exact origin_fold_sound t (t.ancestry atNode).reverse _ _ (fun x hx => hx)
        (fun _ h => by cases h) n hr |>.imp_left List.mem_reverse.mp
    · cases h
  · intro nd hnd hb
    have ha : t.ancestry atNode = Tree.ancestryFrom t.nodes.length t nd.parent ++ [atNode] := by
      simp only [Tree.ancestry, Tree.ancestryFrom, hnd]
    unfold origin
    rw [ha, List.reverse_append]
    simp only [List.reverse_cons, List.reverse_nil, List.nil_append, List.singleton_append,
      List.foldl_cons]
    have hs : originStep t (Sum.inl (file, position)) atNode = .inr (.node atNode) := by
      simp only [originStep, hnd, hb]
    rw [hs, origin_found_stays]

end TraceLean.Provenance
