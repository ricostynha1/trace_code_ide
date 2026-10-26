import TraceLean.Command
import TraceLean.Policy
import TraceLean.Hash
import TraceLean.SpecSorted
import Lean

namespace TraceLean.Mirror

open TraceLean.Command
open TraceLean.Policy

open Lean (ToJson FromJson)

def touchedPaths (before after : Workspace) : List String :=
  let names := (before.files.map (·.1) ++ after.files.map (·.1)).filter isMirrored
  (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) []).mergeSort (· <= ·)

private def mirrorStep (before after : Workspace) (path : String)
    : List Command × List Command × List Command :=
    match before.get path, after.get path with
    | some old, none => ([Command.deleteFile path old], [], [])
    | none, some new =>
      ([], Command.createFile path ::
        (if new == "" then [] else [Command.insert path 0 new]), [])
    | some old, some new =>
      if old == new then ([], [], [])
      else
        ([], [],
          (if old == "" then [] else [Command.delete path 0 old]) ++
          (if new == "" then [] else [Command.insert path 0 new]))
    | none, none => ([], [], [])

def mutations (before after : Workspace) : List Command :=
  let parts := (touchedPaths before after).map (mirrorStep before after)
  (parts.bind (·.1)) ++ (parts.bind (·.2.1)) ++ (parts.bind (·.2.2))

structure Mirroring where
  reached : Workspace
  refused : List Command
  deriving Repr, Inhabited, ToJson, FromJson

private def mirrorApply (acc : Mirroring) (c : Command) : Mirroring :=
  match apply acc.reached c with
  | .ok next => { acc with reached := next }
  | .error _ => { acc with refused := acc.refused ++ [c] }

/-- @models REQ-MIRROR.apply_reproduces -/
def mirrored (before after : Workspace) : Mirroring :=
  let b := before.canon
  let a := after.canon
  (mutations b a).foldl mirrorApply { reached := b, refused := [] }

/-- @models REQ-MIRROR.diff_is_pure
@models REQ-MIRROR.minimal
@models REQ-MIRROR.ordering_defined
@models REQ-MIRROR.protected_excluded -/
def mutationsOf (before after : Workspace) : List Command :=
  mutations before.canon after.canon

structure SelfWrite where
  path : String
  content : String
  age : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Suppression where
  suppressed : Bool
  pending : List SelfWrite
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def decrement (n : Nat) : Nat := n - 1

/-- @models REQ-SELFWRITE.own_writes_ignored
@models REQ-SELFWRITE.suppression_is_consumed
@models REQ-SELFWRITE.content_matched
@models REQ-SELFWRITE.unmatched_is_external -/
def suppress (pending : List SelfWrite) (path content : String) : Suppression :=
  let step := fun (acc : Bool × List SelfWrite) (write : SelfWrite) =>
    if !acc.1 && write.path == path && write.content == content then (true, acc.2)
    else (acc.1, acc.2 ++ [write])
  let result := pending.foldl step (false, [])
  { suppressed := result.1, pending := result.2 }

/-- @models REQ-SELFWRITE.no_deadlock -/
def expire (pending : List SelfWrite) : List SelfWrite :=
  (pending.map (fun w => { w with age := decrement w.age })).filter (·.age != 0)

/-- @proves REQ-SELFWRITE.unmatched_is_external -/
theorem nothing_pending_is_external (path content : String) :
    suppress [] path content = { suppressed := false, pending := [] } := by
  rfl

/-- @proves REQ-SELFWRITE.suppression_is_consumed -/
theorem one_write_suppresses_one_observation (path content : String) :
    (suppress [{ path := path, content := content, age := 3 }] path content).suppressed = true ∧
    (suppress (suppress [{ path := path, content := content, age := 3 }] path content).pending
      path content).suppressed = false := by
  simp [suppress]

inductive FileState where
  | absent
  | text (content : String)
  | opaque (hash : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Change where
  | mirror (command : Command)
  | reportOnly (path : String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- @models REQ-MIRROR.binary_handled -/
def changeAt (path : String) (before after : FileState) : List Change :=
  if !isMirrored path then []
  else
    match before, after with
    | .absent, .absent => []
    | .opaque old, .opaque new => if old == new then [] else [.reportOnly path]
    | .opaque _, _ => [.reportOnly path]
    | _, .opaque _ => [.reportOnly path]
    | .absent, .text content =>
      Change.mirror (Command.createFile path) ::
        (if content == "" then [] else [.mirror (Command.insert path 0 content)])
    | .text content, .absent => [.mirror (Command.deleteFile path content)]
    | .text old, .text new =>
      if old == new then []
      else
        (if old == "" then [] else [Change.mirror (Command.delete path 0 old)]) ++
        (if new == "" then [] else [Change.mirror (Command.insert path 0 new)])

def Change.isReportOnly : Change → Bool
  | .mirror _ => false
  | .reportOnly _ => true

/-- @proves REQ-MIRROR.binary_handled -/
theorem an_opaque_file_is_never_mirrored (path hash : String) (after : FileState) :
    (changeAt path (.opaque hash) after).all Change.isReportOnly = true := by
  unfold changeAt
  cases after
  all_goals split
  all_goals simp [Change.isReportOnly]
  all_goals split
  all_goals simp [Change.isReportOnly]

def commandPath : Command → String
  | .insert file _ _ => file
  | .delete file _ _ => file
  | .createFile path => path
  | .deleteFile path _ => path
  | .renameFile _ _ => ""
  | .batch _ => ""

def touchedByAgent (base : List (String × String)) (agent : Workspace) (path : String) : Bool :=
  (agent.get path).map TraceLean.Hash.digest != (base.reverse.find? (·.1 == path)).map (·.2)

/-- @models REQ-OBS.only_what_the_tool_changed -/
def changedSince (base : Option (List (String × String))) (project agent : Workspace) : List Command :=
  match base with
  | none => []
  | some b => (mutationsOf project agent).filter
      (fun c => commandPath c == "" || touchedByAgent b agent.canon (commandPath c))

/-- @proves REQ-SELFWRITE.own_writes_ignored -/
theorem an_own_write_is_suppressed (path content : String) :
    (suppress [⟨path, content, 3⟩] path content).suppressed = true := by
  simp [suppress]

/-- @proves REQ-SELFWRITE.content_matched -/
theorem other_content_at_the_path_is_external (path written seen : String) (h : written ≠ seen) :
    suppress [⟨path, written, 3⟩] path seen = { suppressed := false, pending := [⟨path, written, 3⟩] } := by
  simp [suppress, h]

/-- @proves REQ-SELFWRITE.no_deadlock -/
theorem an_unobserved_write_expires (path content : String) :
    expire (expire [⟨path, content, 2⟩]) = [] ∧ expire [⟨path, content, 1⟩] = [] := by
  simp [expire, decrement]

open TraceLean.SpecSorted (str_lt_trans str_antisymm str_le_trans str_lt_asymm ascending_unique)

def Good (l : List (String × String)) : Prop := List.Pairwise (fun x y => x.1 < y.1) l

def Distinct (l : List (String × String)) : Prop := List.Pairwise (fun x y => x.1 ≠ y.1) l

theorem distinct_of_good {l : List (String × String)} (h : Good l) : Distinct l := by
  apply List.Pairwise.imp _ h
  intro x y hxy e
  rw [e] at hxy
  exact String.lt_irrefl _ hxy

theorem good_of_sorted {l : List (String × String)}
    (hs : List.Pairwise (fun x y => decide (x.1 ≤ y.1) = true) l) (hd : Distinct l) : Good l := by
  apply List.Pairwise.imp _ (List.Pairwise.and hs hd)
  intro x y h
  have hle : ¬ y.1 < x.1 := by
    simpa using h.1
  refine (Classical.em (x.1 < y.1)).elim id ?_
  intro hn
  exact absurd (str_antisymm hn hle) h.2

theorem sorted_mergeSort_pairs (l : List (String × String)) :
    List.Pairwise (fun x y => decide (x.1 ≤ y.1) = true)
      (l.mergeSort (fun a b => decide (a.1 ≤ b.1))) := by
  apply List.mergeSort_sorted
  · intro a b c hab hbc
    have h1 : ¬ b.1 < a.1 := by
      simpa using hab
    have h2 : ¬ c.1 < b.1 := by
      simpa using hbc
    have h3 := str_le_trans h1 h2
    simpa using h3
  · intro a b hab
    have h1 : ¬ a.1 ≤ b.1 := by
      simpa using hab
    have h2 : b.1 < a.1 := Classical.not_not.1 h1
    have h3 := str_lt_asymm h2
    simpa using h3

theorem good_mergeSort (l : List (String × String)) (hd : Distinct l) :
    Good (l.mergeSort (fun a b => decide (a.1 ≤ b.1))) := by
  apply good_of_sorted (sorted_mergeSort_pairs l)
  have p := List.mergeSort_perm (fun a b => decide (a.1 ≤ b.1)) l
  exact (p.pairwise_iff (fun h e => h e.symm)).2 hd

theorem get_eq_some_iff (l : List (String × String)) (hd : Distinct l) (p v : String) :
    (Workspace.mk l).get p = some v ↔ (p, v) ∈ l := by
  induction l with
  | nil => simp [Workspace.get]
  | cons x rest ih =>
    have hd' := List.pairwise_cons.1 hd
    have ih' := ih hd'.2
    simp only [Workspace.get] at ih' ⊢
    by_cases hx : x.1 = p
    · simp only [List.find?_cons, hx, beq_self_eq_true, Option.map_some', Option.some.injEq,
        List.mem_cons]
      constructor
      · intro h
        left
        rw [← hx, ← h]
      · intro h
        rcases h with h | h
        · rw [← h]
        · exact absurd hx (hd'.1 _ h)
    · have hb : (x.1 == p) = false := by
        simpa using hx
      simp only [List.find?_cons, hb, List.mem_cons]
      rw [ih']
      constructor
      · intro h
        exact Or.inr h
      · intro h
        rcases h with h | h
        · rw [← h] at hx
          exact absurd rfl hx
        · exact h

theorem get_eq_of_mem_iff (l1 l2 : List (String × String)) (h1 : Distinct l1) (h2 : Distinct l2)
    (q : String) (h : ∀ v, (q, v) ∈ l1 ↔ (q, v) ∈ l2) :
    (Workspace.mk l1).get q = (Workspace.mk l2).get q := by
  apply Option.ext
  intro v
  show _ = some v ↔ _ = some v
  rw [get_eq_some_iff l1 h1, get_eq_some_iff l2 h2]
  exact h v

theorem good_set (w : Workspace) (hg : Good w.files) (p c : String) :
    Good (w.set p c).files ∧ ∀ q, (w.set p c).get q = if q = p then some c else w.get q := by
  have hdf : Distinct (w.files.filter (·.1 != p) ++ [(p, c)]) := by
    apply List.pairwise_append.2
    refine ⟨List.Pairwise.filter _ (distinct_of_good hg), by simp, ?_⟩
    intro a ha b hb
    simp only [List.mem_singleton] at hb
    subst hb
    simp only [List.mem_filter, bne_iff_ne, ne_eq] at ha
    exact ha.2
  have hgs : Good (w.set p c).files := good_mergeSort _ hdf
  refine ⟨hgs, ?_⟩
  intro q
  have hmem : ∀ x, x ∈ (w.set p c).files ↔ (x ∈ w.files ∧ x.1 ≠ p) ∨ x = (p, c) := by
    intro x
    simp [Workspace.set, List.mem_mergeSort]
  by_cases hq : q = p
  · subst hq
    simp only [if_true]
    exact (get_eq_some_iff _ (distinct_of_good hgs) q c).2 ((hmem _).2 (Or.inr rfl))
  · simp only [hq, if_false]
    apply get_eq_of_mem_iff _ _ (distinct_of_good hgs) (distinct_of_good hg)
    intro v
    rw [hmem]
    constructor
    · intro h
      rcases h with h | h
      · exact h.1
      · simp only [Prod.mk.injEq] at h
        exact absurd h.1 hq
    · intro h
      exact Or.inl ⟨h, hq⟩

theorem good_remove (w : Workspace) (hg : Good w.files) (p : String) :
    Good (w.remove p).files ∧ ∀ q, (w.remove p).get q = if q = p then none else w.get q := by
  have hgr : Good (w.remove p).files := List.Pairwise.filter _ hg
  refine ⟨hgr, ?_⟩
  intro q
  by_cases hq : q = p
  · subst hq
    simp only [if_true, Workspace.remove, Workspace.get]
    rw [Option.map_eq_none', List.find?_eq_none]
    intro x hx
    simp only [List.mem_filter, bne_iff_ne, ne_eq] at hx
    simpa using hx.2
  · simp only [hq, if_false]
    apply get_eq_of_mem_iff _ _ (distinct_of_good hgr) (distinct_of_good hg)
    intro v
    simp only [Workspace.remove, List.mem_filter, bne_iff_ne, ne_eq, decide_eq_true_eq]
    constructor
    · intro h
      exact h.1
    · intro h
      exact ⟨h, hq⟩

theorem dedup_distinct (xs : List (String × String)) : ∀ acc : List (String × String), Distinct acc →
    Distinct (xs.foldl (fun acc (x : String × String) => (acc.filter (·.1 != x.1)) ++ [x]) acc) := by
  induction xs with
  | nil => intro acc h; exact h
  | cons x _ ih =>
    intro acc h
    apply ih
    apply List.pairwise_append.2
    refine ⟨List.Pairwise.filter _ h, by simp, ?_⟩
    intro a ha b hb
    simp only [List.mem_singleton] at hb
    subst hb
    simp only [List.mem_filter, bne_iff_ne, ne_eq] at ha
    exact ha.2

theorem good_canon (w : Workspace) : Good w.canon.files := by
  apply good_mergeSort
  have := dedup_distinct w.files [] List.Pairwise.nil
  exact this

theorem good_ext (l1 l2 : List (String × String)) (h1 : Good l1) (h2 : Good l2)
    (h : ∀ q, (Workspace.mk l1).get q = (Workspace.mk l2).get q) : l1 = l2 := by
  apply ascending_unique (fun (x y : String × String) => x.1 < y.1) (fun x => String.lt_irrefl x.1)
    (fun _ _ _ hab hbc => str_lt_trans hab hbc) l1 l2 h1 h2
  intro x
  show x ∈ l1 ↔ x ∈ l2
  have e1 := get_eq_some_iff l1 (distinct_of_good h1) x.1 x.2
  have e2 := get_eq_some_iff l2 (distinct_of_good h2) x.1 x.2
  rw [← e1, ← e2, h]

def localStep : Option String → Command → Option (Option String)
  | some content, .insert _ offset text =>
    if offset > content.length then none
    else
      let (before, after) := splitAt content offset
      some (some (before ++ text ++ after))
  | some content, .delete _ offset deleted =>
    if offset + deleted.length > content.length then none
    else
      let (before, rest) := splitAt content offset
      let (removed, after) := splitAt rest deleted.length
      if removed != deleted then none else some (some (before ++ after))
  | none, .createFile _ => some (some "")
  | some actual, .deleteFile _ content => if actual != content then none else some none
  | _, _ => none

def simple : Command → Bool
  | .renameFile _ _ => false
  | .batch _ => false
  | _ => true

theorem apply_local (S : Workspace) (c : Command) (hg : Good S.files) (hs : simple c = true) :
    (localStep (S.get (commandPath c)) c = none ∧ ∃ e, apply S c = .error e) ∨
    ∃ v S', localStep (S.get (commandPath c)) c = some v ∧ apply S c = .ok S' ∧ Good S'.files ∧
      ∀ q, S'.get q = if q = commandPath c then v else S.get q := by
  cases c with
  | insert file offset text =>
    simp only [commandPath]
    cases hget : S.get file with
    | none =>
      left
      refine ⟨rfl, ?_⟩
      rw [apply]
      simp [hget]
    | some content =>
      by_cases ho : offset > content.length
      · left
        refine ⟨by simp [localStep, ho], ?_⟩
        rw [apply]
        simp [hget, ho]
      · right
        have hset := good_set S hg file
          ((splitAt content offset).1 ++ text ++ (splitAt content offset).2)
        refine ⟨_, _, by simp [localStep, ho], ?_, hset.1, hset.2⟩
        rw [apply]
        simp [hget, ho]
  | delete file offset deleted =>
    simp only [commandPath]
    cases hget : S.get file with
    | none =>
      left
      refine ⟨rfl, ?_⟩
      rw [apply]
      simp [hget]
    | some content =>
      by_cases ho : offset + deleted.length > content.length
      · left
        refine ⟨by simp [localStep, ho], ?_⟩
        rw [apply]
        simp [hget, ho]
      · by_cases hw : (splitAt (splitAt content offset).2 deleted.length).1 = deleted
        · right
          have hset := good_set S hg file
            ((splitAt content offset).1 ++ (splitAt (splitAt content offset).2 deleted.length).2)
          refine ⟨_, _, by simp [localStep, ho, hw], ?_, hset.1, hset.2⟩
          rw [apply]
          simp [hget, ho, hw]
        · left
          refine ⟨by simp [localStep, ho, hw], ?_⟩
          rw [apply]
          simp [hget, ho, hw]
  | createFile path =>
    simp only [commandPath]
    cases hget : S.get path with
    | none =>
      right
      have hset := good_set S hg path ""
      refine ⟨_, _, rfl, ?_, hset.1, hset.2⟩
      rw [apply]
      simp [Workspace.contains, hget]
    | some content =>
      left
      refine ⟨rfl, ?_⟩
      rw [apply]
      simp [Workspace.contains, hget]
  | deleteFile path content =>
    simp only [commandPath]
    cases hget : S.get path with
    | none =>
      left
      refine ⟨rfl, ?_⟩
      rw [apply]
      simp [hget]
    | some actual =>
      by_cases ha : actual = content
      · right
        have hrem := good_remove S hg path
        refine ⟨_, _, by simp [localStep, ha], ?_, hrem.1, hrem.2⟩
        rw [apply]
        simp [hget, ha]
      · left
        refine ⟨by simp [localStep, ha], ?_⟩
        rw [apply]
        simp [hget, ha]
  | renameFile a b => simp [simple] at hs
  | batch cs => simp [simple] at hs

def localRun : Option String → List Command → Option (Option String)
  | s, [] => some s
  | s, c :: cs => (localStep s c).bind (fun t => localRun t cs)

def onPath (q : String) (cs : List Command) : List Command :=
  cs.filter (fun c => commandPath c == q)

theorem refused_grows (cs : List Command) :
    ∀ acc : Mirroring, ∃ e, (cs.foldl mirrorApply acc).refused = acc.refused ++ e := by
  induction cs with
  | nil => intro acc; exact ⟨[], by simp⟩
  | cons c rest ih =>
    intro acc
    simp only [List.foldl_cons]
    obtain ⟨e, he⟩ := ih (mirrorApply acc c)
    unfold mirrorApply at he ⊢
    split at he
    · exact ⟨e, by simp_all⟩
    · exact ⟨[c] ++ e, by simp_all⟩

theorem run_clean (cs : List Command) : ∀ S : Workspace, Good S.files → (∀ c ∈ cs, simple c = true) →
    (cs.foldl mirrorApply { reached := S, refused := [] }).refused = [] →
    ∀ q, localRun (S.get q) (onPath q cs) =
      some ((cs.foldl mirrorApply { reached := S, refused := [] }).reached.get q) := by
  induction cs with
  | nil =>
    intro S _ _ _ q
    simp [localRun, onPath]
  | cons c rest ih =>
    intro S hg hs hr q
    have hc := hs c (List.mem_cons_self _ _)
    have hrest : ∀ c ∈ rest, simple c = true := fun c' h => hs c' (List.mem_cons_of_mem _ h)
    simp only [List.foldl_cons] at hr ⊢
    rcases apply_local S c hg hc with ⟨_, e, he⟩ | ⟨v, S', hv, he, hg', hget⟩
    · have hm : mirrorApply { reached := S, refused := [] } c = { reached := S, refused := [c] } := by
        simp [mirrorApply, he]
      rw [hm] at hr
      obtain ⟨e', he'⟩ := refused_grows rest { reached := S, refused := [c] }
      rw [hr] at he'
      simp at he'
    · have hm : mirrorApply { reached := S, refused := [] } c = { reached := S', refused := [] } := by
        simp [mirrorApply, he]
      rw [hm] at hr ⊢
      have ih' := ih S' hg' hrest hr q
      by_cases hq : commandPath c = q
      · have hon : onPath q (c :: rest) = c :: onPath q rest := by
          simp [onPath, hq]
        subst hq
        have e : S'.get (commandPath c) = v := by rw [hget, if_pos rfl]
        rw [hon, localRun, hv]
        show localRun v (onPath (commandPath c) rest) = _
        rw [← e]
        exact ih'
      · have hon : onPath q (c :: rest) = onPath q rest := by
          simp [onPath, hq]
        rw [hon, ← ih', hget, if_neg (Ne.symm hq)]

theorem run_succeeds (cs : List Command) : ∀ S : Workspace, Good S.files →
    (∀ c ∈ cs, simple c = true) → (∀ q, (localRun (S.get q) (onPath q cs)).isSome = true) →
    (cs.foldl mirrorApply { reached := S, refused := [] }).refused = [] ∧
    Good (cs.foldl mirrorApply { reached := S, refused := [] }).reached.files := by
  induction cs with
  | nil =>
    intro S hg _ _
    exact ⟨rfl, hg⟩
  | cons c rest ih =>
    intro S hg hs hrun
    have hc := hs c (List.mem_cons_self _ _)
    have hrest : ∀ c ∈ rest, simple c = true := fun c' h => hs c' (List.mem_cons_of_mem _ h)
    simp only [List.foldl_cons]
    have hon : onPath (commandPath c) (c :: rest) = c :: onPath (commandPath c) rest := by
      simp [onPath]
    rcases apply_local S c hg hc with ⟨hnone, _⟩ | ⟨v, S', hv, he, hg', hget⟩
    · have h := hrun (commandPath c)
      rw [hon, localRun, hnone] at h
      simp at h
    · have hm : mirrorApply { reached := S, refused := [] } c = { reached := S', refused := [] } := by
        simp [mirrorApply, he]
      rw [hm]
      apply ih S' hg' hrest
      intro q
      have h := hrun q
      by_cases hq : commandPath c = q
      · subst hq
        rw [hon, localRun, hv] at h
        rw [hget, if_pos rfl]
        exact h
      · have hon' : onPath q (c :: rest) = onPath q rest := by
          simp [onPath, hq]
        rw [hon'] at h
        rw [hget, if_neg (Ne.symm hq)]
        exact h

theorem dedup_names (names : List String) : ∀ acc : List String, acc.Nodup →
    (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) acc).Nodup ∧
    ∀ x, x ∈ names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) acc ↔
      x ∈ acc ∨ x ∈ names := by
  induction names with
  | nil => intro acc h; simp [h]
  | cons n rest ih =>
    intro acc h
    simp only [List.foldl_cons]
    by_cases hn : acc.contains n = true
    · simp only [hn, if_true]
      obtain ⟨h1, h2⟩ := ih acc h
      refine ⟨h1, fun x => ?_⟩
      rw [h2]
      have hn' : n ∈ acc := by simpa using hn
      simp only [List.mem_cons]
      constructor
      · intro h
        rcases h with h | h
        · exact Or.inl h
        · exact Or.inr (Or.inr h)
      · intro h
        rcases h with h | h | h
        · exact Or.inl h
        · exact Or.inl (h ▸ hn')
        · exact Or.inr h
    · have hb : acc.contains n = false := by simpa using hn
      simp only [hb, Bool.false_eq_true, if_false]
      have hn' : n ∉ acc := by simpa using hn
      have hnd : (acc ++ [n]).Nodup := by
        apply List.pairwise_append.2
        refine ⟨h, by simp, ?_⟩
        intro x hx y hy e
        simp only [List.mem_singleton] at hy
        subst hy e
        exact hn' hx
      obtain ⟨h1, h2⟩ := ih (acc ++ [n]) hnd
      refine ⟨h1, fun x => ?_⟩
      rw [h2]
      simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · intro h
        rcases h with (h | h) | h
        · exact Or.inl h
        · exact Or.inr (Or.inl h)
        · exact Or.inr (Or.inr h)
      · intro h
        rcases h with h | h | h
        · exact Or.inl (Or.inl h)
        · exact Or.inl (Or.inr h)
        · exact Or.inr h

theorem touched_spec (b a : Workspace) : (touchedPaths b a).Nodup ∧
    ∀ q, q ∈ touchedPaths b a ↔
      isMirrored q = true ∧ (q ∈ b.files.map (·.1) ∨ q ∈ a.files.map (·.1)) := by
  obtain ⟨h1, h2⟩ := dedup_names ((b.files.map (·.1) ++ a.files.map (·.1)).filter isMirrored) []
    List.nodup_nil
  have p := List.mergeSort_perm (fun x y : String => decide (x ≤ y))
    (((b.files.map (·.1) ++ a.files.map (·.1)).filter isMirrored).foldl
      (fun acc n => if acc.contains n then acc else acc ++ [n]) [])
  refine ⟨p.nodup_iff.2 h1, fun q => ?_⟩
  simp only [touchedPaths]
  rw [List.mem_mergeSort, h2]
  simp only [List.not_mem_nil, false_or, List.mem_filter, List.mem_append]
  exact And.comm

theorem get_none_of_absent (w : Workspace) (q : String) (h : q ∉ w.files.map (·.1)) :
    w.get q = none := by
  simp only [Workspace.get]
  rw [Option.map_eq_none', List.find?_eq_none]
  intro x hx e
  apply h
  simp only [beq_iff_eq] at e
  exact List.mem_map.2 ⟨x, hx, e⟩

def seqOf (b a : Workspace) (p : String) : List Command :=
  (mirrorStep b a p).1 ++ (mirrorStep b a p).2.1 ++ (mirrorStep b a p).2.2

theorem step_paths (b a : Workspace) (p : String) (c : Command)
    (h : c ∈ (mirrorStep b a p).1 ∨ c ∈ (mirrorStep b a p).2.1 ∨ c ∈ (mirrorStep b a p).2.2) :
    commandPath c = p ∧ simple c = true := by
  unfold mirrorStep at h
  split at h <;> (repeat' split at h) <;>
    (try simp only [List.mem_cons, List.mem_append, List.not_mem_nil, or_false, false_or] at h) <;>
    (repeat' rcases h with h | h) <;> (try subst h) <;> exact ⟨rfl, rfl⟩

theorem onPath_bind (g : String → List Command) (hg : ∀ p c, c ∈ g p → commandPath c = p)
    (q : String) : ∀ P : List String, P.Nodup →
    onPath q (P.bind g) = if q ∈ P then g q else [] := by
  intro P
  induction P with
  | nil => intro _; simp [onPath]
  | cons p rest ih =>
    intro hnd
    have hnd' := List.nodup_cons.1 hnd
    rw [List.bind_cons, onPath, List.filter_append, ← onPath, ← onPath, ih hnd'.2]
    by_cases hq : q = p
    · subst hq
      have e : onPath q (g q) = g q := by
        apply List.filter_eq_self.2
        intro c hc
        simp [hg q c hc]
      simp [e, hnd'.1]
    · have e : onPath q (g p) = [] := by
        apply List.filter_eq_nil.2
        intro c hc
        simp only [hg p c hc, beq_iff_eq]
        exact Ne.symm hq
      simp [e, hq]

theorem mutations_eq (b a : Workspace) : mutations b a =
    (touchedPaths b a).bind (fun p => (mirrorStep b a p).1) ++
    (touchedPaths b a).bind (fun p => (mirrorStep b a p).2.1) ++
    (touchedPaths b a).bind (fun p => (mirrorStep b a p).2.2) := by
  simp [mutations, List.bind_map]

theorem onPath_mutations (b a : Workspace) (q : String) :
    onPath q (mutations b a) = if q ∈ touchedPaths b a then seqOf b a q else [] := by
  have hnd := (touched_spec b a).1
  rw [mutations_eq]
  simp only [onPath, List.filter_append]
  rw [← onPath, ← onPath, ← onPath]
  rw [onPath_bind _ (fun p c h => (step_paths b a p c (Or.inl h)).1) q _ hnd,
    onPath_bind _ (fun p c h => (step_paths b a p c (Or.inr (Or.inl h))).1) q _ hnd,
    onPath_bind _ (fun p c h => (step_paths b a p c (Or.inr (Or.inr h))).1) q _ hnd]
  by_cases hq : q ∈ touchedPaths b a <;> simp [hq, seqOf]

theorem mem_mutations (b a : Workspace) (c : Command) (h : c ∈ mutations b a) :
    ∃ p ∈ touchedPaths b a, c ∈ seqOf b a p := by
  rw [mutations_eq] at h
  simp only [List.mem_append, List.mem_bind] at h
  rcases h with (⟨p, hp, hc⟩ | ⟨p, hp, hc⟩) | ⟨p, hp, hc⟩ <;>
    exact ⟨p, hp, by simp [seqOf, hc]⟩

theorem simple_mutations (b a : Workspace) (c : Command) (h : c ∈ mutations b a) :
    simple c = true ∧ isMirrored (commandPath c) = true := by
  obtain ⟨p, hp, hc⟩ := mem_mutations b a c h
  simp only [seqOf, List.mem_append] at hc
  have hs := step_paths b a p c (by
    rcases hc with (h | h) | h
    · exact Or.inl h
    · exact Or.inr (Or.inl h)
    · exact Or.inr (Or.inr h))
  refine ⟨hs.2, ?_⟩
  rw [hs.1]
  exact ((touched_spec b a).2 p).1 hp |>.1

theorem splitAt_zero (s : String) : splitAt s 0 = ("", s) := by
  cases s
  rfl

theorem splitAt_length (s : String) : splitAt s s.length = (s, "") := by
  cases s
  simp [splitAt, String.length]

theorem empty_append' (s : String) : "" ++ s = s := by
  cases s
  rfl

theorem append_empty' (s : String) : s ++ "" = s := by
  cases s
  show String.mk (_ ++ []) = _
  rw [List.append_nil]

theorem append_ne_self (new old : String) (h : old ≠ "") : new ++ old ≠ new := by
  intro e
  apply h
  cases new
  cases old
  rename_i n o
  have e' : n ++ o = n := congrArg String.data e
  have : o = [] := by
    have hl := congrArg List.length e'
    simpa using hl
  subst this
  rfl

theorem step_deleteFile (p old : String) : localStep (some old) (.deleteFile p old) = some none := by
  simp [localStep]

theorem step_create (p : String) : localStep none (.createFile p) = some (some "") := rfl

theorem step_insert (p old new : String) :
    localStep (some old) (.insert p 0 new) = some (some (new ++ old)) := by
  simp [localStep, splitAt_zero, empty_append']

theorem step_delete (p old : String) : localStep (some old) (.delete p 0 old) = some (some "") := by
  simp [localStep, splitAt_zero, splitAt_length]

theorem step_insert_none (p new : String) : localStep none (.insert p 0 new) = none := rfl

theorem seq_reaches (b a : Workspace) (p : String) :
    localRun (b.get p) (seqOf b a p) = some (a.get p) := by
  unfold seqOf mirrorStep
  cases b.get p <;> cases a.get p
  · simp [localRun]
  · rename_i new
    by_cases hn : new = ""
    · simp [localRun, hn, step_create]
    · simp [localRun, hn, step_create, step_insert, append_empty']
  · rename_i old
    simp [localRun, step_deleteFile]
  · rename_i old new
    by_cases he : old = new
    · simp [localRun, he]
    · by_cases ho : old = "" <;> by_cases hn : new = "" <;>
        simp_all [localRun, step_insert, step_delete, append_empty', empty_append']

theorem list_one {α : Type} (F1 F2 : List α) (c x : α) (h : F1 ++ c :: F2 = [x]) :
    F1 = [] ∧ F2 = [] := by
  rcases F1 with _ | ⟨_, _ | ⟨_, _⟩⟩ <;> simp_all

theorem list_two {α : Type} (F1 F2 : List α) (c x y : α) (h : F1 ++ c :: F2 = [x, y]) :
    (F1 = [] ∧ F2 = [y]) ∨ (F1 = [x] ∧ F2 = []) := by
  rcases F1 with _ | ⟨_, _ | ⟨_, _ | ⟨_, _⟩⟩⟩ <;> simp_all

theorem seq_minimal (b a : Workspace) (p : String) (F1 F2 : List Command) (c : Command)
    (h : F1 ++ c :: F2 = seqOf b a p) : localRun (b.get p) (F1 ++ F2) ≠ some (a.get p) := by
  unfold seqOf mirrorStep at h
  cases hb : b.get p <;> cases ha : a.get p <;> simp only [hb, ha] at h
  · simp at h
  · rename_i new
    by_cases hn : new = ""
    · simp only [hn, beq_self_eq_true, if_true, List.append_nil, List.nil_append] at h
      obtain ⟨rfl, rfl⟩ := list_one _ _ _ _ h
      simp [localRun]
    · have hb' : (new == "") = false := by simpa using hn
      simp only [hb', Bool.false_eq_true, if_false, List.append_nil, List.nil_append] at h
      rcases list_two _ _ _ _ _ h with ⟨rfl, rfl⟩ | ⟨rfl, rfl⟩
      · simp [localRun, step_insert_none]
      · simp [localRun, step_create]
        first | exact he | exact fun e => he e.symm | exact Ne.symm hn | exact append_ne_self new old ho | simp_all
  · simp only [List.append_nil] at h
    obtain ⟨rfl, rfl⟩ := list_one _ _ _ _ h
    simp [localRun]
  · rename_i old new
    by_cases he : old = new
    · simp [he] at h
    · have he' : (old == new) = false := by simpa using he
      simp only [he', Bool.false_eq_true, if_false, List.nil_append] at h
      by_cases ho : old = "" <;> by_cases hn : new = ""
      · exact absurd (ho.trans hn.symm) he
      · have hn' : (new == "") = false := by simpa using hn
        simp only [ho, hn', beq_self_eq_true, if_true, Bool.false_eq_true, if_false,
          List.nil_append] at h
        obtain ⟨rfl, rfl⟩ := list_one _ _ _ _ h
        simp [localRun]
        first | exact he | exact fun e => he e.symm | exact Ne.symm hn | exact append_ne_self new old ho | simp_all
      · have ho' : (old == "") = false := by simpa using ho
        simp only [hn, ho', beq_self_eq_true, if_true, Bool.false_eq_true, if_false,
          List.append_nil] at h
        obtain ⟨rfl, rfl⟩ := list_one _ _ _ _ h
        simp [localRun]
        first | exact he | exact fun e => he e.symm | exact Ne.symm hn | exact append_ne_self new old ho | simp_all
      · have ho' : (old == "") = false := by simpa using ho
        have hn' : (new == "") = false := by simpa using hn
        simp only [hn', ho', Bool.false_eq_true, if_false, List.singleton_append] at h
        rcases list_two _ _ _ _ _ h with ⟨rfl, rfl⟩ | ⟨rfl, rfl⟩
        · simp [localRun, step_insert]
          first | exact he | exact fun e => he e.symm | exact Ne.symm hn | exact append_ne_self new old ho | simp_all
        · simp [localRun, step_delete]
          first | exact he | exact fun e => he e.symm | exact Ne.symm hn | exact append_ne_self new old ho | simp_all

theorem mutations_run (b a : Workspace) (hg : Good b.files) :
    ((mutations b a).foldl mirrorApply { reached := b, refused := [] }).refused = [] ∧
    Good ((mutations b a).foldl mirrorApply { reached := b, refused := [] }).reached.files ∧
    ∀ q, ((mutations b a).foldl mirrorApply { reached := b, refused := [] }).reached.get q =
      if q ∈ touchedPaths b a then a.get q else b.get q := by
  have hs : ∀ c ∈ mutations b a, simple c = true := fun c h => (simple_mutations b a c h).1
  have hrun : ∀ q, localRun (b.get q) (onPath q (mutations b a)) =
      some (if q ∈ touchedPaths b a then a.get q else b.get q) := by
    intro q
    rw [onPath_mutations]
    by_cases hq : q ∈ touchedPaths b a
    · simp only [hq, if_true]
      exact seq_reaches b a q
    · simp [hq, localRun]
  obtain ⟨h1, h2⟩ := run_succeeds (mutations b a) b hg hs (fun q => by rw [hrun q]; rfl)
  refine ⟨h1, h2, fun q => ?_⟩
  have h3 := run_clean (mutations b a) b hg hs h1 q
  rw [hrun q] at h3
  exact (Option.some.inj h3).symm

theorem reached_get (before after : Workspace) (q : String) :
    (mirrored before after).reached.get q =
      if isMirrored q = true then after.canon.get q else before.canon.get q := by
  simp only [mirrored]
  rw [(mutations_run before.canon after.canon (good_canon before)).2.2 q]
  have hspec := (touched_spec before.canon after.canon).2 q
  by_cases hm : isMirrored q = true
  · simp only [hm, if_true]
    by_cases hq : q ∈ touchedPaths before.canon after.canon
    · simp [hq]
    · simp only [hq, if_false]
      have hb : q ∉ before.canon.files.map (·.1) := fun h => hq (hspec.2 ⟨hm, Or.inl h⟩)
      have ha : q ∉ after.canon.files.map (·.1) := fun h => hq (hspec.2 ⟨hm, Or.inr h⟩)
      rw [get_none_of_absent _ _ hb, get_none_of_absent _ _ ha]
  · have hq : q ∉ touchedPaths before.canon after.canon := fun h => hm (hspec.1 h).1
    simp [hm, hq]

/-- @proves REQ-MIRROR.apply_reproduces -/
theorem applying_the_mutations_reproduces_the_tree (before after : Workspace) :
    (mirrored before after).refused = [] ∧
    (∀ p, isMirrored p = true → (mirrored before after).reached.get p = after.canon.get p) ∧
    (∀ p, isMirrored p = false → (mirrored before after).reached.get p = before.canon.get p) ∧
    ((∀ p, isMirrored p = false → before.canon.get p = after.canon.get p) →
      (mirrored before after).reached = after.canon) := by
  refine ⟨(mutations_run before.canon after.canon (good_canon before)).1, ?_, ?_, ?_⟩
  · intro p hp
    rw [reached_get, if_pos hp]
  · intro p hp
    rw [reached_get, hp]
    rfl
  · intro same
    have hg : Good (mirrored before after).reached.files :=
      (mutations_run before.canon after.canon (good_canon before)).2.1
    have e : (mirrored before after).reached.files = after.canon.files := by
      apply good_ext _ _ hg (good_canon after)
      intro q
      show (mirrored before after).reached.get q = after.canon.get q
      rw [reached_get]
      by_cases hm : isMirrored q = true
      · rw [if_pos hm]
      · rw [if_neg hm]
        exact same q (by simpa using hm)
    cases h : (mirrored before after).reached
    rw [h] at e
    cases hc : after.canon
    rw [hc] at e
    simp only at e
    rw [e]

/-- @proves REQ-MIRROR.diff_is_pure -/
theorem the_mutations_depend_only_on_the_two_trees (before before' after after' : Workspace)
    (hb : ∀ p, before.canon.get p = before'.canon.get p)
    (ha : ∀ p, after.canon.get p = after'.canon.get p) :
    mutationsOf before after = mutationsOf before' after' := by
  have eb : before.canon = before'.canon := by
    have e := good_ext _ _ (good_canon before) (good_canon before') hb
    cases h1 : before.canon
    cases h2 : before'.canon
    rw [h1, h2] at e
    simp only at e
    rw [e]
  have ea : after.canon = after'.canon := by
    have e := good_ext _ _ (good_canon after) (good_canon after') ha
    cases h1 : after.canon
    cases h2 : after'.canon
    rw [h1, h2] at e
    simp only at e
    rw [e]
  simp only [mutationsOf, eb, ea]

/-- @proves REQ-MIRROR.protected_excluded -/
theorem no_mutation_touches_a_protected_path (before after : Workspace) :
    ∀ c ∈ mutationsOf before after,
      isMirrored (commandPath c) = true ∧ classify (commandPath c) ≠ Class.«protected» := by
  intro c hc
  have hm := (simple_mutations _ _ c hc).2
  refine ⟨hm, fun hp => ?_⟩
  rw [protected_is_not_mirrored _ hp] at hm
  exact Bool.false_ne_true hm

/-- @proves REQ-MIRROR.ordering_defined -/
theorem each_mutation_finds_the_state_it_needs (before after : Workspace)
    (pre post : List Command) (c : Command) (split : mutationsOf before after = pre ++ c :: post) :
    ∃ next, apply (pre.foldl mirrorApply { reached := before.canon, refused := [] }).reached c =
      .ok next := by
  have hr := (applying_the_mutations_reproduces_the_tree before after).1
  simp only [mirrored] at hr
  have split' : mutations before.canon after.canon = (pre ++ [c]) ++ post := by
    rw [← mutationsOf, split]
    simp
  rw [split', List.foldl_append] at hr
  obtain ⟨e, he⟩ := refused_grows post
    ((pre ++ [c]).foldl mirrorApply { reached := before.canon, refused := [] })
  rw [hr] at he
  have hnil : ((pre ++ [c]).foldl mirrorApply { reached := before.canon, refused := [] }).refused =
      [] := by
    cases e' : ((pre ++ [c]).foldl mirrorApply { reached := before.canon, refused := [] }).refused
    · rfl
    · rw [e'] at he
      simp at he
  rw [List.foldl_append] at hnil
  simp only [List.foldl_cons, List.foldl_nil] at hnil
  unfold mirrorApply at hnil
  split at hnil
  · rename_i next hnext
    exact ⟨next, hnext⟩
  · simp at hnil

/-- @proves REQ-MIRROR.minimal -/
theorem no_mutation_can_be_dropped (before after : Workspace) (pre post : List Command)
    (c : Command) (split : mutationsOf before after = pre ++ c :: post) :
    ¬ (((pre ++ post).foldl mirrorApply { reached := before.canon, refused := [] }).refused = [] ∧
      ∀ p, isMirrored p = true →
        ((pre ++ post).foldl mirrorApply { reached := before.canon, refused := [] }).reached.get p =
          after.canon.get p) := by
  intro ⟨hr, hp⟩
  have hc : c ∈ mutations before.canon after.canon := by
    rw [← mutationsOf, split]
    simp
  have hsimple : ∀ x ∈ pre ++ post, simple x = true := by
    intro x hx
    apply (simple_mutations before.canon after.canon x _).1
    rw [← mutationsOf, split]
    simp only [List.mem_append, List.mem_cons] at hx ⊢
    rcases hx with hx | hx
    · exact Or.inl hx
    · exact Or.inr (Or.inr hx)
  have hclean := run_clean (pre ++ post) before.canon (good_canon before) hsimple hr (commandPath c)
  have hm := (simple_mutations _ _ c hc).2
  rw [hp _ hm] at hclean
  have hon := onPath_mutations before.canon after.canon (commandPath c)
  rw [← mutationsOf, split] at hon
  have hin : commandPath c ∈ touchedPaths before.canon after.canon := by
    by_cases hq : commandPath c ∈ touchedPaths before.canon after.canon
    · exact hq
    · rw [if_neg hq] at hon
      simp [onPath] at hon
  rw [if_pos hin] at hon
  have hsplit : onPath (commandPath c) pre ++ c :: onPath (commandPath c) post =
      seqOf before.canon after.canon (commandPath c) := by
    rw [← hon]
    simp [onPath]
  apply seq_minimal before.canon after.canon (commandPath c) _ _ c hsplit
  rw [← hclean]
  simp [onPath]

/-- @proves REQ-OBS.only_what_the_tool_changed -/
theorem only_the_tools_changes_are_offered (project agent : Workspace)
    (base : List (String × String)) :
    changedSince none project agent = [] ∧
    (∀ c, c ∈ changedSince (some base) project agent ↔
      c ∈ mutationsOf project agent ∧
        (commandPath c = "" ∨ touchedByAgent base agent.canon (commandPath c) = true)) ∧
    (∀ p, p ≠ "" → touchedByAgent base agent.canon p = false →
      ∀ c ∈ changedSince (some base) project agent, commandPath c ≠ p) := by
  refine ⟨rfl, ?_, ?_⟩
  · intro c
    simp [changedSince]
  · intro p hp hu c hc e
    simp only [changedSince, List.mem_filter, Bool.or_eq_true, beq_iff_eq] at hc
    rcases hc.2 with h | h
    · exact hp (e ▸ h)
    · rw [e, hu] at h
      exact Bool.false_ne_true h

end TraceLean.Mirror
