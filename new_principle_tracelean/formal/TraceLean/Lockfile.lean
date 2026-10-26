import TraceLean.Evidence
import TraceLean.Record
import Lean

namespace TraceLean.Lockfile

open TraceLean.Evidence
open TraceLean.Record

open Lean (ToJson FromJson)

structure LockLink where
  role : String
  req : String
  clause : Option String
  file : String
  anchor : String
  body_hash : String
  link_hash : String
  qualifier : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def optionLe (a b : Option String) : Bool :=
  match a, b with
  | none, _ => true
  | some _, none => false
  | some x, some y => decide (x ≤ y)

def optionEq (a b : Option String) : Bool := a == b

def lockLinkLe (a b : LockLink) : Bool :=
  if a.role != b.role then decide (a.role ≤ b.role)
  else if a.req != b.req then decide (a.req ≤ b.req)
  else if !optionEq a.clause b.clause then optionLe a.clause b.clause
  else if a.file != b.file then decide (a.file ≤ b.file)
  else if a.anchor != b.anchor then decide (a.anchor ≤ b.anchor)
  else if a.body_hash != b.body_hash then decide (a.body_hash ≤ b.body_hash)
  else if a.link_hash != b.link_hash then decide (a.link_hash ≤ b.link_hash)
  else optionLe a.qualifier b.qualifier

/-- @models REQ-LOCK.deterministic_bytes
@models ARCH-DETERMINISM.stable_ordering -/
def orderedLinks (links : List LockLink) : List LockLink :=
  links.mergeSort lockLinkLe

def lockVersion : Nat := 1

inductive VersionVerdict where
  | readable
  | unknown (found reads : Nat)
  | unstamped
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-LOCK.version_stamped -/
def versionVerdict (version : Option Nat) : VersionVerdict :=
  match version with
  | none => .unstamped
  | some v => if v == lockVersion then .readable else .unknown v lockVersion

/-- @models REQ-LOCK.evidence_preserved -/
def carriedEvidence (evidence : List Evidence) : List Evidence := evidence

theorem char_antisymm {x y : Char} (h1 : ¬ x < y) (h2 : ¬ y < x) : x = y := by
  apply Char.ext
  apply UInt32.eq_of_val_eq
  apply Fin.ext
  simp only [Char.lt_def, UInt32.lt_def, Fin.lt_def, Nat.not_lt] at h1 h2
  omega

theorem char_not_lt_trans {x y z : Char} (h1 : ¬ x < y) (h2 : ¬ y < z) : ¬ x < z := by
  simp only [Char.not_lt] at *
  exact Char.le_trans h2 h1

theorem str_lt_trans {a b c : String} (h1 : a < b) (h2 : b < c) : a < c :=
  List.lt_trans' (fun h h' => Char.lt_trans h h') char_not_lt_trans h1 h2

theorem str_antisymm {a b : String} (h1 : ¬ a < b) (h2 : ¬ b < a) : a = b := by
  cases a; cases b
  congr
  exact List.lt_antisymm' char_antisymm h1 h2

theorem str_le_total (a b : String) : a ≤ b ∨ b ≤ a := by
  by_cases h : b < a
  · exact Or.inr (fun h' => String.lt_irrefl a (str_lt_trans h' h))
  · exact Or.inl h

theorem str_le_trans {a b c : String} (h1 : a ≤ b) (h2 : b ≤ c) : a ≤ c := by
  intro h
  by_cases hab : a < b
  · exact h2 (str_lt_trans h hab)
  · have := str_antisymm hab h1
    subst this
    exact h2 h

def IsTrans {α : Type} (r : α → α → Bool) : Prop := ∀ a b c, r a b = true → r b c = true → r a c = true
def IsTotal {α : Type} (r : α → α → Bool) : Prop := ∀ a b, r a b = true ∨ r b a = true
def IsAntisymm {α : Type} (r : α → α → Bool) : Prop := ∀ a b, r a b = true → r b a = true → a = b

def lexStep {α β : Type} [BEq β] (f : α → β) (le : β → β → Bool) (rest : α → α → Bool) (a b : α) : Bool :=
  if f a != f b then le (f a) (f b) else rest a b

theorem strLe_trans : IsTrans (fun (a : String) (b : String) => decide (a ≤ b)) := by
  intro a b c h1 h2
  simp only [decide_eq_true_eq] at *
  exact str_le_trans h1 h2

theorem strLe_total : IsTotal (fun (a : String) (b : String) => decide (a ≤ b)) := by
  intro a b
  simp only [decide_eq_true_eq]
  exact str_le_total a b

theorem strLe_antisymm : IsAntisymm (fun (a : String) (b : String) => decide (a ≤ b)) := by
  intro a b h1 h2
  simp only [decide_eq_true_eq] at *
  exact str_antisymm h2 h1

theorem optionLe_trans : IsTrans optionLe := by
  intro a b c h1 h2
  cases a <;> cases b <;> cases c <;> simp_all [optionLe]
  exact str_le_trans h1 h2

theorem optionLe_total : IsTotal optionLe := by
  intro a b
  cases a <;> cases b <;> simp [optionLe]
  exact str_le_total _ _

theorem optionLe_antisymm : IsAntisymm optionLe := by
  intro a b h1 h2
  cases a <;> cases b <;> simp_all [optionLe]
  exact str_antisymm h2 h1

theorem lexStep_trans {α β : Type} [BEq β] [LawfulBEq β] (f : α → β) (le : β → β → Bool)
    (rest : α → α → Bool) (ht : IsTrans le) (ha : IsAntisymm le) (hr : IsTrans rest) :
    IsTrans (lexStep f le rest) := by
  intro a b c h1 h2
  unfold lexStep at *
  by_cases hab : f a = f b <;> by_cases hbc : f b = f c <;> simp_all
  · exact hr _ _ _ h1 h2
  · by_cases hac : f a = f c
    · exact absurd (ha _ _ h1 (hac ▸ h2)) hab
    · simp [hac]
      exact ht _ _ _ h1 h2

theorem lexStep_total {α β : Type} [BEq β] [LawfulBEq β] (f : α → β) (le : β → β → Bool)
    (rest : α → α → Bool) (ht : IsTotal le) (hr : IsTotal rest) :
    IsTotal (lexStep f le rest) := by
  intro a b
  unfold lexStep
  by_cases hab : f a = f b
  · simp [hab]
    exact hr a b
  · have hba : ¬ f b = f a := fun h => hab h.symm
    simp [hab, hba]
    exact ht _ _

theorem lexStep_antisymm {α β : Type} [BEq β] [LawfulBEq β] (f : α → β) (le : β → β → Bool)
    (rest : α → α → Bool) (ha : IsAntisymm le) (a b : α)
    (h1 : lexStep f le rest a b = true) (h2 : lexStep f le rest b a = true) :
    f a = f b ∧ rest a b = true ∧ rest b a = true := by
  unfold lexStep at *
  by_cases hab : f a = f b
  · simp_all
  · have hba : ¬ f b = f a := fun h => hab h.symm
    simp [hab, hba] at h1 h2
    exact absurd (ha _ _ h1 h2) hab

def sLe (a b : String) : Bool := decide (a ≤ b)

theorem lockLinkLe_lex (a b : LockLink) :
    lockLinkLe a b =
      lexStep LockLink.role sLe (lexStep LockLink.req sLe (lexStep LockLink.clause optionLe
        (lexStep LockLink.file sLe (lexStep LockLink.anchor sLe (lexStep LockLink.body_hash sLe
          (lexStep LockLink.link_hash sLe (fun x y => optionLe x.qualifier y.qualifier))))))) a b := by
  rfl

theorem lockLinkLe_trans : IsTrans lockLinkLe := by
  intro a b c h1 h2
  rw [lockLinkLe_lex] at *
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_ _ _ _ h1 h2
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_
  refine lexStep_trans _ _ _ optionLe_trans optionLe_antisymm ?_
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_
  refine lexStep_trans _ _ _ strLe_trans strLe_antisymm ?_
  intro x y z hx hy
  exact optionLe_trans _ _ _ hx hy

theorem lockLinkLe_total : IsTotal lockLinkLe := by
  intro a b
  rw [lockLinkLe_lex, lockLinkLe_lex]
  refine lexStep_total _ _ _ strLe_total ?_ a b
  refine lexStep_total _ _ _ strLe_total ?_
  refine lexStep_total _ _ _ optionLe_total ?_
  refine lexStep_total _ _ _ strLe_total ?_
  refine lexStep_total _ _ _ strLe_total ?_
  refine lexStep_total _ _ _ strLe_total ?_
  refine lexStep_total _ _ _ strLe_total ?_
  intro x y
  exact optionLe_total _ _

theorem lockLinkLe_antisymm : IsAntisymm lockLinkLe := by
  intro a b h1 h2
  rw [lockLinkLe_lex] at h1 h2
  obtain ⟨e1, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  obtain ⟨e2, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  obtain ⟨e3, h1, h2⟩ := lexStep_antisymm _ _ _ optionLe_antisymm a b h1 h2
  obtain ⟨e4, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  obtain ⟨e5, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  obtain ⟨e6, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  obtain ⟨e7, h1, h2⟩ := lexStep_antisymm _ _ _ strLe_antisymm a b h1 h2
  have e8 := optionLe_antisymm _ _ h1 h2
  cases a; cases b
  simp_all

theorem sorted_perm_eq {α : Type} (r : α → α → Bool) (ha : IsAntisymm r) :
    ∀ (l1 l2 : List α), l1.Pairwise (fun a b => r a b = true) → l2.Pairwise (fun a b => r a b = true) →
      l1.Perm l2 → l1 = l2
  | [], l2, _, _, p => List.Perm.nil_eq p
  | a :: t1, [], _, _, p => absurd p.length_eq (by simp)
  | a :: t1, b :: t2, s1, s2, p => by
    have hab : a = b := by
      have ha2 : a ∈ b :: t2 := p.mem_iff.mp (List.mem_cons_self a t1)
      have hb1 : b ∈ a :: t1 := p.mem_iff.mpr (List.mem_cons_self b t2)
      rcases List.mem_cons.mp ha2 with e | ha2
      · exact e
      rcases List.mem_cons.mp hb1 with e | hb1
      · exact e.symm
      exact ha _ _ ((List.pairwise_cons.mp s1).1 b hb1) ((List.pairwise_cons.mp s2).1 a ha2)
    subst hab
    rw [sorted_perm_eq r ha t1 t2 (List.pairwise_cons.mp s1).2 (List.pairwise_cons.mp s2).2 p.cons_inv]

/-- @proves ARCH-DETERMINISM.stable_ordering -/
theorem links_have_a_total_order (links : List LockLink) :
    (∀ a b, lockLinkLe a b = true ∨ lockLinkLe b a = true) ∧
    (∀ a b c, lockLinkLe a b = true → lockLinkLe b c = true → lockLinkLe a c = true) ∧
    (∀ a b, lockLinkLe a b = true → lockLinkLe b a = true → a = b) ∧
    (orderedLinks links).Pairwise (fun a b => lockLinkLe a b = true) ∧
    (orderedLinks links).Perm links :=
  ⟨lockLinkLe_total, lockLinkLe_trans, lockLinkLe_antisymm,
   List.mergeSort_sorted lockLinkLe_trans
     (fun a b h => (lockLinkLe_total a b).resolve_left (by simpa using h)) links,
   List.mergeSort_perm (le := lockLinkLe) links⟩

/-- @proves REQ-LOCK.deterministic_bytes -/
theorem same_links_same_order (l1 l2 : List LockLink) (h : l1.Perm l2) :
    orderedLinks l1 = orderedLinks l2 :=
  sorted_perm_eq lockLinkLe lockLinkLe_antisymm _ _ (links_have_a_total_order l1).2.2.2.1
    (links_have_a_total_order l2).2.2.2.1
    (((List.mergeSort_perm (le := lockLinkLe) l1).trans h).trans (List.mergeSort_perm (le := lockLinkLe) l2).symm)

/-- @proves REQ-LOCK.version_stamped -/
theorem only_the_current_version_is_read (version : Option Nat) :
    (versionVerdict version = .readable ↔ version = some lockVersion) ∧
    (version = none → versionVerdict version = .unstamped) ∧
    (∀ v, version = some v → v ≠ lockVersion → versionVerdict version = .unknown v lockVersion) := by
  refine ⟨?_, ?_, ?_⟩
  · cases version with
    | none => simp [versionVerdict]
    | some v =>
      by_cases h : v = lockVersion
      · simp [versionVerdict, h]
      · simp [versionVerdict, h]
  · intro h
    subst h
    rfl
  · intro v h hv
    subst h
    simp [versionVerdict, hv]

/-- @proves REQ-LOCK.evidence_preserved -/
theorem evidence_is_carried_unchanged (evidence : List Evidence) :
    carriedEvidence evidence = evidence ∧
    ∀ r, r ∈ carriedEvidence evidence ↔ r ∈ evidence :=
  ⟨rfl, fun _ => Iff.rfl⟩

end TraceLean.Lockfile
