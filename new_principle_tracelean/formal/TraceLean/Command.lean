import Lean

namespace TraceLean.Command

open Lean (Json ToJson FromJson)

structure Workspace where
  files : List (String × String) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Workspace

def canon (w : Workspace) : Workspace :=
  let deduped := w.files.foldl
    (fun acc (name, content) => (acc.filter (·.1 != name)) ++ [(name, content)]) []
  ⟨deduped.mergeSort (fun a b => a.1 ≤ b.1)⟩

def get (w : Workspace) (path : String) : Option String :=
  (w.files.find? (·.1 == path)).map (·.2)

def contains (w : Workspace) (path : String) : Bool :=
  (w.get path).isSome

def set (w : Workspace) (path content : String) : Workspace :=
  let rest := w.files.filter (·.1 != path)
  ⟨(rest ++ [(path, content)]).mergeSort (fun a b => a.1 ≤ b.1)⟩

def remove (w : Workspace) (path : String) : Workspace :=
  ⟨w.files.filter (·.1 != path)⟩

end Workspace

/-- @models REQ-CMD.witness_carried -/
inductive Command where
  | insert (file : String) (offset : Nat) (text : String)
  | delete (file : String) (offset : Nat) (deleted : String)
  | createFile (path : String)
  | deleteFile (path : String) (content : String)
  | renameFile (from_ to : String)
  | batch (commands : List Command)
  deriving Repr, Inhabited, ToJson, FromJson

inductive Refusal where
  | noSuchFile (path : String)
  | fileExists (path : String)
  | offsetOutOfRange (path : String) (offset len : Nat)
  | witnessMismatch (path : String) (offset : Nat)
  | notACharBoundary (path : String) (offset : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Refusal.kind : Refusal → String
  | .noSuchFile _ => "noSuchFile"
  | .fileExists _ => "fileExists"
  | .offsetOutOfRange _ _ _ => "offsetOutOfRange"
  | .witnessMismatch _ _ => "witnessMismatch"
  | .notACharBoundary _ _ => "notACharBoundary"

inductive Outcome where
  | ok (w : Workspace)
  | refused (kind : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def splitAt (s : String) (n : Nat) : String × String :=
  let cs := s.toList
  (String.mk (cs.take n), String.mk (cs.drop n))

/-- @models REQ-CMD.single_path
@models REQ-CMD.total_or_refused -/
def apply (w : Workspace) (c : Command) : Except Refusal Workspace :=
  match c with
  | .insert file offset text =>
    match w.get file with
    | none => .error (.noSuchFile file)
    | some content =>
      if offset > content.length then
        .error (.offsetOutOfRange file offset content.length)
      else
        let (before, after) := splitAt content offset
        .ok (w.set file (before ++ text ++ after))
  | .delete file offset deleted =>
    match w.get file with
    | none => .error (.noSuchFile file)
    | some content =>
      if offset + deleted.length > content.length then
        .error (.offsetOutOfRange file (offset + deleted.length) content.length)
      else
        let (before, rest) := splitAt content offset
        let (removed, after) := splitAt rest deleted.length
        if removed != deleted then
          .error (.witnessMismatch file offset)
        else
          .ok (w.set file (before ++ after))
  | .createFile path =>
    if w.contains path then .error (.fileExists path)
    else .ok (w.set path "")
  | .deleteFile path content =>
    match w.get path with
    | none => .error (.noSuchFile path)
    | some actual =>
      if actual != content then .error (.witnessMismatch path 0)
      else .ok (w.remove path)
  | .renameFile from_ to =>
    match w.get from_ with
    | none => .error (.noSuchFile from_)
    | some content =>
      if w.contains to then .error (.fileExists to)
      else .ok ((w.remove from_).set to content)
  | .batch commands =>
    commands.attach.foldlM (fun acc c => apply acc c.val) w
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/-- @models REQ-CMD.inverse_exists
@models REQ-CMD.batch_reverses -/
def inverse : Command → Command
  | .insert file offset text => .delete file offset text
  | .delete file offset deleted => .insert file offset deleted
  | .createFile path => .deleteFile path ""
  | .deleteFile path content =>
    .batch [.createFile path, .insert path 0 content]
  | .renameFile from_ to => .renameFile to from_
  | .batch commands => .batch ((commands.attach.map (fun c => inverse c.val)).reverse)
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

def touched : Command → String
  | .insert file _ _ => file
  | .delete file _ _ => file
  | .createFile path => path
  | .deleteFile path _ => path
  | .renameFile from_ to => from_ ++ " → " ++ to
  | .batch commands => String.intercalate ", " (commands.attach.map (fun c => touched c.val))
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/-- @models REQ-CMD.round_trip -/
def roundTrip (w : Workspace) (c : Command) : Except Refusal Workspace := do
  let after ← apply w c
  apply after (inverse c)

def roundTripOutcome (w : Workspace) (c : Command) : Outcome :=
  match roundTrip w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

def applyOutcome (w : Workspace) (c : Command) : Outcome :=
  match apply w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

/-- @proves REQ-CMD.inverse_exists -/
theorem inverse_inverse_insert (file : String) (offset : Nat) (text : String) :
    inverse (inverse (.insert file offset text)) = .insert file offset text := by
  simp [inverse]

theorem inverse_inverse_delete (file : String) (offset : Nat) (deleted : String) :
    inverse (inverse (.delete file offset deleted)) = .delete file offset deleted := by
  simp [inverse]

theorem inverse_inverse_rename (a b : String) :
    inverse (inverse (.renameFile a b)) = .renameFile a b := by
  simp [inverse]

/-- @proves REQ-CMD.batch_reverses -/
theorem batch_inverse_is_reversed (commands : List Command) :
    inverse (.batch commands) = .batch ((commands.map inverse).reverse) := by
  simp [inverse, List.attach_map_val]

def sample : Workspace := ⟨[("a.rs", "hello"), ("b.rs", "")]⟩

/-- @proves REQ-CMD.single_path -/
theorem a_batch_is_its_members_in_turn (w : Workspace) (a b : Command) :
    apply w (.batch [a, b]) = Except.bind (apply w a) (fun v => apply v b) := by
  rw [apply]
  simp [List.attach, List.foldlM]
  rfl

def Keys (w : Workspace) : Prop := (w.files.map (·.1)).Nodup

def Same (a b : Workspace) : Prop := ∀ p, a.get p = b.get p

theorem find_some_iff (l : List (String × String)) (nd : (l.map (·.1)).Nodup) (p v : String) :
    (l.find? (·.1 == p)).map (·.2) = some v ↔ (p, v) ∈ l := by
  induction l with
  | nil => simp
  | cons x rest ih =>
    rcases x with ⟨k, u⟩
    have ⟨notIn, ndRest⟩ := List.nodup_cons.mp nd
    by_cases e : k = p
    · subst e
      simp only [List.find?_cons, beq_self_eq_true, Option.map_some', Option.some.injEq,
        List.mem_cons, Prod.mk.injEq, true_and]
      constructor
      · intro h; exact Or.inl h.symm
      · rintro (h | h)
        · exact h.symm
        · exact absurd (List.mem_map_of_mem (·.1) h) notIn
    · have ne : (k == p) = false := by simp [e]
      simp only [List.find?_cons, ne, List.mem_cons, Prod.mk.injEq]
      rw [ih ndRest]
      constructor
      · intro h; exact Or.inr h
      · rintro (h | h)
        · exact absurd h.1.symm e
        · exact h

theorem find_none_iff (l : List (String × String)) (p : String) :
    (l.find? (·.1 == p)).map (·.2) = none ↔ ∀ v, (p, v) ∉ l := by
  simp only [Option.map_eq_none', List.find?_eq_none, beq_iff_eq]
  constructor
  · intro h v hv
    exact h _ hv rfl
  · intro h x hx e
    rcases x with ⟨k, u⟩
    simp only at e
    subst e
    exact h u hx

theorem get_some_iff (w : Workspace) (k : Keys w) (p v : String) :
    w.get p = some v ↔ (p, v) ∈ w.files :=
  find_some_iff w.files k p v

theorem get_none_iff (w : Workspace) (p : String) :
    w.get p = none ↔ ∀ v, (p, v) ∉ w.files :=
  find_none_iff w.files p

theorem get_of_mem_iff (a b : Workspace) (ka : Keys a) (kb : Keys b) (q : String)
    (h : ∀ v, (q, v) ∈ a.files ↔ (q, v) ∈ b.files) : a.get q = b.get q := by
  cases hb : b.get q with
  | none =>
    rw [get_none_iff] at hb ⊢
    intro v hv
    exact hb v ((h v).mp hv)
  | some v =>
    rw [get_some_iff b kb] at hb
    rw [get_some_iff a ka]
    exact (h v).mpr hb

theorem keys_filter (l : List (String × String)) (nd : (l.map (·.1)).Nodup)
    (f : String × String → Bool) : ((l.filter f).map (·.1)).Nodup :=
  List.Nodup.sublist ((List.filter_sublist l).map _) nd

theorem keys_set (w : Workspace) (k : Keys w) (p c : String) : Keys (w.set p c) := by
  unfold Keys Workspace.set
  apply List.Nodup.perm _ ((List.mergeSort_perm _ _).map _).symm
  rw [List.map_append]
  unfold List.Nodup
  rw [List.pairwise_append]
  refine ⟨keys_filter w.files k _, List.pairwise_singleton _ _, ?_⟩
  intro a ha b hb
  simp only [List.map_cons, List.map_nil, List.mem_singleton] at hb
  subst hb
  simp only [List.mem_map, List.mem_filter, bne_iff_ne, ne_eq] at ha
  obtain ⟨x, ⟨_, ne⟩, rfl⟩ := ha
  exact ne

theorem keys_remove (w : Workspace) (k : Keys w) (p : String) : Keys (w.remove p) :=
  keys_filter w.files k _

theorem get_set (w : Workspace) (k : Keys w) (p c q : String) :
    (w.set p c).get q = if q = p then some c else w.get q := by
  have ks := keys_set w k p c
  have mem : ∀ x, x ∈ (w.set p c).files ↔ (x.1 ≠ p ∧ x ∈ w.files) ∨ x = (p, c) := by
    intro x
    simp only [Workspace.set, List.mem_mergeSort, List.mem_append, List.mem_filter,
      List.mem_singleton, bne_iff_ne, ne_eq]
    constructor
    · rintro (⟨a, b⟩ | a)
      · exact Or.inl ⟨b, a⟩
      · exact Or.inr a
    · rintro (⟨a, b⟩ | a)
      · exact Or.inl ⟨b, a⟩
      · exact Or.inr a
  split
  · rename_i e
    subst e
    rw [get_some_iff _ ks, mem]
    exact Or.inr rfl
  · rename_i e
    refine get_of_mem_iff _ _ ks k q (fun v => ?_)
    rw [mem]
    simp only [Prod.mk.injEq]
    constructor
    · rintro (⟨_, h⟩ | ⟨h, _⟩)
      · exact h
      · exact absurd h e
    · intro h
      exact Or.inl ⟨e, h⟩

theorem get_remove (w : Workspace) (k : Keys w) (p q : String) :
    (w.remove p).get q = if q = p then none else w.get q := by
  have kr := keys_remove w k p
  split
  · rename_i e
    subst e
    rw [get_none_iff]
    intro v hv
    simp [Workspace.remove, List.mem_filter] at hv
  · rename_i e
    refine get_of_mem_iff _ _ kr k q (fun v => ?_)
    simp only [Workspace.remove, List.mem_filter, bne_iff_ne, ne_eq]
    constructor
    · exact fun h => h.1
    · exact fun h => ⟨h, e⟩

theorem foldlM_attach (l : List Command) (f : Workspace → Command → Except Refusal Workspace)
    (w : Workspace) :
    l.attach.foldlM (fun acc c => f acc c.val) w = l.foldlM f w := by
  have gen : ∀ (m : List {x // x ∈ l}) (w : Workspace),
      m.foldlM (fun acc c => f acc c.val) w = (m.map Subtype.val).foldlM f w := by
    intro m
    induction m with
    | nil => intro w; rfl
    | cons x xs ih =>
      intro w
      simp only [List.foldlM_cons, List.map_cons]
      congr 1
      funext w'
      exact ih w'
  rw [gen, List.attach_map_subtype_val]

theorem apply_batch (w : Workspace) (cs : List Command) :
    apply w (.batch cs) = cs.foldlM apply w := by
  rw [apply]
  exact foldlM_attach cs apply w

theorem bind_ok {α β : Type} (x : Except Refusal α) (f : α → Except Refusal β) (b : β) :
    (x >>= f) = .ok b ↔ ∃ a, x = .ok a ∧ f a = .ok b := by
  cases x with
  | error e =>
    constructor
    · intro h; cases h
    · rintro ⟨a, h, _⟩; cases h
  | ok a =>
    constructor
    · intro h; exact ⟨a, rfl, h⟩
    · rintro ⟨a', h, hf⟩
      cases h
      exact hf

theorem len_data (s : String) : s.length = s.data.length := by
  cases s; rfl

theorem len_mk (l : List Char) : (String.mk l).length = l.length := rfl

theorem fold_ok (cs : List Command) (c : Command) (w w' : Workspace) :
    (c :: cs).foldlM apply w = .ok w' ↔ ∃ w1, apply w c = .ok w1 ∧ cs.foldlM apply w1 = .ok w' := by
  rw [List.foldlM_cons, bind_ok]

theorem fold_nil_ok (w w' : Workspace) : ([] : List Command).foldlM apply w = .ok w' ↔ w = w' := by
  simp only [List.foldlM_nil]
  constructor
  · intro h; cases h; rfl
  · intro h; subst h; rfl

theorem keys_fold (cs : List Command)
    (each : ∀ c ∈ cs, ∀ w w', Keys w → apply w c = .ok w' → Keys w') :
    ∀ w w', Keys w → cs.foldlM apply w = .ok w' → Keys w' := by
  induction cs with
  | nil => intro w w' k h; rw [fold_nil_ok] at h; subst h; exact k
  | cons c rest ih =>
    intro w w' k h
    obtain ⟨w1, h1, h2⟩ := (fold_ok rest c w w').mp h
    exact ih (fun c' hc' => each c' (List.mem_cons_of_mem _ hc')) w1 w'
      (each c (List.mem_cons_self _ _) w w1 k h1) h2

theorem keys_apply (c : Command) : ∀ w w', Keys w → apply w c = .ok w' → Keys w' := by
  match c with
  | .insert file offset text =>
    intro w w' k h
    simp only [apply] at h
    split at h
    · cases h
    · split at h
      · cases h
      · cases h; exact keys_set w k _ _
  | .delete file offset deleted =>
    intro w w' k h
    simp only [apply] at h
    split at h
    · cases h
    · split at h
      · cases h
      · split at h
        · cases h
        · cases h; exact keys_set w k _ _
  | .createFile path =>
    intro w w' k h
    simp only [apply] at h
    split at h
    · cases h
    · cases h; exact keys_set w k _ _
  | .deleteFile path content =>
    intro w w' k h
    simp only [apply] at h
    split at h
    · cases h
    · split at h
      · cases h
      · cases h; exact keys_remove w k _
  | .renameFile from_ to =>
    intro w w' k h
    simp only [apply] at h
    split at h
    · cases h
    · split at h
      · cases h
      · cases h; exact keys_set _ (keys_remove w k _) _ _
  | .batch cs =>
    intro w w' k h
    rw [apply_batch] at h
    exact keys_fold cs (fun c' hc' => keys_apply c') w w' k h
termination_by sizeOf c
decreasing_by
  simp_wf
  have := List.sizeOf_lt_of_mem hc'
  omega

theorem apply_insert (w : Workspace) (f : String) (o : Nat) (t : String) (cs : List Char)
    (hg : w.get f = some ⟨cs⟩) (hle : o ≤ cs.length) :
    apply w (.insert f o t) = .ok (w.set f (String.mk (cs.take o) ++ t ++ String.mk (cs.drop o))) := by
  have : ¬ o > (String.mk cs).length := by simp only [len_mk] at *; (try simp only [len_data] at *); omega
  simp only [apply, hg, this, if_false]
  rfl

theorem apply_insert_inv (w w' : Workspace) (f : String) (o : Nat) (t : String)
    (h : apply w (.insert f o t) = .ok w') :
    ∃ cs : List Char, w.get f = some ⟨cs⟩ ∧ o ≤ cs.length ∧
      w' = w.set f (String.mk (cs.take o) ++ t ++ String.mk (cs.drop o)) := by
  simp only [apply] at h
  split at h
  · cases h
  · rename_i content hg
    split at h
    · cases h
    · rename_i hle
      cases h
      rcases content with ⟨cs⟩
      refine ⟨cs, hg, ?_, rfl⟩
      rw [len_mk] at hle
      omega

theorem apply_delete (w : Workspace) (f : String) (o : Nat) (d : String) (cs : List Char)
    (hg : w.get f = some ⟨cs⟩) (hle : o + d.length ≤ cs.length)
    (hw : String.mk ((cs.drop o).take d.length) = d) :
    apply w (.delete f o d) =
      .ok (w.set f (String.mk (cs.take o) ++ String.mk ((cs.drop o).drop d.length))) := by
  have : ¬ o + d.length > (String.mk cs).length := by simp only [len_mk] at *; (try simp only [len_data] at *); omega
  simp only [apply, hg, this, if_false, splitAt, String.toList]
  rw [hw]
  simp

theorem apply_delete_inv (w w' : Workspace) (f : String) (o : Nat) (d : String)
    (h : apply w (.delete f o d) = .ok w') :
    ∃ cs : List Char, w.get f = some ⟨cs⟩ ∧ o + d.length ≤ cs.length ∧
      String.mk ((cs.drop o).take d.length) = d ∧
      w' = w.set f (String.mk (cs.take o) ++ String.mk ((cs.drop o).drop d.length)) := by
  simp only [apply] at h
  split at h
  · cases h
  · rename_i content hg
    split at h
    · cases h
    · rename_i hle
      simp only [splitAt, String.toList] at h
      split at h
      · cases h
      · rename_i same
        cases h
        rcases content with ⟨cs⟩
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at same
        refine ⟨cs, hg, ?_, same, rfl⟩
        simp only [len_mk] at *
        simp only [len_data] at *
        omega

theorem apply_create (w : Workspace) (p : String) (hg : w.get p = none) :
    apply w (.createFile p) = .ok (w.set p "") := by
  simp [apply, Workspace.contains, hg]

theorem apply_create_inv (w w' : Workspace) (p : String) (h : apply w (.createFile p) = .ok w') :
    w.get p = none ∧ w' = w.set p "" := by
  simp only [apply, Workspace.contains] at h
  split at h
  · cases h
  · rename_i hc
    cases h
    refine ⟨?_, rfl⟩
    cases hg : w.get p
    · rfl
    · rw [hg] at hc; simp at hc

theorem apply_delete_file (w : Workspace) (p c : String) (hg : w.get p = some c) :
    apply w (.deleteFile p c) = .ok (w.remove p) := by
  simp [apply, hg]

theorem apply_delete_file_inv (w w' : Workspace) (p c : String)
    (h : apply w (.deleteFile p c) = .ok w') : w.get p = some c ∧ w' = w.remove p := by
  simp only [apply] at h
  split at h
  · cases h
  · rename_i actual hg
    split at h
    · cases h
    · rename_i same
      cases h
      simp only [bne_iff_ne, ne_eq, Decidable.not_not] at same
      exact ⟨same ▸ hg, rfl⟩

theorem apply_rename (w : Workspace) (a b c : String) (ha : w.get a = some c) (hb : w.get b = none) :
    apply w (.renameFile a b) = .ok ((w.remove a).set b c) := by
  simp [apply, Workspace.contains, ha, hb]

theorem apply_rename_inv (w w' : Workspace) (a b : String) (h : apply w (.renameFile a b) = .ok w') :
    ∃ c, w.get a = some c ∧ w.get b = none ∧ w' = (w.remove a).set b c := by
  simp only [apply, Workspace.contains] at h
  split at h
  · cases h
  · rename_i c ha
    split at h
    · cases h
    · rename_i hc
      cases h
      refine ⟨c, ha, ?_, rfl⟩
      cases hg : w.get b
      · rfl
      · rw [hg] at hc; simp at hc

def Agrees (a b : Except Refusal Workspace) : Prop :=
  match a, b with
  | .ok x, .ok y => Same x y
  | .error e, .error f => e = f
  | _, _ => False

theorem same_set (a b : Workspace) (ka : Keys a) (kb : Keys b) (h : Same a b) (p c : String) :
    Same (a.set p c) (b.set p c) := by
  intro q
  rw [get_set a ka, get_set b kb, h q]

theorem same_remove (a b : Workspace) (ka : Keys a) (kb : Keys b) (h : Same a b) (p : String) :
    Same (a.remove p) (b.remove p) := by
  intro q
  rw [get_remove a ka, get_remove b kb, h q]

theorem agrees_fold (cs : List Command)
    (each : ∀ c ∈ cs, ∀ w1 w2, Keys w1 → Keys w2 → Same w1 w2 → Agrees (apply w1 c) (apply w2 c)) :
    ∀ w1 w2, Keys w1 → Keys w2 → Same w1 w2 →
      Agrees (cs.foldlM apply w1) (cs.foldlM apply w2) := by
  induction cs with
  | nil => intro w1 w2 _ _ h; exact h
  | cons c rest ih =>
    intro w1 w2 k1 k2 h
    have hc := each c (List.mem_cons_self _ _) w1 w2 k1 k2 h
    rw [List.foldlM_cons, List.foldlM_cons]
    cases h1 : apply w1 c <;> cases h2 : apply w2 c <;> rw [h1, h2] at hc <;>
      simp only [Agrees] at hc
    · exact hc
    · rename_i x y
      exact ih (fun c' hc' => each c' (List.mem_cons_of_mem _ hc')) x y
        (keys_apply c w1 x k1 h1) (keys_apply c w2 y k2 h2) hc

theorem agrees_apply (c : Command) :
    ∀ w1 w2, Keys w1 → Keys w2 → Same w1 w2 → Agrees (apply w1 c) (apply w2 c) := by
  match c with
  | .batch cs =>
    intro w1 w2 k1 k2 h
    rw [apply_batch, apply_batch]
    exact agrees_fold cs (fun c' hc' => agrees_apply c') w1 w2 k1 k2 h
  | .insert file offset text =>
    intro w1 w2 k1 k2 h
    simp only [apply, h file]
    repeat' split
    all_goals first | rfl | exact same_set w1 w2 k1 k2 h _ _
  | .delete file offset deleted =>
    intro w1 w2 k1 k2 h
    simp only [apply, h file]
    repeat' split
    all_goals first | rfl | exact same_set w1 w2 k1 k2 h _ _
  | .createFile path =>
    intro w1 w2 k1 k2 h
    simp only [apply, Workspace.contains, h path]
    repeat' split
    all_goals first | rfl | exact same_set w1 w2 k1 k2 h _ _
  | .deleteFile path content =>
    intro w1 w2 k1 k2 h
    simp only [apply, h path]
    repeat' split
    all_goals first | rfl | exact same_remove w1 w2 k1 k2 h _
  | .renameFile from_ to =>
    intro w1 w2 k1 k2 h
    simp only [apply, Workspace.contains, h from_, h to]
    repeat' split
    all_goals first | rfl | exact same_set _ _ (keys_remove w1 k1 _) (keys_remove w2 k2 _) (same_remove w1 w2 k1 k2 h _) _ _
termination_by sizeOf c
decreasing_by
  simp_wf
  have := List.sizeOf_lt_of_mem hc'
  omega

theorem restored (w : Workspace) (k : Keys w) (f n content : String) (hg : w.get f = some content) :
    Same ((w.set f n).set f content) w := by
  intro q
  rw [get_set _ (keys_set w k f n), get_set w k]
  by_cases e : q = f
  · subst e; simp [hg]
  · simp [e]

theorem restores_fold (cs : List Command)
    (each : ∀ c ∈ cs, ∀ w w', Keys w → apply w c = .ok w' →
      ∃ r, apply w' (inverse c) = .ok r ∧ Same r w) :
    ∀ w w', Keys w → cs.foldlM apply w = .ok w' →
      ∃ r, ((cs.map inverse).reverse).foldlM apply w' = .ok r ∧ Same r w := by
  induction cs with
  | nil =>
    intro w w' _ h
    rw [fold_nil_ok] at h
    subst h
    exact ⟨w, rfl, fun _ => rfl⟩
  | cons c rest ih =>
    intro w w' k h
    obtain ⟨w1, h1, h2⟩ := (fold_ok rest c w w').mp h
    have k1 := keys_apply c w w1 k h1
    obtain ⟨r1, hr1, s1⟩ := ih (fun c' hc' => each c' (List.mem_cons_of_mem _ hc')) w1 w' k1 h2
    obtain ⟨r, hr, s⟩ := each c (List.mem_cons_self _ _) w w1 k h1
    have kw' := keys_fold rest (fun c' _ => keys_apply c') w1 w' k1 h2
    have kr1 := keys_fold _ (fun c' _ => keys_apply c') w' r1 kw' hr1
    have ag := agrees_apply (inverse c) r1 w1 kr1 k1 s1
    rw [hr] at ag
    cases hx : apply r1 (inverse c) with
    | error e => rw [hx] at ag; exact ag.elim
    | ok x =>
      rw [hx] at ag
      refine ⟨x, ?_, fun q => (ag q).trans (s q)⟩
      rw [List.map_cons, List.reverse_cons, List.foldlM_append, hr1]
      show [inverse c].foldlM apply r1 = _
      rw [List.foldlM_cons, hx]
      rfl

theorem restores (c : Command) : ∀ w w', Keys w → apply w c = .ok w' →
    ∃ r, apply w' (inverse c) = .ok r ∧ Same r w := by
  match c with
  | .insert f o t =>
    intro w w' k h
    obtain ⟨cs, hg, hle, rfl⟩ := apply_insert_inv w w' f o t h
    have inv : inverse (.insert f o t) = .delete f o t := by rw [inverse]
    rw [inv]
    rcases t with ⟨ts⟩
    have tl : (cs.take o).length = o := by simp [List.length_take]; omega
    have get' : (w.set f (String.mk (cs.take o) ++ ⟨ts⟩ ++ String.mk (cs.drop o))).get f =
        some ⟨cs.take o ++ (ts ++ cs.drop o)⟩ := by
      rw [get_set w k, if_pos rfl]
      simp only [Option.some.injEq]
      show String.mk (cs.take o ++ ts ++ cs.drop o) = _
      rw [List.append_assoc]
    have hle' : o + (String.mk ts).length ≤ (cs.take o ++ (ts ++ cs.drop o)).length := by
      simp only [len_mk, List.length_append, tl, List.length_drop]
      omega
    have hw : String.mk (((cs.take o ++ (ts ++ cs.drop o)).drop o).take (String.mk ts).length) =
        ⟨ts⟩ := by
      rw [List.drop_left' tl, len_mk, List.take_left' rfl]
    refine ⟨_, apply_delete _ f o ⟨ts⟩ _ get' hle' hw, ?_⟩
    rw [List.take_left' tl, List.drop_left' tl, len_mk, List.drop_left' rfl]
    have back : String.mk (cs.take o) ++ String.mk (cs.drop o) = ⟨cs⟩ := by
      show String.mk (cs.take o ++ cs.drop o) = _
      rw [List.take_append_drop]
    rw [back]
    exact restored w k f _ _ hg
  | .delete f o d =>
    intro w w' k h
    obtain ⟨cs, hg, hle, hw, rfl⟩ := apply_delete_inv w w' f o d h
    have inv : inverse (.delete f o d) = .insert f o d := by rw [inverse]
    rw [inv]
    have dl : d.length = d.data.length := len_data d
    have tl : (cs.take o).length = o := by simp [List.length_take]; omega
    have get' : (w.set f (String.mk (cs.take o) ++ String.mk ((cs.drop o).drop d.length))).get f =
        some ⟨cs.take o ++ (cs.drop o).drop d.length⟩ := by
      rw [get_set w k, if_pos rfl]
      rfl
    have hle' : o ≤ (cs.take o ++ (cs.drop o).drop d.length).length := by
      simp only [List.length_append, tl]
      omega
    refine ⟨_, apply_insert _ f o d _ get' hle', ?_⟩
    rw [List.take_left' tl, List.drop_left' tl]
    have back : String.mk (cs.take o) ++ d ++ String.mk ((cs.drop o).drop d.length) = ⟨cs⟩ := by
      have hd : (cs.drop o).take d.length = d.data := congrArg String.data hw
      show String.mk (cs.take o ++ d.data ++ (cs.drop o).drop d.length) = _
      rw [← hd, List.append_assoc, List.take_append_drop, List.take_append_drop]
    rw [back]
    exact restored w k f _ _ hg
  | .createFile p =>
    intro w w' k h
    obtain ⟨hg, rfl⟩ := apply_create_inv w w' p h
    have inv : inverse (.createFile p) = .deleteFile p "" := by rw [inverse]
    rw [inv]
    have get' : (w.set p "").get p = some "" := by rw [get_set w k, if_pos rfl]
    refine ⟨_, apply_delete_file _ p "" get', ?_⟩
    intro q
    rw [get_remove _ (keys_set w k p ""), get_set w k]
    by_cases e : q = p
    · subst e; simp [hg]
    · simp [e]
  | .deleteFile p c =>
    intro w w' k h
    obtain ⟨hg, rfl⟩ := apply_delete_file_inv w w' p c h
    have inv : inverse (.deleteFile p c) = .batch [.createFile p, .insert p 0 c] := by rw [inverse]
    rw [inv]
    have kr := keys_remove w k p
    have none' : (w.remove p).get p = none := by rw [get_remove w k, if_pos rfl]
    have get' : ((w.remove p).set p "").get p = some ⟨[]⟩ := by
      rw [get_set _ kr, if_pos rfl]
    refine ⟨((w.remove p).set p "").set p (String.mk ([].take 0) ++ c ++ String.mk ([].drop 0)), ?_, ?_⟩
    · rw [apply_batch, List.foldlM_cons, apply_create _ p none']
      show (List.foldlM apply ((w.remove p).set p "") [.insert p 0 c]) = _
      rw [List.foldlM_cons, apply_insert _ p 0 c [] get' (Nat.zero_le _)]
      rfl
    · have cc : String.mk ([].take 0) ++ c ++ String.mk ([].drop 0) = c := by
        rcases c with ⟨cs⟩
        show String.mk ([] ++ cs ++ []) = _
        simp
      rw [cc]
      intro q
      rw [get_set _ (keys_set _ kr p ""), get_set _ kr, get_remove w k]
      by_cases e : q = p
      · subst e; simp [hg]
      · simp [e]
  | .renameFile a b =>
    intro w w' k h
    obtain ⟨c, ha, hb, rfl⟩ := apply_rename_inv w w' a b h
    have inv : inverse (.renameFile a b) = .renameFile b a := by rw [inverse]
    rw [inv]
    have ne : a ≠ b := by
      intro e; subst e; rw [ha] at hb; cases hb
    have kr := keys_remove w k a
    have ks := keys_set _ kr b c
    have gb : ((w.remove a).set b c).get b = some c := by rw [get_set _ kr, if_pos rfl]
    have ga : ((w.remove a).set b c).get a = none := by
      rw [get_set _ kr, if_neg ne, get_remove w k, if_pos rfl]
    refine ⟨_, apply_rename _ b a c gb ga, ?_⟩
    intro q
    rw [get_set _ (keys_remove _ ks b), get_remove _ ks, get_set _ kr, get_remove w k]
    by_cases ea : q = a
    · subst ea; simp [ne, ha]
    · by_cases eb : q = b
      · subst eb; simp [ea, hb]
      · simp [ea, eb]
  | .batch cs =>
    intro w w' k h
    rw [apply_batch] at h
    rw [batch_inverse_is_reversed, apply_batch]
    exact restores_fold cs (fun c' hc' => restores c') w w' k h
termination_by sizeOf c
decreasing_by
  simp_wf
  have := List.sizeOf_lt_of_mem hc'
  omega

theorem keys_push (acc : List (String × String)) (nd : (acc.map (·.1)).Nodup) (n c : String) :
    ((acc.filter (·.1 != n) ++ [(n, c)]).map (·.1)).Nodup := by
  rw [List.map_append]
  unfold List.Nodup
  rw [List.pairwise_append]
  refine ⟨keys_filter acc nd _, List.pairwise_singleton _ _, ?_⟩
  intro a ha b hb
  simp only [List.map_cons, List.map_nil, List.mem_singleton] at hb
  subst hb
  simp only [List.mem_map, List.mem_filter, bne_iff_ne, ne_eq] at ha
  obtain ⟨x, ⟨_, ne⟩, rfl⟩ := ha
  exact ne

theorem keys_canon (w : Workspace) : Keys w.canon := by
  have go : ∀ (l acc : List (String × String)), (acc.map (·.1)).Nodup →
      ((l.foldl (fun acc (x : String × String) => match x with
        | (name, content) => (acc.filter (·.1 != name)) ++ [(name, content)]) acc).map (·.1)).Nodup := by
    intro l
    induction l with
    | nil => intro acc h; exact h
    | cons x rest ih =>
      intro acc h
      rcases x with ⟨n, c⟩
      exact ih _ (keys_push acc h n c)
  unfold Keys Workspace.canon
  exact List.Nodup.perm (go w.files [] List.nodup_nil) ((List.mergeSort_perm _ _).map _).symm

/-- @proves REQ-CMD.round_trip -/
theorem applying_then_inverting_restores_the_state (w w' : Workspace) (c : Command)
    (h : apply w.canon c = .ok w') :
    ∃ r, roundTrip w.canon c = .ok r ∧ ∀ path, r.get path = w.canon.get path := by
  obtain ⟨r, hr, same⟩ := restores c w.canon w' (keys_canon w) h
  refine ⟨r, ?_, same⟩
  unfold roundTrip
  rw [h]
  exact hr

/-- @proves REQ-CMD.total_or_refused -/
theorem a_refused_member_refuses_the_whole_batch (w w1 : Workspace) (pre rest : List Command)
    (c : Command) (e : Refusal) (done : apply w (.batch pre) = .ok w1)
    (refused : apply w1 c = .error e) :
    apply w (.batch (pre ++ c :: rest)) = .error e := by
  rw [apply_batch] at done ⊢
  rw [List.foldlM_append, done]
  show (c :: rest).foldlM apply w1 = _
  rw [List.foldlM_cons, refused]
  rfl

/-- @proves REQ-CMD.total_or_refused -/
theorem a_command_that_does_not_fit_is_refused_with_its_reason (w : Workspace) (f : String)
    (o : Nat) (t : String) (cs : List Char) :
    (w.get f = none → apply w (.insert f o t) = .error (.noSuchFile f)) ∧
    (w.get f = none → apply w (.delete f o t) = .error (.noSuchFile f)) ∧
    (w.get f = some ⟨cs⟩ → o > cs.length →
      apply w (.insert f o t) = .error (.offsetOutOfRange f o cs.length)) ∧
    (w.get f = some ⟨cs⟩ → o + t.length ≤ cs.length → String.mk ((cs.drop o).take t.length) ≠ t →
      apply w (.delete f o t) = .error (.witnessMismatch f o)) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro hg; simp [apply, hg]
  · intro hg; simp [apply, hg]
  · intro hg hgt
    have : o > (String.mk cs).length := hgt
    simp only [apply, hg, this, if_true]
    rfl
  · intro hg hle hne
    have : ¬ o + t.length > (String.mk cs).length := by
      simp only [len_mk] at *; (try simp only [len_data] at *); omega
    simp only [apply, hg, this, if_false, splitAt, String.toList]
    simp [hne]

/-- @proves REQ-CMD.witness_carried -/
theorem a_destructive_command_carries_what_it_destroys (w w' : Workspace) (f : String) (o : Nat)
    (d p c : String) :
    (apply w (.delete f o d) = .ok w' →
      ∃ cs : List Char, w.get f = some ⟨cs⟩ ∧ String.mk ((cs.drop o).take d.length) = d) ∧
    (apply w (.deleteFile p c) = .ok w' → w.get p = some c) := by
  constructor
  · intro h
    obtain ⟨cs, hg, _, hw, _⟩ := apply_delete_inv w w' f o d h
    exact ⟨cs, hg, hw⟩
  · intro h
    exact (apply_delete_file_inv w w' p c h).1

end TraceLean.Command
