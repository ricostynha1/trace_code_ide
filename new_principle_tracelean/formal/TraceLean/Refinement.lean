import Lean

namespace TraceLean.Refinement

open Lean (ToJson FromJson)

inductive Mark where
  | open_ | done
  deriving Repr, DecidableEq, Inhabited

abbrev Graph := List (String × List String)

def canonGraph (edges : Graph) : Graph :=
  (edges.foldl
    (fun acc kv =>
      if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
      else acc ++ [kv]) []).mergeSort (fun a b => decide (a.1 ≤ b.1))

def parentsOf (g : Graph) (node : String) : List String :=
  (g.find? (·.1 == node)).map (·.2) |>.getD []

def hasNode (g : Graph) (node : String) : Bool := g.any (·.1 == node)

def markOf (marks : List (String × Mark)) (node : String) : Option Mark :=
  (marks.find? (·.1 == node)).map (·.2)

def setMark (marks : List (String × Mark)) (node : String) (m : Mark) : List (String × Mark) :=
  if marks.any (·.1 == node) then marks.map (fun p => if p.1 == node then (node, m) else p)
  else marks ++ [(node, m)]

structure Walk where
  path : List String
  next : List Nat
  marks : List (String × Mark)

def step (g : Graph) (w : Walk) : Walk ⊕ List String :=
  match w.path.reverse, w.next.reverse with
  | node :: _, index :: _ =>
    let parents := parentsOf g node
    if index ≥ parents.length then
      .inl { path := w.path.dropLast, next := w.next.dropLast,
             marks := setMark w.marks node .done }
    else
      let w := { w with next := w.next.dropLast ++ [index + 1] }
      let parent := parents.getD index ""
      if !hasNode g parent then .inl w
      else
        match markOf w.marks parent with
        | some .open_ =>
          let start := (w.path.findIdx? (· == parent)).getD 0
          .inr ((w.path.drop start) ++ [parent])
        | some .done => .inl w
        | none =>
          .inl { path := w.path ++ [parent], next := w.next ++ [0],
                 marks := setMark w.marks parent .open_ }
  | _, _ => .inl w

def walk (g : Graph) : Nat → Walk → List (String × Mark) ⊕ List String
  | 0, w => .inl w.marks
  | fuel + 1, w =>
    if w.path.isEmpty then .inl w.marks
    else match step g w with
      | .inr cycle => .inr cycle
      | .inl w' => walk g fuel w'

def roots (g : Graph) : List String := g.map (·.1)

def searchFrom (g : Graph) (fuel : Nat)
    : List String → List (String × Mark) → Option (List String)
  | [], _ => none
  | root :: rest, marks =>
    if markOf marks root == some .done then searchFrom g fuel rest marks
    else
      match walk g fuel { path := [root], next := [0], marks := setMark marks root .open_ } with
      | .inr cycle => some cycle
      | .inl marks' => searchFrom g fuel rest marks'

def cycleIn (g : Graph) : Option (List String) :=
  let budget := g.length * 2 + (g.foldl (fun n e => n + e.2.length) 0) + 1
  searchFrom g budget (roots g) []

/-- @models REQ-REQDOC.refines_resolves -/
def danglingIn (g : Graph) : List (String × String) :=
  let pairs := g.foldl
    (fun acc e => acc ++ (e.2.filter (fun p => !hasNode g p)).map (fun p => (e.1, p))) []
  (pairs.foldl (fun acc p => if acc.contains p then acc else acc ++ [p]) []).mergeSort
    (fun a b => if a.1 != b.1 then decide (a.1 ≤ b.1) else decide (a.2 ≤ b.2))

structure GraphReport where
  cycle : Option (List String)
  dangling : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-REQDOC.refines_dag -/
def graphReport (edges : Graph) : GraphReport :=
  let g := canonGraph edges
  { cycle := cycleIn g, dangling := danglingIn g }


private theorem mem_foldl_append {α β : Type} (f : β → List α) : ∀ (l : List β) (acc : List α) (x : α),
    x ∈ l.foldl (fun acc e => acc ++ f e) acc ↔ x ∈ acc ∨ ∃ e ∈ l, x ∈ f e := by
  intro l
  induction l with
  | nil => intro acc x; simp
  | cons e rest ih =>
    intro acc x
    simp only [List.foldl, ih, List.mem_append, List.mem_cons]
    constructor
    · rintro ((h | h) | ⟨e', he', h⟩)
      · exact Or.inl h
      · exact Or.inr ⟨e, Or.inl rfl, h⟩
      · exact Or.inr ⟨e', Or.inr he', h⟩
    · rintro (h | ⟨e', rfl | he', h⟩)
      · exact Or.inl (Or.inl h)
      · exact Or.inl (Or.inr h)
      · exact Or.inr ⟨e', he', h⟩

private theorem mem_firstSeen {α : Type} [BEq α] [LawfulBEq α] : ∀ (l acc : List α) (x : α),
    x ∈ l.foldl (fun acc p => if acc.contains p then acc else acc ++ [p]) acc ↔ x ∈ acc ∨ x ∈ l := by
  intro l
  induction l with
  | nil => intro acc x; simp
  | cons a rest ih =>
    intro acc x
    simp only [List.foldl]
    rw [ih]
    split
    · rename_i h
      have ha : a ∈ acc := List.mem_of_elem_eq_true h
      simp only [List.mem_cons]
      constructor
      · rintro (h | h)
        · exact Or.inl h
        · exact Or.inr (Or.inr h)
      · rintro (h | rfl | h)
        · exact Or.inl h
        · exact Or.inl ha
        · exact Or.inr h
    · simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · rintro ((h | rfl) | h)
        · exact Or.inl h
        · exact Or.inr (Or.inl rfl)
        · exact Or.inr (Or.inr h)
      · rintro (h | rfl | h)
        · exact Or.inl (Or.inl h)
        · exact Or.inl (Or.inr rfl)
        · exact Or.inr h

/-- @proves REQ-REQDOC.refines_resolves -/
theorem a_missing_parent_is_reported (g : Graph) (child parent : String) :
    (child, parent) ∈ danglingIn g ↔
      ∃ e ∈ g, e.1 = child ∧ parent ∈ e.2 ∧ hasNode g parent = false := by
  unfold danglingIn
  simp only []
  rw [(List.mergeSort_perm _ _).mem_iff, mem_firstSeen, mem_foldl_append]
  simp only [List.not_mem_nil, false_or, List.mem_map, List.mem_filter, Bool.not_eq_true']
  constructor
  · rintro ⟨e, he, p, ⟨hp, hn⟩, h⟩
    cases h
    exact ⟨e, he, rfl, hp, hn⟩
  · rintro ⟨e, he, rfl, hp, hn⟩
    exact ⟨e, he, parent, ⟨hp, hn⟩, rfl⟩

def Linked (g : Graph) : List String → Prop
  | a :: b :: rest => b ∈ parentsOf g a ∧ Linked g (b :: rest)
  | _ => True

def IsCycle (g : Graph) (c : List String) : Prop :=
  Linked g c ∧ 2 ≤ c.length ∧ c.head? = c.getLast?

theorem markOf_cons (p : String × Mark) (rest : List (String × Mark)) (k : String) :
    markOf (p :: rest) k = if p.1 = k then some p.2 else markOf rest k := by
  unfold markOf
  by_cases h : p.1 = k
  · rw [if_pos h]
    have : (p.1 == k) = true := by rw [h]; exact beq_self_eq_true k
    simp [List.find?, this]
  · rw [if_neg h]
    have : (p.1 == k) = false := by
      cases e : (p.1 == k)
      · rfl
      · exact absurd ((beq_iff_eq _ _).mp e) h
    simp [List.find?, this]

private theorem markOf_map (marks : List (String × Mark)) (n k : String) (m : Mark) :
    markOf (marks.map (fun p => if p.1 == n then (n, m) else p)) k =
      if k = n then (if marks.any (·.1 == n) then some m else none) else markOf marks k := by
  induction marks with
  | nil => by_cases hk : k = n <;> simp [hk, markOf]
  | cons p rest ih =>
    rw [List.map_cons, markOf_cons, markOf_cons, ih, List.any_cons]
    by_cases hp : p.1 = n
    · have e : (p.1 == n) = true := by rw [hp]; exact beq_self_eq_true n
      rw [if_pos e]
      by_cases hk : k = n
      · subst hk
        simp [e]
      · have hk' : ¬ n = k := fun h => hk h.symm
        rw [if_neg hk', if_neg hk, if_neg hk, hp, if_neg hk']
    · have e : (p.1 == n) = false := by
        cases e : (p.1 == n)
        · rfl
        · exact absurd ((beq_iff_eq _ _).mp e) hp
      have hf : (if (p.1 == n) = true then (n, m) else p) = p :=
        if_neg (by rw [e]; exact Bool.false_ne_true)
      rw [hf]
      by_cases hk : k = n
      · rw [if_pos hk, if_pos hk, hk, if_neg hp, e, Bool.false_or]
      · rw [if_neg hk, if_neg hk]

private theorem markOf_append (marks : List (String × Mark)) (n k : String) (m : Mark)
    (h : marks.any (·.1 == n) = false) :
    markOf (marks ++ [(n, m)]) k = if k = n then some m else markOf marks k := by
  induction marks with
  | nil =>
    rw [List.nil_append, markOf_cons]
    by_cases hk : k = n
    · subst hk; simp
    · rw [if_neg (fun e => hk e.symm), if_neg hk]
  | cons p rest ih =>
    rw [List.any_cons, Bool.or_eq_false_iff] at h
    rw [List.cons_append, markOf_cons, markOf_cons, ih h.2]
    have hpn : p.1 ≠ n := fun e => by rw [e, beq_self_eq_true] at h; exact Bool.noConfusion h.1
    by_cases hk : k = n
    · subst hk
      rw [if_neg hpn, if_pos rfl, if_pos rfl]
    · rw [if_neg hk, if_neg hk]

theorem markOf_setMark (marks : List (String × Mark)) (n k : String) (m : Mark) :
    markOf (setMark marks n m) k = if k = n then some m else markOf marks k := by
  unfold setMark
  split
  · rename_i h
    rw [markOf_map]
    by_cases hk : k = n
    · rw [if_pos hk, if_pos hk, if_pos h]
    · rw [if_neg hk, if_neg hk]
  · rename_i h
    exact markOf_append marks n k m (by simpa only [Bool.not_eq_true] using h)

theorem hasNode_of_parents {g : Graph} {n p : String} (h : p ∈ parentsOf g n) : hasNode g n = true := by
  unfold parentsOf at h
  unfold hasNode
  cases hf : g.find? (·.1 == n) with
  | none => rw [hf] at h; cases h
  | some e =>
    exact List.any_eq_true.mpr ⟨e, List.mem_of_find?_eq_some hf, by have := List.find?_some hf; simpa using this⟩

theorem parents_entry {g : Graph} {n : String} (h : hasNode g n = true) :
    ∃ e ∈ g, e.1 = n ∧ e.2 = parentsOf g n := by
  unfold parentsOf
  cases hf : g.find? (·.1 == n) with
  | none =>
    rw [List.find?_eq_none] at hf
    obtain ⟨e, he, hn⟩ := List.any_eq_true.mp h
    exact absurd hn (hf e he)
  | some e =>
    refine ⟨e, List.mem_of_find?_eq_some hf, ?_, rfl⟩
    have := List.find?_some hf
    simpa using this

theorem linked_exist {g : Graph} : ∀ {l : List String}, Linked g l →
    ∀ n ∈ l, hasNode g n = true ∨ l.getLast? = some n := by
  intro l
  induction l with
  | nil => intro _ n h; cases h
  | cons a rest ih =>
    intro hl n hn
    cases rest with
    | nil =>
      rw [List.mem_singleton] at hn
      subst hn
      exact Or.inr rfl
    | cons b r =>
      rcases List.mem_cons.mp hn with rfl | hn
      · exact Or.inl (hasNode_of_parents hl.1)
      · rcases ih hl.2 n hn with h | h
        · exact Or.inl h
        · right
          rw [← h]
          simp [List.getLast?_cons_cons]

theorem cycle_exist {g : Graph} {c : List String} (h : IsCycle g c) : ∀ n ∈ c, hasNode g n = true := by
  obtain ⟨hl, h2, he⟩ := h
  intro n hn
  rcases linked_exist hl n hn with h | h
  · exact h
  · rw [← he] at h
    match c, hl, h2 with
    | a :: b :: rest, hl, _ =>
      simp only [List.head?_cons, Option.some.injEq] at h
      subst h
      exact hasNode_of_parents hl.1

theorem linked_pred {g : Graph} : ∀ {l : List String}, Linked g l →
    ∀ n ∈ l, l.head? = some n ∨ ∃ q ∈ l, n ∈ parentsOf g q := by
  intro l
  induction l with
  | nil => intro _ n h; cases h
  | cons a rest ih =>
    intro hl n hn
    rcases List.mem_cons.mp hn with rfl | hn
    · exact Or.inl rfl
    · cases rest with
      | nil => cases hn
      | cons b r =>
        right
        rcases ih hl.2 n hn with h | ⟨q, hq, h⟩
        · simp only [List.head?_cons, Option.some.injEq] at h
          subst h
          exact ⟨a, List.mem_cons_self _ _, hl.1⟩
        · exact ⟨q, List.mem_cons_of_mem _ hq, h⟩

theorem linked_last_pred {g : Graph} : ∀ {l : List String}, Linked g l → 2 ≤ l.length →
    ∀ x, l.getLast? = some x → ∃ q ∈ l, x ∈ parentsOf g q := by
  intro l
  induction l with
  | nil => intro _ h; cases h
  | cons a rest ih =>
    intro hl hlen x hx
    cases rest with
    | nil => simp at hlen
    | cons b r =>
      cases r with
      | nil =>
        simp only [List.getLast?_cons_cons, List.getLast?_singleton, Option.some.injEq] at hx
        subst hx
        exact ⟨a, List.mem_cons_self _ _, hl.1⟩
      | cons c r' =>
        rw [List.getLast?_cons_cons] at hx
        obtain ⟨q, hq, h⟩ := ih hl.2 (by simp) x hx
        exact ⟨q, List.mem_cons_of_mem _ hq, h⟩

theorem cycle_pred {g : Graph} {c : List String} (h : IsCycle g c) :
    ∀ n ∈ c, ∃ q ∈ c, n ∈ parentsOf g q := by
  obtain ⟨hl, h2, he⟩ := h
  intro n hn
  rcases linked_pred hl n hn with h | h
  · exact linked_last_pred hl h2 n (he ▸ h)
  · exact h

theorem linked_prefix {g : Graph} : ∀ {l : List String} {x : String}, Linked g (l ++ [x]) → Linked g l := by
  intro l
  induction l with
  | nil => intro _ _; trivial
  | cons a rest ih =>
    intro x hl
    cases rest with
    | nil => trivial
    | cons b r => exact ⟨hl.1, ih hl.2⟩

theorem linked_snoc {g : Graph} : ∀ {l : List String} {x y : String}, Linked g (l ++ [x]) →
    y ∈ parentsOf g x → Linked g (l ++ [x] ++ [y]) := by
  intro l
  induction l with
  | nil => intro x y _ h; exact ⟨h, trivial⟩
  | cons a rest ih =>
    intro x y hl h
    cases rest with
    | nil => exact ⟨hl.1, h, trivial⟩
    | cons b r => exact ⟨hl.1, ih hl.2 h⟩

theorem linked_drop {g : Graph} : ∀ {l : List String} (k : Nat), Linked g l → Linked g (l.drop k) := by
  intro l
  induction l with
  | nil => intro k _; simp; trivial
  | cons a rest ih =>
    intro k hl
    cases k with
    | zero => exact hl
    | succ k =>
      rw [List.drop_succ_cons]
      apply ih
      cases rest with
      | nil => trivial
      | cons b r => exact hl.2

private theorem getD_mem : ∀ (l : List String) (t : Nat), t < l.length → l.getD t "" ∈ l := by
  intro l
  induction l with
  | nil => intro t h; cases h
  | cons a rest ih =>
    intro t h
    cases t with
    | zero => exact List.mem_cons_self _ _
    | succ t =>
      show rest.getD t "" ∈ a :: rest
      exact List.mem_cons_of_mem _ (ih t (by simp at h; omega))

private theorem mem_getD : ∀ (l : List String) (p : String), p ∈ l → ∃ t < l.length, l.getD t "" = p := by
  intro l
  induction l with
  | nil => intro p h; cases h
  | cons a rest ih =>
    intro p h
    rcases List.mem_cons.mp h with rfl | h
    · exact ⟨0, by simp, rfl⟩
    · obtain ⟨t, ht, e⟩ := ih p h
      exact ⟨t + 1, by simp; omega, e⟩

private theorem findIdx_mem : ∀ (l : List String) (n : String) (s : Nat), n ∈ l →
    ∃ k, l.findIdx? (· == n) s = some (s + k) ∧ ∃ rest, l.drop k = n :: rest := by
  intro l
  induction l with
  | nil => intro _ _ h; cases h
  | cons a rest ih =>
    intro n s h
    simp only [List.findIdx?]
    by_cases ha : a = n
    · subst ha
      exact ⟨0, by simp, rest, rfl⟩
    · have hb : (a == n) = false := by simpa using ha
      rw [if_neg (by rw [hb]; exact Bool.false_ne_true)]
      have h' : n ∈ rest := by
        rcases List.mem_cons.mp h with h | h
        · exact absurd h.symm ha
        · exact h
      obtain ⟨k, hk, r, hr⟩ := ih n (s + 1) h'
      exact ⟨k + 1, by rw [hk]; congr 1; omega, r, by rw [List.drop_succ_cons]; exact hr⟩

def Settled (g : Graph) (marks : List (String × Mark)) (p : String) : Prop :=
  hasNode g p = true → markOf marks p = some .done

def Frames (g : Graph) (marks : List (String × Mark)) (path : List String) (next : List Nat) : Prop :=
  ∀ k n i, path[k]? = some n → next[k]? = some i → ∀ t < i,
    Settled g marks ((parentsOf g n).getD t "") ∨
    (t + 1 = i ∧ path[k + 1]? = some ((parentsOf g n).getD t ""))

def Closed (g : Graph) (marks : List (String × Mark)) : Prop :=
  (∀ n, markOf marks n = some .done → ∀ p ∈ parentsOf g n, hasNode g p = true →
    markOf marks p = some .done) ∧
  (∀ c, IsCycle g c → ∃ n ∈ c, markOf marks n ≠ some .done)

def Inv (g : Graph) (w : Walk) : Prop :=
  w.path.length = w.next.length ∧ Linked g w.path ∧ w.path.Nodup ∧
  (∀ n, markOf w.marks n = some .open_ ↔ n ∈ w.path) ∧
  (∀ n ∈ w.path, hasNode g n = true) ∧ Closed g w.marks ∧ Frames g w.marks w.path w.next

def Grows (marks marks' : List (String × Mark)) : Prop :=
  (∀ n, markOf marks n = some .done → markOf marks' n = some .done) ∧
  (∀ n, markOf marks n ≠ none → markOf marks' n ≠ none)

def weight (marks : List (String × Mark)) : Graph → Nat
  | [] => 0
  | e :: rest => (match markOf marks e.1 with
      | none => e.2.length + 2
      | some _ => 0) + weight marks rest

def total : Graph → Nat
  | [] => 0
  | e :: rest => e.2.length + 2 + total rest

def frameWeight (g : Graph) : List String → List Nat → Nat
  | n :: ns, i :: is => ((parentsOf g n).length - i + 1) + frameWeight g ns is
  | _, _ => 0

def potential (g : Graph) (w : Walk) : Nat := weight w.marks g + frameWeight g w.path w.next

theorem grows_refl (marks : List (String × Mark)) : Grows marks marks := ⟨fun _ h => h, fun _ h => h⟩

theorem grows_trans {a b c : List (String × Mark)} (h1 : Grows a b) (h2 : Grows b c) : Grows a c :=
  ⟨fun n h => h2.1 n (h1.1 n h), fun n h => h2.2 n (h1.2 n h)⟩

theorem get_snoc {α : Type} (l : List α) (x : α) (k : Nat) :
    (l ++ [x])[k]? = if k < l.length then l[k]? else if k = l.length then some x else none := by
  by_cases h : k < l.length
  · rw [if_pos h, List.getElem?_append_left h]
  · rw [if_neg h, List.getElem?_append_right (by omega)]
    by_cases h2 : k = l.length
    · subst h2; simp
    · rw [if_neg h2]
      obtain ⟨j, hj⟩ : ∃ j, k - l.length = j + 1 := ⟨k - l.length - 1, by omega⟩
      rw [hj]
      rfl

theorem frameWeight_snoc (g : Graph) (n : String) (i : Nat) : ∀ (ps : List String) (is : List Nat),
    ps.length = is.length →
    frameWeight g (ps ++ [n]) (is ++ [i]) = frameWeight g ps is + ((parentsOf g n).length - i + 1) := by
  intro ps
  induction ps with
  | nil =>
    intro is h
    cases is with
    | nil => simp [frameWeight]
    | cons _ _ => simp at h
  | cons a rest ih =>
    intro is h
    cases is with
    | nil => simp at h
    | cons j js =>
      rw [List.cons_append, List.cons_append, frameWeight, frameWeight, ih js (by simpa using h)]
      omega

theorem weight_mono {marks marks' : List (String × Mark)}
    (h : ∀ k, markOf marks k ≠ none → markOf marks' k ≠ none) : ∀ es, weight marks' es ≤ weight marks es := by
  intro es
  induction es with
  | nil => exact Nat.le_refl _
  | cons e rest ih =>
    simp only [weight]
    have : (match markOf marks' e.1 with | none => e.2.length + 2 | some _ => 0) ≤
        (match markOf marks e.1 with | none => e.2.length + 2 | some _ => 0) := by
      cases h' : markOf marks' e.1 with
      | none =>
        have : markOf marks e.1 = none := by
          cases h'' : markOf marks e.1 with
          | none => rfl
          | some _ => exact absurd h' (h e.1 (by rw [h'']; exact Option.noConfusion))
        rw [this]; exact Nat.le_refl _
      | some _ => exact Nat.zero_le _
    omega

theorem weight_mark {marks marks' : List (String × Mark)} {p : String} {e : String × List String}
    (h : ∀ k, markOf marks k ≠ none → markOf marks' k ≠ none) (hp : markOf marks' p ≠ none)
    (hn : markOf marks p = none) (he : e.1 = p) :
    ∀ es, e ∈ es → weight marks' es + e.2.length + 2 ≤ weight marks es := by
  intro es
  induction es with
  | nil => intro h; cases h
  | cons e' rest ih =>
    intro hmem
    simp only [weight]
    have hle := weight_mono h rest
    rcases List.mem_cons.mp hmem with rfl | hmem
    · rw [he, hn]
      cases h' : markOf marks' p with
      | none => exact absurd h' hp
      | some v =>
        show 0 + weight marks' rest + e.2.length + 2 ≤ e.2.length + 2 + weight marks rest
        omega
    · have := ih hmem
      have : (match markOf marks' e'.1 with | none => e'.2.length + 2 | some _ => 0) ≤
          (match markOf marks e'.1 with | none => e'.2.length + 2 | some _ => 0) := by
        cases h' : markOf marks' e'.1 with
        | none =>
          have : markOf marks e'.1 = none := by
            cases h'' : markOf marks e'.1 with
            | none => rfl
            | some _ => exact absurd h' (h e'.1 (by rw [h'']; exact Option.noConfusion))
          rw [this]; exact Nat.le_refl _
        | some _ => exact Nat.zero_le _
      omega

theorem weight_le_total (marks : List (String × Mark)) : ∀ es, weight marks es ≤ total es := by
  intro es
  induction es with
  | nil => exact Nat.le_refl _
  | cons e rest ih =>
    simp only [weight, total]
    cases markOf marks e.1 with
    | none => exact Nat.add_le_add_left ih _
    | some _ =>
      show 0 + weight marks rest ≤ e.2.length + 2 + total rest
      omega

theorem total_eq : ∀ (es : Graph) (a : Nat),
    es.foldl (fun n e => n + e.2.length) a + es.length * 2 = a + total es := by
  intro es
  induction es with
  | nil => intro a; simp [total]
  | cons e rest ih =>
    intro a
    simp only [List.foldl, List.length_cons, total]
    have := ih (a + e.2.length)
    rw [Nat.succ_mul]
    omega

theorem step_eq (g : Graph) (w : Walk) (ps : List String) (node : String) (is : List Nat) (idx : Nat)
    (hp : w.path = ps ++ [node]) (hn : w.next = is ++ [idx]) :
    step g w =
      if idx ≥ (parentsOf g node).length then
        Sum.inl ⟨ps, is, setMark w.marks node .done⟩
      else if !hasNode g ((parentsOf g node).getD idx "") then
        Sum.inl ⟨ps ++ [node], is ++ [idx + 1], w.marks⟩
      else match markOf w.marks ((parentsOf g node).getD idx "") with
        | some .open_ => Sum.inr ((ps ++ [node]).drop
            (((ps ++ [node]).findIdx? (· == (parentsOf g node).getD idx "")).getD 0) ++
            [(parentsOf g node).getD idx ""])
        | some .done => Sum.inl ⟨ps ++ [node], is ++ [idx + 1], w.marks⟩
        | none => Sum.inl ⟨ps ++ [node] ++ [(parentsOf g node).getD idx ""], is ++ [idx + 1] ++ [0],
            setMark w.marks ((parentsOf g node).getD idx "") .open_⟩ := by
  cases w with
  | mk path next marks =>
    simp only at hp hn
    subst hp hn
    unfold step
    simp only [List.reverse_append, List.reverse_cons, List.reverse_nil, List.nil_append,
      List.singleton_append, List.dropLast_concat]

theorem nodup_snoc {l : List String} {x : String} : (l ++ [x]).Nodup ↔ l.Nodup ∧ x ∉ l := by
  unfold List.Nodup
  rw [List.pairwise_append]
  constructor
  · rintro ⟨h1, _, h3⟩
    exact ⟨h1, fun hx => h3 x hx x (List.mem_singleton_self _) rfl⟩
  · rintro ⟨h1, h2⟩
    refine ⟨h1, List.pairwise_singleton _ _, fun a ha b hb => ?_⟩
    rw [List.mem_singleton] at hb
    subst hb
    exact fun e => h2 (e ▸ ha)

theorem snoc_of_ne_nil {α : Type} {l : List α} (h : l ≠ []) : ∃ L b, l = L ++ [b] := by
  rcases List.eq_nil_or_concat l with e | ⟨L, b, e⟩
  · exact absurd e h
  · exact ⟨L, b, by rw [e, List.concat_eq_append]⟩

theorem below_length {α : Type} {l : List α} {k : Nat} {x : α} (h : l[k]? = some x) : k < l.length := by
  apply Nat.lt_of_not_le
  intro hk
  rw [List.getElem?_eq_none hk] at h
  cases h

theorem top_settled (g : Graph) (w : Walk) (ps : List String) (node : String) (is : List Nat) (idx : Nat)
    (hfr : Frames g w.marks (ps ++ [node]) (is ++ [idx])) (hl2 : ps.length = is.length) :
    ∀ t < idx, Settled g w.marks ((parentsOf g node).getD t "") := by
  intro t ht
  rcases hfr ps.length node idx (by rw [get_snoc]; simp) (by rw [get_snoc, ← hl2]; simp) t ht with h | ⟨_, h⟩
  · exact h
  · rw [get_snoc, if_neg (by omega), if_neg (by omega)] at h
    cases h

theorem getLast_drop {l : List String} {k : Nat} (h : l.drop k ≠ []) : (l.drop k).getLast? = l.getLast? := by
  conv => rhs; rw [← List.take_append_drop k l]
  rw [List.getLast?_append]
  obtain ⟨L, b, e⟩ := snoc_of_ne_nil h
  rw [e]
  simp

theorem inv_advance (g : Graph) (w : Walk) (ps : List String) (node : String) (is : List Nat) (idx : Nat)
    (hi : Inv g w) (hp : w.path = ps ++ [node]) (hn : w.next = is ++ [idx])
    (hlt : idx < (parentsOf g node).length) (hs : Settled g w.marks ((parentsOf g node).getD idx "")) :
    Inv g ⟨ps ++ [node], is ++ [idx + 1], w.marks⟩ ∧
    potential g ⟨ps ++ [node], is ++ [idx + 1], w.marks⟩ < potential g w := by
  obtain ⟨hlen, hlink, hnd, hopen, hex, hcl, hfr⟩ := hi
  have hl2 : ps.length = is.length := by rw [hp, hn] at hlen; simpa using hlen
  rw [hp, hn] at hfr
  have htop := top_settled g w ps node is idx hfr hl2
  refine ⟨⟨by simp [hl2], hp ▸ hlink, hp ▸ hnd, fun n => hp ▸ hopen n, fun n h => hex n (hp ▸ h), hcl, ?_⟩, ?_⟩
  · intro k n i hk1 hk2 t ht
    rw [get_snoc] at hk2
    by_cases hkl : k < is.length
    · rw [if_pos hkl] at hk2
      exact hfr k n i hk1 (by rw [get_snoc, if_pos hkl]; exact hk2) t ht
    · rw [if_neg hkl] at hk2
      by_cases hk3 : k = is.length
      · rw [if_pos hk3] at hk2
        injection hk2 with hk2
        subst hk2
        have : n = node := by
          simp only at hk1
          rw [get_snoc, if_neg (by omega), if_pos (by omega)] at hk1
          injection hk1 with hk1
          exact hk1.symm
        subst this
        left
        by_cases htl : t < idx
        · exact htop t htl
        · have : t = idx := by omega
          subst this
          exact hs
      · rw [if_neg hk3] at hk2
        cases hk2
  · unfold potential
    simp only []
    rw [hp, hn, frameWeight_snoc _ _ _ _ _ hl2, frameWeight_snoc _ _ _ _ _ hl2]
    omega

theorem inv_step (g : Graph) (w : Walk) (hi : Inv g w) (hne : w.path ≠ []) :
    (∀ w', step g w = .inl w' → Inv g w' ∧ potential g w' < potential g w ∧ Grows w.marks w'.marks) ∧
    (∀ c, step g w = .inr c → IsCycle g c) := by
  obtain ⟨ps, node, hp⟩ := snoc_of_ne_nil hne
  obtain ⟨is, idx, hn⟩ : ∃ is idx, w.next = is ++ [idx] := by
    apply snoc_of_ne_nil
    intro e
    have := hi.1
    rw [e, hp] at this
    simp at this
  have hi' := hi
  obtain ⟨hlen, hlink, hnd, hopen, hex, hcl, hfr⟩ := hi
  have hl2 : ps.length = is.length := by rw [hp, hn] at hlen; simpa using hlen
  rw [hp] at hlink hnd hex
  rw [hp, hn] at hfr
  have hopen' : ∀ n, markOf w.marks n = some .open_ ↔ n ∈ ps ++ [node] := fun n => hp ▸ hopen n
  have ⟨hnd1, hnode⟩ := nodup_snoc.mp hnd
  have htop := top_settled g w ps node is idx hfr hl2
  have hnodeopen : markOf w.marks node = some .open_ :=
    (hopen' node).mpr (List.mem_append_right _ (List.mem_singleton_self _))
  have hnodeex : hasNode g node = true := hex node (List.mem_append_right _ (List.mem_singleton_self _))
  rw [step_eq g w ps node is idx hp hn]
  split
  · rename_i hge
    refine ⟨?_, fun c hc => by cases hc⟩
    intro w' hw
    injection hw with hw
    subst hw
    have hM : ∀ k, markOf (setMark w.marks node .done) k =
        if k = node then some .done else markOf w.marks k := fun k => markOf_setMark _ _ _ _
    have hall : ∀ p ∈ parentsOf g node, hasNode g p = true → markOf w.marks p = some .done := by
      intro p hp' hpe
      obtain ⟨t, ht, e⟩ := mem_getD _ p hp'
      have := htop t (Nat.lt_of_lt_of_le ht hge)
      rw [e] at this
      exact this hpe
    have grows : Grows w.marks (setMark w.marks node .done) := by
      constructor
      · intro n h
        rw [hM]
        by_cases hk : n = node
        · rw [if_pos hk]
        · rw [if_neg hk]; exact h
      · intro n h
        rw [hM]
        by_cases hk : n = node
        · rw [if_pos hk]; exact Option.noConfusion
        · rw [if_neg hk]; exact h
    have doneM : ∀ p, markOf w.marks p = some .done → markOf (setMark w.marks node .done) p = some .done :=
      grows.1
    refine ⟨⟨hl2, linked_prefix hlink, hnd1, ?_, fun n h => hex n (List.mem_append_left _ h), ⟨?_, ?_⟩, ?_⟩, ?_, grows⟩
    · intro n
      rw [hM]
      by_cases hk : n = node
      · rw [if_pos hk]
        constructor
        · intro h; cases h
        · intro h; rw [hk] at h; exact absurd h hnode
      · rw [if_neg hk, hopen']
        constructor
        · intro h
          rcases List.mem_append.mp h with h | h
          · exact h
          · exact absurd (List.mem_singleton.mp h) hk
        · intro h; exact List.mem_append_left _ h
    · intro n hn p hp' hpe
      rw [hM] at hn
      by_cases hk : n = node
      · rw [hk] at hp'
        exact doneM p (hall p hp' hpe)
      · rw [if_neg hk] at hn
        exact doneM p (hcl.1 n hn p hp' hpe)
    · intro c hc
      obtain ⟨n0, hn0, hnd0⟩ := hcl.2 c hc
      apply Classical.byContradiction
      intro hcon
      have hallc : ∀ n ∈ c, markOf (setMark w.marks node .done) n = some .done :=
        fun n hn => Classical.byContradiction (fun h => hcon ⟨n, hn, h⟩)
      have h0 := hallc n0 hn0
      rw [hM] at h0
      by_cases hk : n0 = node
      · have hnc : node ∈ c := hk ▸ hn0
        obtain ⟨q, hq, hnq⟩ := cycle_pred hc node hnc
        have hqd := hallc q hq
        rw [hM] at hqd
        by_cases hqk : q = node
        · rw [hqk] at hnq
          have := hall node hnq hnodeex
          rw [hnodeopen] at this
          cases this
        · rw [if_neg hqk] at hqd
          have := hcl.1 q hqd node hnq hnodeex
          rw [hnodeopen] at this
          cases this
      · rw [if_neg hk] at h0
        exact hnd0 h0
    · intro k n i hk1 hk2 t ht
      have hkl : k < ps.length := below_length hk1
      have hk1' : (ps ++ [node])[k]? = some n := by rw [get_snoc, if_pos hkl]; exact hk1
      have hk2' : (is ++ [idx])[k]? = some i := by rw [get_snoc, if_pos (hl2 ▸ hkl)]; exact hk2
      rcases hfr k n i hk1' hk2' t ht with h | ⟨h1, h2⟩
      · left; exact fun he => doneM _ (h he)
      · rw [get_snoc] at h2
        by_cases hk3 : k + 1 < ps.length
        · right; rw [if_pos hk3] at h2; exact ⟨h1, h2⟩
        · rw [if_neg hk3] at h2
          by_cases hk4 : k + 1 = ps.length
          · rw [if_pos hk4] at h2
            left
            intro _
            injection h2 with h2
            rw [← h2, hM, if_pos rfl]
          · rw [if_neg hk4] at h2
            cases h2
    · unfold potential
      simp only []
      rw [hp, hn, frameWeight_snoc _ _ _ _ _ hl2]
      have := weight_mono grows.2 g
      omega
  · rename_i hge
    have hlt : idx < (parentsOf g node).length := Nat.lt_of_not_le hge
    split
    · rename_i hdang
      refine ⟨?_, fun c hc => by cases hc⟩
      intro w' hw
      injection hw with hw
      subst hw
      have hs : Settled g w.marks ((parentsOf g node).getD idx "") := by
        intro he
        rw [he] at hdang
        cases hdang
      have ⟨a, b⟩ := inv_advance g w ps node is idx hi' hp hn hlt hs
      exact ⟨a, b, grows_refl _⟩
    · rename_i hdang
      have hqex : hasNode g ((parentsOf g node).getD idx "") = true := by
        cases e : hasNode g ((parentsOf g node).getD idx "")
        · rw [e] at hdang; exact absurd rfl hdang
        · rfl
      split
      · rename_i heq
        refine ⟨(fun w' hw => by cases hw), ?_⟩
        intro c hc
        injection hc with hc
        subst hc
        have hqin := (hopen' _).mp heq
        obtain ⟨k, hk, rest, hr⟩ := findIdx_mem (ps ++ [node]) ((parentsOf g node).getD idx "") 0 hqin
        rw [hk, Option.getD_some, Nat.zero_add, hr]
        have hl := linked_drop k hlink
        rw [hr] at hl
        have hlast : (((parentsOf g node).getD idx "") :: rest).getLast? = some node := by
          rw [← hr, getLast_drop (by rw [hr]; exact List.cons_ne_nil _ _)]
          simp
        obtain ⟨L, b, e⟩ := snoc_of_ne_nil (List.cons_ne_nil ((parentsOf g node).getD idx "") rest)
        rw [e] at hl hlast
        rw [List.getLast?_append] at hlast
        have hb' : some b = some node := hlast
        injection hb' with hb
        subst hb
        refine ⟨?_, ?_, ?_⟩
        · rw [e]
          exact linked_snoc hl (getD_mem _ _ hlt)
        · simp
        · rw [List.getLast?_append]
          rfl
      · rename_i heq
        refine ⟨?_, fun c hc => by cases hc⟩
        intro w' hw
        injection hw with hw
        subst hw
        have ⟨a, b⟩ := inv_advance g w ps node is idx hi' hp hn hlt (fun _ => heq)
        exact ⟨a, b, grows_refl _⟩
      · rename_i hqn
        refine ⟨?_, fun c hc => by cases hc⟩
        intro w' hw
        injection hw with hw
        subst hw
        generalize hq : (parentsOf g node).getD idx "" = q at hqex hqn
        have hqmem : q ∈ parentsOf g node := hq ▸ getD_mem _ _ hlt
        have hM : ∀ k, markOf (setMark w.marks q .open_) k =
            if k = q then some .open_ else markOf w.marks k := fun k => markOf_setMark _ _ _ _
        have hqnot : q ∉ ps ++ [node] := fun h => by
          have := (hopen' q).mpr h
          rw [hqn] at this
          cases this
        have donesame : ∀ k, markOf (setMark w.marks q .open_) k = some .done ↔
            markOf w.marks k = some .done := by
          intro k
          rw [hM]
          by_cases hk : k = q
          · rw [if_pos hk, hk, hqn]
            constructor <;> intro h <;> cases h
          · rw [if_neg hk]
        have grows : Grows w.marks (setMark w.marks q .open_) := by
          constructor
          · intro n h; exact (donesame n).mpr h
          · intro n h
            rw [hM]
            by_cases hk : n = q
            · rw [if_pos hk]; exact Option.noConfusion
            · rw [if_neg hk]; exact h
        have tr : ∀ p, Settled g w.marks p → Settled g (setMark w.marks q .open_) p :=
          fun p h he => (donesame p).mpr (h he)
        refine ⟨⟨by simp [hl2], linked_snoc hlink hqmem, nodup_snoc.mpr ⟨hnd, hqnot⟩, ?_, ?_,
          ⟨?_, ?_⟩, ?_⟩, ?_, grows⟩
        · intro n
          rw [hM]
          by_cases hk : n = q
          · rw [if_pos hk]
            constructor
            · intro _; exact List.mem_append_right _ (hk ▸ List.mem_singleton_self _)
            · intro _; rfl
          · rw [if_neg hk, hopen']
            constructor
            · intro h; exact List.mem_append_left _ h
            · intro h
              rcases List.mem_append.mp h with h | h
              · exact h
              · exact absurd (List.mem_singleton.mp h) hk
        · intro n hn
          rcases List.mem_append.mp hn with h | h
          · exact hex n h
          · rw [List.mem_singleton.mp h]; exact hqex
        · intro n hn p hp' hpe
          rw [donesame] at hn ⊢
          exact hcl.1 n hn p hp' hpe
        · intro c hc
          obtain ⟨n, hn, h⟩ := hcl.2 c hc
          exact ⟨n, hn, fun e => h ((donesame n).mp e)⟩
        · intro k n i hk1 hk2 t ht
          rw [get_snoc, get_snoc] at hk1
          rw [get_snoc, get_snoc] at hk2
          simp only [List.length_append, List.length_singleton] at hk1 hk2
          by_cases hkl : k < ps.length
          · rw [if_pos (by omega), if_pos hkl] at hk1
            rw [if_pos (by omega), if_pos (hl2 ▸ hkl)] at hk2
            rcases hfr k n i (by rw [get_snoc, if_pos hkl]; exact hk1)
              (by rw [get_snoc, if_pos (hl2 ▸ hkl)]; exact hk2) t ht with h | ⟨h1, h2⟩
            · exact Or.inl (tr _ h)
            · right
              refine ⟨h1, ?_⟩
              rw [List.getElem?_append_left (by simp; omega)]
              exact h2
          · by_cases hk3 : k = ps.length
            · rw [if_pos (by omega), if_neg hkl, if_pos hk3] at hk1
              rw [if_pos (by omega), if_neg (by omega), if_pos (by omega)] at hk2
              injection hk1 with hk1
              injection hk2 with hk2
              subst hk1 hk2 hk3
              by_cases htl : t < idx
              · exact Or.inl (tr _ (htop t htl))
              · have : t = idx := by omega
                subst this
                right
                refine ⟨rfl, ?_⟩
                rw [hq, get_snoc]
                simp
            · by_cases hk4 : k = ps.length + 1
              · rw [if_neg (by omega), if_pos (by omega)] at hk2
                injection hk2 with hk2
                subst hk2
                exact absurd ht (Nat.not_lt_zero _)
              · rw [if_neg (by omega), if_neg (by omega)] at hk2
                cases hk2
        · obtain ⟨e, he, he1, he2⟩ := parents_entry hqex
          have hw := weight_mark grows.2 (by rw [hM, if_pos rfl]; exact Option.noConfusion) hqn he1 g he
          rw [he2] at hw
          unfold potential
          simp only []
          rw [hp, hn, frameWeight_snoc _ _ _ _ _ (by simp [hl2]), frameWeight_snoc _ _ _ _ _ hl2,
            frameWeight_snoc _ _ _ _ _ hl2]
          omega

theorem walk_cycle (g : Graph) : ∀ (fuel : Nat) (w : Walk) (c : List String),
    Inv g w → walk g fuel w = .inr c → IsCycle g c := by
  intro fuel
  induction fuel with
  | zero => intro w c _ h; cases h
  | succ f ih =>
    intro w c hi h
    rw [walk] at h
    split at h
    · cases h
    · rename_i hne
      have hne' : w.path ≠ [] := fun e => hne (by rw [e]; rfl)
      have ⟨h1, h2⟩ := inv_step g w hi hne'
      split at h
      · rename_i c' hs
        injection h with h
        subst h
        exact h2 c' hs
      · rename_i w' hs
        exact ih w' c (h1 w' hs).1 h

theorem walk_done (g : Graph) : ∀ (fuel : Nat) (w : Walk) (m : List (String × Mark)),
    Inv g w → potential g w < fuel → walk g fuel w = .inl m →
    (∀ n, markOf m n ≠ some .open_) ∧ Closed g m ∧ Grows w.marks m := by
  intro fuel
  induction fuel with
  | zero => intro _ _ _ h; exact absurd h (Nat.not_lt_zero _)
  | succ f ih =>
    intro w m hi hf h
    rw [walk] at h
    split at h
    · rename_i he
      injection h with h
      subst h
      have hp : w.path = [] := List.isEmpty_iff.mp he
      refine ⟨fun n hn => ?_, hi.2.2.2.2.2.1, grows_refl _⟩
      have := (hi.2.2.2.1 n).mp hn
      rw [hp] at this
      cases this
    · rename_i hne
      have hne' : w.path ≠ [] := fun e => hne (by rw [e]; rfl)
      have ⟨h1, _⟩ := inv_step g w hi hne'
      split at h
      · cases h
      · rename_i w' hs
        have ⟨a, b, c⟩ := h1 w' hs
        have ⟨r1, r2, r3⟩ := ih w' m a (by omega) h
        exact ⟨r1, r2, grows_trans c r3⟩

def Ready (g : Graph) (marks : List (String × Mark)) : Prop :=
  (∀ n, markOf marks n ≠ some .open_) ∧ Closed g marks

theorem start_inv (g : Graph) (marks : List (String × Mark)) (root : String) (hr : Ready g marks)
    (hex : hasNode g root = true) (hnd : markOf marks root ≠ some .done) :
    Inv g ⟨[root], [0], setMark marks root .open_⟩ ∧ Grows marks (setMark marks root .open_) ∧
    potential g ⟨[root], [0], setMark marks root .open_⟩ < total g + 1 := by
  have hnone : markOf marks root = none := by
    cases h : markOf marks root with
    | none => rfl
    | some x =>
      cases x with
      | open_ => exact absurd h (hr.1 root)
      | done => exact absurd h hnd
  have hM : ∀ k, markOf (setMark marks root .open_) k =
      if k = root then some .open_ else markOf marks k := fun k => markOf_setMark _ _ _ _
  have donesame : ∀ k, markOf (setMark marks root .open_) k = some .done ↔ markOf marks k = some .done := by
    intro k
    rw [hM]
    by_cases hk : k = root
    · rw [if_pos hk, hk, hnone]
      constructor <;> intro h <;> cases h
    · rw [if_neg hk]
  have grows : Grows marks (setMark marks root .open_) := by
    constructor
    · intro n h; exact (donesame n).mpr h
    · intro n h
      rw [hM]
      by_cases hk : n = root
      · rw [if_pos hk]; exact Option.noConfusion
      · rw [if_neg hk]; exact h
  refine ⟨⟨rfl, trivial, List.pairwise_singleton _ _, ?_, ?_, ⟨?_, ?_⟩, ?_⟩, grows, ?_⟩
  · intro n
    rw [hM, List.mem_singleton]
    by_cases hk : n = root
    · rw [if_pos hk]; exact ⟨fun _ => hk, fun _ => rfl⟩
    · rw [if_neg hk]; exact ⟨fun h => absurd h (hr.1 n), fun h => absurd h hk⟩
  · intro n hn
    rw [List.mem_singleton.mp hn]
    exact hex
  · intro n hn p hp hpe
    rw [donesame] at hn ⊢
    exact hr.2.1 n hn p hp hpe
  · intro c hc
    obtain ⟨n, hn, h⟩ := hr.2.2 c hc
    exact ⟨n, hn, fun e => h ((donesame n).mp e)⟩
  · intro k n i _ hk2 t ht
    cases k with
    | zero =>
      injection hk2 with hk2
      subst hk2
      exact absurd ht (Nat.not_lt_zero _)
    | succ k => cases hk2
  · obtain ⟨e, he, he1, he2⟩ := parents_entry hex
    have hw := weight_mark grows.2 (by rw [hM, if_pos rfl]; exact Option.noConfusion) hnone he1 g he
    rw [he2] at hw
    have ht := weight_le_total marks g
    unfold potential
    simp only [frameWeight]
    omega

theorem search_some (g : Graph) (fuel : Nat) (hfuel : total g < fuel) :
    ∀ (roots : List String) (marks : List (String × Mark)) (c : List String),
    Ready g marks → (∀ r ∈ roots, hasNode g r = true) → searchFrom g fuel roots marks = some c →
    IsCycle g c := by
  intro roots
  induction roots with
  | nil => intro _ _ _ _ h; cases h
  | cons root rest ih =>
    intro marks c hr hroots h
    rw [searchFrom] at h
    split at h
    · exact ih marks c hr (fun r hr' => hroots r (List.mem_cons_of_mem _ hr')) h
    · rename_i hnd
      have hnd' : markOf marks root ≠ some .done := fun e => hnd (by rw [e]; exact beq_self_eq_true _)
      have ⟨hi, _, hp⟩ := start_inv g marks root hr (hroots root (List.mem_cons_self _ _)) hnd'
      split at h
      · rename_i cycle hw
        injection h with h
        subst h
        exact walk_cycle g fuel _ cycle hi hw
      · rename_i m hw
        have ⟨a, b, _⟩ := walk_done g fuel _ m hi (by omega) hw
        exact ih m c ⟨a, b⟩ (fun r hr' => hroots r (List.mem_cons_of_mem _ hr')) h

theorem search_none (g : Graph) (fuel : Nat) (hfuel : total g < fuel) :
    ∀ (roots : List String) (marks : List (String × Mark)),
    Ready g marks → (∀ r ∈ roots, hasNode g r = true) →
    (∀ n, hasNode g n = true → n ∈ roots ∨ markOf marks n = some .done) →
    searchFrom g fuel roots marks = none → ∀ c, ¬ IsCycle g c := by
  intro roots
  induction roots with
  | nil =>
    intro marks hr _ hall _ c hc
    obtain ⟨n, hn, h⟩ := hr.2.2 c hc
    rcases hall n (cycle_exist hc n hn) with h' | h'
    · cases h'
    · exact h h'
  | cons root rest ih =>
    intro marks hr hroots hall h
    rw [searchFrom] at h
    split at h
    · rename_i hd
      have hd' : markOf marks root = some .done := by
        cases e : markOf marks root with
        | none => rw [e] at hd; cases hd
        | some x => cases x with
          | open_ => rw [e] at hd; cases hd
          | done => rfl
      refine ih marks hr (fun r hr' => hroots r (List.mem_cons_of_mem _ hr')) ?_ h
      intro n hn
      rcases hall n hn with h' | h'
      · rcases List.mem_cons.mp h' with rfl | h'
        · exact Or.inr hd'
        · exact Or.inl h'
      · exact Or.inr h'
    · rename_i hnd
      have hnd' : markOf marks root ≠ some .done := fun e => hnd (by rw [e]; exact beq_self_eq_true _)
      have ⟨hi, g1, hp⟩ := start_inv g marks root hr (hroots root (List.mem_cons_self _ _)) hnd'
      split at h
      · cases h
      · rename_i m hw
        have ⟨a, b, g2⟩ := walk_done g fuel _ m hi (by omega) hw
        have hrootdone : markOf m root = some .done := by
          have hmk := g2.2 root (by
            show markOf (setMark marks root .open_) root ≠ none
            rw [markOf_setMark, if_pos rfl]
            exact Option.noConfusion)
          cases e : markOf m root with
          | none => exact absurd e hmk
          | some x => cases x with
            | open_ => exact absurd e (a root)
            | done => rfl
        refine ih m ⟨a, b⟩ (fun r hr' => hroots r (List.mem_cons_of_mem _ hr')) ?_ h
        intro n hn
        rcases hall n hn with h' | h'
        · rcases List.mem_cons.mp h' with rfl | h'
          · exact Or.inr hrootdone
          · exact Or.inl h'
        · exact Or.inr (g2.1 n (g1.1 n h'))

theorem ready_empty (g : Graph) : Ready g [] := by
  refine ⟨(fun n h => by cases h), (fun n h => by cases h), ?_⟩
  intro c hc
  obtain ⟨_, h2, _⟩ := hc
  match c, h2 with
  | a :: _, _ => exact ⟨a, List.mem_cons_self _ _, fun h => by cases h⟩

/-- @proves REQ-REQDOC.refines_dag -/
theorem a_cycle_is_reported_exactly_when_there_is_one (edges : Graph) :
    ((graphReport edges).cycle = none ↔ ∀ c, ¬ IsCycle (canonGraph edges) c) ∧
    (∀ c, (graphReport edges).cycle = some c → IsCycle (canonGraph edges) c) := by
  have hfuel : total (canonGraph edges) < (canonGraph edges).length * 2 +
      (canonGraph edges).foldl (fun n e => n + e.2.length) 0 + 1 := by
    have := total_eq (canonGraph edges) 0
    omega
  have hroots : ∀ r ∈ roots (canonGraph edges), hasNode (canonGraph edges) r = true := by
    intro r hr
    obtain ⟨e, he, rfl⟩ := List.mem_map.mp hr
    exact List.any_eq_true.mpr ⟨e, he, beq_self_eq_true _⟩
  have hall : ∀ n, hasNode (canonGraph edges) n = true → n ∈ roots (canonGraph edges) ∨
      markOf [] n = some .done := by
    intro n hn
    obtain ⟨e, he, h⟩ := List.any_eq_true.mp hn
    left
    exact List.mem_map.mpr ⟨e, he, (beq_iff_eq _ _).mp h⟩
  have hsome := search_some _ _ hfuel (roots (canonGraph edges)) []
  have hnone := search_none _ _ hfuel (roots (canonGraph edges)) [] (ready_empty _) hroots hall
  show (cycleIn (canonGraph edges) = none ↔ _) ∧ (∀ c, cycleIn (canonGraph edges) = some c → _)
  unfold cycleIn
  simp only []
  refine ⟨⟨hnone, ?_⟩, fun c h => hsome c (ready_empty _) hroots h⟩
  intro hac
  cases h : searchFrom (canonGraph edges) _ (roots (canonGraph edges)) [] with
  | none => rfl
  | some c => exact absurd (hsome c (ready_empty _) hroots h) (hac c)

end TraceLean.Refinement
