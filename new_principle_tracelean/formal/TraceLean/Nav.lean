import Lean

namespace TraceLean.Nav

open Lean (ToJson FromJson)

def introducers : List String :=
  ["fn", "struct", "enum", "trait", "type", "mod", "const", "static", "union", "macro_rules!",
   "def", "theorem", "lemma", "structure", "inductive", "class", "instance", "abbrev", "axiom",
   "function", "interface", "let", "var"]

def modifier (word : String) : Bool :=
  ["pub", "pub(crate)", "pub(super)", "crate", "async", "unsafe", "extern", "export",
   "default", "private", "protected", "noncomputable", "partial", "@[simp]"].contains word
    || word.startsWith "pub(" || word.startsWith "@["

def brace : Char := Char.ofNat 123

def pathMark : Char := Char.ofNat 8759

def separator (c : Char) : Bool :=
  c.isWhitespace || "()<>:=;,".toList.contains c || c == brace

def declaredAfter : List String → Option String
  | [] => none
  | word :: rest =>
    if introducers.contains word then
      match rest with
      | next :: _ => if next.toList.contains pathMark then none else some next
      | [] => none
    else if modifier word then declaredAfter rest else none

/-- @models REQ-NAV.declaration_line -/
def declaredOn (line : String) : Option String :=
  declaredAfter (((line.replace "::" (String.singleton pathMark)).split separator).filter (fun w => w ≠ ""))

structure Declared where
  path : String
  line : Nat
  text : String
  deriving DecidableEq, Repr, ToJson, FromJson

def placed (path : String) (p : Nat × String) : Declared :=
  { path := path, line := p.1 + 1, text := p.2.trim }

def linesKept (files : List (String × String)) (keep : String → Bool) : List Declared :=
  files.bind (fun file => (((file.2.splitOn "\n").enum.filter (fun p => keep p.2)).map (placed file.1)))

def byPlace (a b : Declared) : Bool :=
  decide (a.path < b.path) || (a.path == b.path && decide (a.line ≤ b.line))

def lastPart (name : String) : String :=
  ((name.splitOn ".").getLast?).getD name

def declares (name : String) (l : String) : Bool :=
  declaredOn l == some name || (lastPart name != name && declaredOn l == some (lastPart name))

/-- @models REQ-NAV.definitions_found -/
def declarations (files : List (String × String)) (name : String) : List Declared :=
  (linesKept files (declares name)).mergeSort byPlace

def wordChar (c : Char) : Bool := c.isAlphanum || c == '_'

def startsAt (line needle : List Char) (i : Nat) : Bool :=
  (line.drop i).take needle.length == needle

def wordAfter (line : List Char) (i : Nat) : Bool :=
  ((line.drop i).head?.map wordChar).getD false

def wholeIn (line needle : List Char) : Bool :=
  (List.range line.length).any (fun i =>
    startsAt line needle i && !(i > 0 && wordChar (line.getD (i - 1) ' '))
      && !(wordAfter line (i + needle.length)))

def heldIn (line needle : String) : Bool :=
  let l := line.toList.map Char.toLower
  let n := needle.toList.map Char.toLower
  (List.range (l.length + 1)).any (startsAt l n)

def holds (needle : String) (whole : Bool) (l : String) : Bool :=
  if whole then wholeIn l.toList needle.toList else heldIn l needle

def byPath (a b : String × String) : Bool := decide (a.1 ≤ b.1)

/-- @models REQ-NAV.uses_found -/
def occurrences (files : List (String × String)) (needle : String) (whole : Bool) (limit : Nat) : List Declared :=
  if needle.isEmpty then [] else (linesKept (files.mergeSort byPath) (holds needle whole)).take limit

def findFrom (lower : List Char) (letter : Char) (start : Nat) : Option Nat :=
  ((List.range lower.length).drop start).find? (fun i => lower.get? i == some letter)

def wordBreak (c : Char) : Bool := "/_-.".toList.contains c

def bonus (lower : List Char) (nameStarts found : Nat) (previous : Option Nat) : Int :=
  (if found ≥ nameStarts then 4 else 0)
    + (if previous.map (fun p => p + 1) == some found then 6 else 0)
    + (if found == nameStarts || (found > 0 && wordBreak (lower.getD (found - 1) ' ')) then 3 else 0)

def lettersFrom (lower : List Char) (nameStarts : Nat) :
    List Char → Nat → Option Nat → Int → Option Int
  | [], _, _, total => some total
  | letter :: rest, start, previous, total =>
    match findFrom lower letter start with
    | none => none
    | some found =>
      lettersFrom lower nameStarts rest (found + 1) (some found)
        (total + 1 + bonus lower nameStarts found previous)

def nameStart (lower : List Char) : Nat :=
  match (lower.enum.filter (fun p => p.2 == '/')).getLast? with
  | some p => p.1 + 1
  | none => 0

def best : List Int → Option Int
  | [] => none
  | t :: ts => some (ts.foldl max t)

def score (path query : String) : Option Int :=
  let lower := path.toList.map Char.toLower
  let wanted := (query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)
  let n := nameStart lower
  if wanted.isEmpty then some (-(lower.length : Int)) else
    (best ([0, n].filterMap (fun start => lettersFrom lower n wanted start none 0))).map
      (fun b => b * 1000000 - ((lower.length - n : Nat) : Int) * 1000 - (lower.length : Int))

def better (a b : Int × String) : Bool :=
  decide (a.1 > b.1) || (a.1 == b.1 && decide (a.2 ≤ b.2))

/-- @models REQ-NAV.quick_open -/
def quickOpen (files : List String) (query : String) (limit : Nat) : List String :=
  (((files.filterMap (fun p => (score p query).map (fun s => (s, p)))).mergeSort better).take limit).map
    (fun p => p.2)

/-- @models REQ-NAV.session_recalled -/
def reopened (files : List String) (shown : Option String) (existing : List String) : List String :=
  (files ++ shown.toList).filter existing.contains

def sample : List (String × String) :=
  [("web/app.ts", "export function open() = 1\nconst x = open();\n"),
   ("src/a.rs", "use x;\npub fn open(a: u8) = 1\nfn other() = open(1)\n"),
   ("formal/A.lean", "def canon (w : W) : W := w\n")]

/-- @proves REQ-NAV.session_recalled -/
theorem everything_reopened_exists (files existing : List String) (shown : Option String) (p : String)
    (h : List.Mem p (reopened files shown existing)) : List.Mem p existing := by
  have kept := (List.mem_filter.mp h).2
  simpa using kept

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

def wordsOf (line : String) : List String :=
  ((line.replace "::" (String.singleton pathMark)).split separator).filter (fun w => w ≠ "")

theorem declaredOn_words (line : String) : declaredOn line = declaredAfter (wordsOf line) := rfl

theorem declaredAfter_sound : ∀ (ws : List String) (w : String), declaredAfter ws = some w →
    pathMark ∉ w.toList ∧ ∃ mods intro rest, ws = mods ++ intro :: w :: rest ∧
      (∀ m ∈ mods, modifier m = true) ∧ introducers.contains intro = true
  | [], _, h => by simp [declaredAfter] at h
  | word :: ws, w, h => by
    unfold declaredAfter at h
    split at h
    · rename_i hi
      split at h
      · rename_i next more
        split at h
        · cases h
        · rename_i hp
          cases h
          refine ⟨by simpa using hp, [], word, more, rfl, fun _ hm => absurd hm (List.not_mem_nil _), hi⟩
      · cases h
    · split at h
      · rename_i hm
        obtain ⟨hp, mods, intro, rest, e, hmods, hi⟩ := declaredAfter_sound ws w h
        refine ⟨hp, word :: mods, intro, rest, by rw [e]; rfl, ?_, hi⟩
        intro m hm'
        rcases List.mem_cons.mp hm' with e | e
        · exact e ▸ hm
        · exact hmods m e
      · cases h

theorem declaredAfter_complete : ∀ (mods : List String) (intro next : String) (rest : List String),
    (∀ m ∈ mods, modifier m = true ∧ introducers.contains m = false) → introducers.contains intro = true →
    declaredAfter (mods ++ intro :: next :: rest) =
      if next.toList.contains pathMark then none else some next
  | [], intro, next, rest, _, hi => by
    simp only [List.nil_append, declaredAfter, hi, ite_true]
  | m :: mods, intro, next, rest, hm, hi => by
    have ⟨h1, h2⟩ := hm m (List.mem_cons_self _ _)
    simp only [List.cons_append, declaredAfter, h2, h1, Bool.false_eq_true, ite_false, ite_true]
    exact declaredAfter_complete mods intro next rest (fun x hx => hm x (List.mem_cons_of_mem _ hx)) hi

/-- @proves REQ-NAV.declaration_line -/
theorem a_line_declares_the_word_after_its_declaring_word (line : String) :
    (∀ mods intro next rest, wordsOf line = mods ++ intro :: next :: rest →
      (∀ m ∈ mods, modifier m = true ∧ introducers.contains m = false) → introducers.contains intro = true →
      (pathMark ∉ next.toList → declaredOn line = some next) ∧
      (pathMark ∈ next.toList → declaredOn line = none)) ∧
    (∀ w, declaredOn line = some w → pathMark ∉ w.toList ∧
      ∃ mods intro rest, wordsOf line = mods ++ intro :: w :: rest ∧
        (∀ m ∈ mods, modifier m = true) ∧ introducers.contains intro = true) := by
  constructor
  · intro mods intro next rest hw hm hi
    rw [declaredOn_words, hw, declaredAfter_complete mods intro next rest hm hi]
    constructor
    · intro hp
      have : next.toList.contains pathMark = false := by simpa using hp
      simp only [this, Bool.false_eq_true, ite_false]
    · intro hp
      have : next.toList.contains pathMark = true := by simpa using hp
      simp only [this, ite_true]
  · intro w h
    exact declaredAfter_sound _ w h

theorem byPlace_total (a b : Declared) : byPlace a b = true ∨ byPlace b a = true := by
  unfold byPlace
  by_cases h1 : a.path < b.path
  · simp [h1]
  · by_cases h2 : b.path < a.path
    · simp [h2]
    · have e := str_antisymm h1 h2
      simp only [e, decide_eq_true_eq, beq_self_eq_true, Bool.true_and, Bool.or_eq_true]
      omega

theorem byPlace_trans (a b c : Declared) (h1 : byPlace a b = true) (h2 : byPlace b c = true) :
    byPlace a c = true := by
  unfold byPlace at *
  simp only [Bool.or_eq_true, decide_eq_true_eq, Bool.and_eq_true, beq_iff_eq] at *
  rcases h1 with h1 | ⟨e1, l1⟩ <;> rcases h2 with h2 | ⟨e2, l2⟩
  · exact Or.inl (str_lt_trans h1 h2)
  · exact Or.inl (e2 ▸ h1)
  · exact Or.inl (e1 ▸ h2)
  · exact Or.inr ⟨e1.trans e2, Nat.le_trans l1 l2⟩

theorem mem_linesKept (files : List (String × String)) (keep : String → Bool) (d : Declared) :
    d ∈ linesKept files keep ↔ ∃ file ∈ files, ∃ i t, (file.2.splitOn "\n")[i]? = some t ∧
      keep t = true ∧ d = placed file.1 (i, t) := by
  unfold linesKept
  simp only [List.mem_bind, List.mem_map, List.mem_filter]
  constructor
  · intro ⟨file, hf, ⟨i, t⟩, ⟨hit, hk⟩, e⟩
    exact ⟨file, hf, i, t, List.mem_enum_iff_getElem?.mp hit, hk, e.symm⟩
  · intro ⟨file, hf, i, t, hit, hk, e⟩
    exact ⟨file, hf, (i, t), ⟨List.mem_enum_iff_getElem?.mpr hit, hk⟩, e.symm⟩

/-- @proves REQ-NAV.definitions_found -/
theorem every_declaring_line_offered_in_place_order (files : List (String × String)) (name : String) :
    (∀ d, d ∈ declarations files name ↔ ∃ file ∈ files, ∃ i t, (file.2.splitOn "\n")[i]? = some t ∧
      (declaredOn t = some name ∨ (lastPart name ≠ name ∧ declaredOn t = some (lastPart name))) ∧
      d = placed file.1 (i, t)) ∧
    (declarations files name).Pairwise (fun a b => byPlace a b = true) := by
  constructor
  · intro d
    unfold declarations
    rw [List.mem_mergeSort, mem_linesKept]
    simp only [declares, Bool.or_eq_true, beq_iff_eq, Bool.and_eq_true, bne_iff_ne, ne_eq]
  · exact List.mergeSort_sorted byPlace_trans (fun a b h => (byPlace_total a b).resolve_left (by simpa using h)) _

theorem str_le_refl (a : String) : a ≤ a := String.lt_irrefl a

theorem byPath_total (a b : String × String) : byPath a b = true ∨ byPath b a = true := by
  unfold byPath
  simp only [decide_eq_true_eq]
  exact str_le_total _ _

theorem byPath_trans (a b c : String × String) (h1 : byPath a b = true) (h2 : byPath b c = true) :
    byPath a c = true := by
  unfold byPath at *
  simp only [decide_eq_true_eq] at *
  exact str_le_trans h1 h2

theorem linesKept_by_path : ∀ (files : List (String × String)) (keep : String → Bool),
    files.Pairwise (fun a b => a.1 ≤ b.1) →
    (linesKept files keep).Pairwise (fun a b => a.path ≤ b.path)
  | [], _, _ => List.Pairwise.nil
  | f :: fs, keep, h => by
    have hrest := linesKept_by_path fs keep (List.pairwise_cons.mp h).2
    have hsplit : linesKept (f :: fs) keep =
        (((f.2.splitOn "\n").enum.filter (fun p => keep p.2)).map (placed f.1)) ++ linesKept fs keep := by
      simp [linesKept]
    have hpath : ∀ d ∈ (((f.2.splitOn "\n").enum.filter (fun p => keep p.2)).map (placed f.1)), d.path = f.1 := by
      intro d hd
      obtain ⟨_, _, rfl⟩ := List.mem_map.mp hd
      rfl
    rw [hsplit, List.pairwise_append]
    refine ⟨?_, hrest, ?_⟩
    · rw [List.pairwise_iff_forall_sublist]
      intro a b hab
      have ha := hpath a (hab.subset (List.mem_cons_self _ _))
      have hb := hpath b (hab.subset (List.mem_cons_of_mem _ (List.mem_singleton_self _)))
      rw [ha, hb]
      exact str_le_refl _
    · intro a ha b hb
      rw [hpath a ha]
      obtain ⟨g, hg, _, _, _, _, rfl⟩ := (mem_linesKept fs keep b).mp hb
      exact (List.pairwise_cons.mp h).1 g hg

/-- @proves REQ-NAV.uses_found -/
theorem uses_are_every_holding_line_by_path (files : List (String × String)) (needle : String)
    (whole : Bool) (limit : Nat) :
    (needle.isEmpty = true → occurrences files needle whole limit = []) ∧
    (occurrences files needle whole limit).length ≤ limit ∧
    (∀ d ∈ occurrences files needle whole limit, ∃ file ∈ files, ∃ i t,
      (file.2.splitOn "\n")[i]? = some t ∧ holds needle whole t = true ∧ d = placed file.1 (i, t)) ∧
    (needle.isEmpty = false → (linesKept files (holds needle whole)).length ≤ limit →
      ∀ file ∈ files, ∀ i t, (file.2.splitOn "\n")[i]? = some t → holds needle whole t = true →
        placed file.1 (i, t) ∈ occurrences files needle whole limit) ∧
    (occurrences files needle whole limit).Pairwise (fun a b => a.path ≤ b.path) := by
  have sorted : (files.mergeSort byPath).Pairwise (fun a b => a.1 ≤ b.1) := by
    have := List.mergeSort_sorted byPath_trans
      (fun a b h => (byPath_total a b).resolve_left (by simpa using h)) files
    exact this.imp (fun h => by simpa [byPath] using h)
  have perm := List.mergeSort_perm (le := byPath) files
  refine ⟨?_, ?_, ?_, ?_, ?_⟩
  · intro h
    simp [occurrences, h]
  · unfold occurrences
    split
    · simp
    · exact List.length_take_le _ _
  · intro d hd
    unfold occurrences at hd
    split at hd
    · simp at hd
    · obtain ⟨file, hf, rest⟩ := (mem_linesKept _ _ d).mp (List.mem_of_mem_take hd)
      exact ⟨file, perm.mem_iff.mp hf, rest⟩
  · intro hn hlen file hf i t hi ht
    have hlen' : (linesKept (files.mergeSort byPath) (holds needle whole)).length ≤ limit := by
      have : (linesKept (files.mergeSort byPath) (holds needle whole)).length =
          (linesKept files (holds needle whole)).length := by
        unfold linesKept
        exact (perm.bind_right _).length_eq
      omega
    unfold occurrences
    simp only [hn, Bool.false_eq_true, ite_false]
    rw [List.take_of_length_le hlen']
    exact (mem_linesKept _ _ _).mpr ⟨file, perm.mem_iff.mpr hf, i, t, hi, ht, rfl⟩
  · unfold occurrences
    split
    · exact List.Pairwise.nil
    · exact (linesKept_by_path _ _ sorted).sublist (List.take_sublist _ _)

theorem better_total (a b : Int × String) : better a b = true ∨ better b a = true := by
  unfold better
  simp only [Bool.or_eq_true, decide_eq_true_eq, Bool.and_eq_true, beq_iff_eq]
  rcases Int.lt_trichotomy a.1 b.1 with h | h | h
  · exact Or.inr (Or.inl h)
  · rcases str_le_total a.2 b.2 with h2 | h2
    · exact Or.inl (Or.inr ⟨h, h2⟩)
    · exact Or.inr (Or.inr ⟨h.symm, h2⟩)
  · exact Or.inl (Or.inl h)

theorem better_trans (a b c : Int × String) (h1 : better a b = true) (h2 : better b c = true) :
    better a c = true := by
  unfold better at *
  simp only [Bool.or_eq_true, decide_eq_true_eq, Bool.and_eq_true, beq_iff_eq] at *
  rcases h1 with h1 | ⟨e1, l1⟩ <;> rcases h2 with h2 | ⟨e2, l2⟩
  · exact Or.inl (by omega)
  · exact Or.inl (by omega)
  · exact Or.inl (by omega)
  · exact Or.inr ⟨by omega, str_le_trans l1 l2⟩

theorem mem_range_drop : ∀ (a n s i : Nat), i ∈ (List.range' a n).drop s → a + s ≤ i ∧ i < a + n
  | a, n, 0, i, h => by
    simp only [List.drop_zero, List.mem_range'_1] at h
    omega
  | a, 0, s + 1, i, h => by simp [List.range'] at h
  | a, n + 1, s + 1, i, h => by
    have := mem_range_drop (a + 1) n s i h
    omega

theorem findFrom_spec (lower : List Char) (letter : Char) (start found : Nat)
    (h : findFrom lower letter start = some found) :
    start ≤ found ∧ ∃ hf : found < lower.length, lower[found] = letter := by
  unfold findFrom at h
  have hm := List.mem_of_find?_eq_some h
  have hp := List.find?_some h
  rw [List.range_eq_range'] at hm
  have := mem_range_drop 0 lower.length start found hm
  refine ⟨by omega, by omega, ?_⟩
  simp only [beq_iff_eq] at hp
  rw [List.get?_eq_getElem?] at hp
  exact (List.getElem?_eq_some.mp hp).2

theorem sublist_drop_of_find (lower : List Char) (start found : Nat) (letter : Char) (rest : List Char)
    (hs : start ≤ found) (hf : found < lower.length) (hl : lower[found] = letter)
    (hr : rest.Sublist (lower.drop (found + 1))) : (letter :: rest).Sublist (lower.drop start) := by
  have e : lower.drop start = (lower.drop start).take (found - start) ++ lower.drop found := by
    have := List.take_append_drop (found - start) (lower.drop start)
    rw [List.drop_drop, Nat.sub_add_cancel hs] at this
    exact this.symm
  rw [e, List.drop_eq_getElem_cons hf, hl]
  exact List.Sublist.trans (List.Sublist.cons₂ letter hr) (List.sublist_append_right _ _)

theorem lettersFrom_sublist (lower : List Char) (nameStarts : Nat) :
    ∀ (wanted : List Char) (start : Nat) (previous : Option Nat) (total s : Int),
      lettersFrom lower nameStarts wanted start previous total = some s → wanted.Sublist (lower.drop start)
  | [], _, _, _, _, _ => List.nil_sublist _
  | letter :: rest, start, previous, total, s, h => by
    unfold lettersFrom at h
    split at h
    · cases h
    · rename_i found hfound
      obtain ⟨hs, hf, hl⟩ := findFrom_spec lower letter start found hfound
      exact sublist_drop_of_find lower start found letter rest hs hf hl
        (lettersFrom_sublist lower nameStarts rest (found + 1) _ _ s h)

theorem best_mem : ∀ (l : List Int) (b : Int), best l = some b → b ∈ l
  | [], _, h => by simp [best] at h
  | t :: ts, b, h => by
    simp only [best, Option.some.injEq] at h
    subst h
    have key : ∀ (xs : List Int) (acc : Int), xs.foldl max acc = acc ∨ xs.foldl max acc ∈ xs := by
      intro xs
      induction xs with
      | nil => intro acc; exact Or.inl rfl
      | cons x xs ih =>
        intro acc
        simp only [List.foldl_cons]
        rcases ih (max acc x) with e | e
        · rw [e]
          by_cases hx : acc ≤ x
          · exact Or.inr (by rw [Int.max_eq_right hx]; exact List.mem_cons_self _ _)
          · exact Or.inl (Int.max_eq_left (by omega))
        · exact Or.inr (List.mem_cons_of_mem _ e)
    rcases key ts t with e | e
    · rw [e]; exact List.mem_cons_self _ _
    · exact List.mem_cons_of_mem _ e

theorem score_letters_in_order (path query : String) (s : Int) (h : score path query = some s) :
    ((query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)).Sublist
      (path.toList.map Char.toLower) := by
  unfold score at h
  simp only [] at h
  split at h
  · rename_i he
    rw [List.isEmpty_iff.mp he]
    exact List.nil_sublist _
  · obtain ⟨b, hb, _⟩ := Option.map_eq_some'.mp h
    have hm := best_mem _ _ hb
    obtain ⟨start, _, hst⟩ := List.mem_filterMap.mp hm
    exact (lettersFrom_sublist _ _ _ _ _ _ _ hst).trans (List.drop_sublist _ _)

def Ranked (query : String) (p q : String) : Prop :=
  ∃ sp sq, score p query = some sp ∧ score q query = some sq ∧ (sp > sq ∨ (sp = sq ∧ p ≤ q))

/-- @proves REQ-NAV.quick_open -/
theorem quick_open_lists_matches_best_first (files : List String) (query : String) (limit : Nat) :
    (quickOpen files query limit).length ≤ limit ∧
    (∀ p ∈ quickOpen files query limit, p ∈ files ∧
      ((query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)).Sublist
        (p.toList.map Char.toLower)) ∧
    (quickOpen files query limit).Pairwise (Ranked query) ∧
    (((query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)) = [] →
      (∀ p, score p query = some (-(p.length : Int))) ∧
      (files.length ≤ limit → ∀ p ∈ files, p ∈ quickOpen files query limit)) := by
  let scored := files.filterMap (fun p => (score p query).map (fun s => (s, p)))
  have hq : quickOpen files query limit = ((scored.mergeSort better).take limit).map (fun p => p.2) := rfl
  have mem_scored : ∀ x, x ∈ scored ↔ x.2 ∈ files ∧ score x.2 query = some x.1 := by
    intro x
    simp only [scored, List.mem_filterMap, Option.map_eq_some']
    constructor
    · intro ⟨p, hp, s, hs, e⟩
      subst e
      exact ⟨hp, hs⟩
    · intro ⟨h1, h2⟩
      exact ⟨x.2, h1, x.1, h2, rfl⟩
  have sorted := List.mergeSort_sorted better_trans
    (fun a b h => (better_total a b).resolve_left (by simpa using h)) scored
  have perm := List.mergeSort_perm (le := better) scored
  refine ⟨?_, ?_, ?_, ?_⟩
  · rw [hq, List.length_map]
    exact List.length_take_le _ _
  · intro p hp
    rw [hq] at hp
    obtain ⟨x, hx, rfl⟩ := List.mem_map.mp hp
    have hx' := (mem_scored x).mp (perm.mem_iff.mp (List.mem_of_mem_take hx))
    exact ⟨hx'.1, score_letters_in_order _ _ _ hx'.2⟩
  · rw [hq, List.pairwise_map]
    apply List.Pairwise.imp_of_mem _ (sorted.sublist (List.take_sublist _ _))
    intro a b ha hb hab
    have ha' := (mem_scored a).mp (perm.mem_iff.mp (List.mem_of_mem_take ha))
    have hb' := (mem_scored b).mp (perm.mem_iff.mp (List.mem_of_mem_take hb))
    refine ⟨a.1, b.1, ha'.2, hb'.2, ?_⟩
    unfold better at hab
    simp only [Bool.or_eq_true, decide_eq_true_eq, Bool.and_eq_true, beq_iff_eq] at hab
    exact hab
  · intro hw
    have hs : ∀ p, score p query = some (-(p.length : Int)) := by
      intro p
      unfold score
      simp only [hw, List.isEmpty_nil, ite_true, List.length_map]
      rfl
    refine ⟨hs, ?_⟩
    intro hlen p hp
    have hlen' : (scored.mergeSort better).length ≤ limit := by
      rw [List.mergeSort_length]
      have : scored.length ≤ files.length := List.length_filterMap_le _ _
      omega
    rw [hq, List.take_of_length_le hlen']
    exact List.mem_map.mpr ⟨(-(p.length : Int), p), perm.mem_iff.mpr ((mem_scored _).mpr ⟨hp, hs p⟩), rfl⟩
end TraceLean.Nav
