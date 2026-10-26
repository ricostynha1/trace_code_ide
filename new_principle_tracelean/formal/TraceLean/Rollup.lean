import TraceLean.Evidence

open Lean (Json ToJson FromJson toJson)

namespace TraceLean.Rollup

open TraceLean.Evidence

/-- @models REQ-ROLLUP.every_part_holds
@models ARCH-HONEST.weakest_link -/
def combine (parts : List Marks) : Marks :=
  match parts with
  | [] => Marks.empty
  | first :: rest => (first :: rest).foldl Marks.both Marks.neutral

theorem both_keeps (a b : Marks) :
    ((Marks.both a b).tested || (Marks.both a b).structural) = true →
      (a.tested || a.structural) = true := by
  rcases a with ⟨_, _, t, _, s⟩
  cases t <;> cases s <;> simp [Marks.both]

theorem fold_keeps (rest : List Marks) : ∀ acc : Marks,
    ((rest.foldl Marks.both acc).tested || (rest.foldl Marks.both acc).structural) = true →
      (acc.tested || acc.structural) = true := by
  induction rest with
  | nil => intro acc h; simpa using h
  | cons x xs ih => intro acc h; exact both_keeps acc x (ih (Marks.both acc x) h)

theorem a_part_without_a_mark_clears_it (first : Marks) (rest : List Marks) :
    (combine (first :: rest)).holds ⟨false, false, true, false, false⟩ = true →
      (first.tested || first.structural) = true := by
  intro h
  have held : ((combine (first :: rest)).tested || (combine (first :: rest)).structural) = true := by
    simp only [Marks.holds] at h
    simp_all
  simp only [combine, List.foldl] at held
  have := fold_keeps rest (Marks.both Marks.neutral first) held
  rcases first with ⟨_, _, t, _, s⟩
  cases t <;> cases s <;> simp_all [Marks.both, Marks.neutral]

structure Figure where
  met : Nat
  total : Nat
  exact : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-ROLLUP.never_complete_when_open -/
def Figure.isComplete (f : Figure) : Bool := f.exact && f.total > 0 && f.met == f.total

/-- @models ARCH-HONEST.lower_bound_marked -/
def render (f : Figure) : String :=
  let percent := if f.total == 0 then 0 else (f.met * 200 + f.total) / (f.total * 2)
  if f.exact then s!"{percent}%" else s!"≤ {percent}% (provisional)"

/-- @proves ARCH-HONEST.lower_bound_marked -/
theorem an_open_figure_is_marked (met total : Nat) :
    render { met := met, total := total, exact := false } =
      s!"≤ {if total == 0 then 0 else (met * 200 + total) / (total * 2)}% (provisional)" := by
  simp [render]

structure Node where
  id : String
  complete : Bool
  clauses : List (Option String)
  exempt : List (Option String)
  partialClauses : List (Option String)
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive RollUp where
  | mk (id : String) (marks : Marks) (covered : Figure) (children : List RollUp)
  deriving Repr, Inhabited

def RollUp.marks : RollUp → Marks
  | .mk _ m _ _ => m

def RollUp.covered : RollUp → Figure
  | .mk _ _ f _ => f

partial def RollUp.toJsonValue : RollUp → Json
  | .mk id marks covered children =>
    Json.mkObj [("id", Json.str id), ("marks", toJson marks),
                ("covered", toJson covered),
                ("children", Json.arr (children.map RollUp.toJsonValue).toArray)]

instance : ToJson RollUp := ⟨RollUp.toJsonValue⟩

def marksAt (held : List ((String × Option String) × Marks)) (key : String × Option String)
    : Option Marks :=
  (held.find? (·.1 == key)).map (·.2)

def partialCap (m : Marks) : Marks := { m with tested := false, proved := false }

def codesLe : List Nat → List Nat → Bool
  | [], _ => true
  | _ :: _, [] => false
  | a :: xs, b :: ys => decide (a < b) || (decide (a = b) && codesLe xs ys)

def codes (s : String) : List Nat := s.data.map Char.toNat

def idLe (a b : Node) : Bool := codesLe (codes a.id) (codes b.id)

def dedupLast : List Node → List Node
  | [] => []
  | n :: rest => if rest.any (fun m => m.id == n.id) then dedupLast rest else n :: dedupLast rest

def canonNodes (nodes : List Node) : List Node :=
  (dedupLast nodes).mergeSort idLe

def canonHeld (held : List ((String × Option String) × Marks))
    : List ((String × Option String) × Marks) :=
  held.foldl
    (fun acc kv => if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
                   else acc ++ [kv]) []

def grow (graph : List Node) (found : List String) : List String :=
  found ++ ((graph.filter (fun n =>
    !(found.contains n.id) && n.refines.any (fun p => found.contains p))).map (·.id))

def growFor (graph : List Node) : Nat → List String → List String
  | 0, found => found
  | k + 1, found => growFor graph k (grow graph found)

def reach (graph : List Node) (root : String) : List String :=
  growFor graph (graph.length + 1) [root]

def members (graph : List Node) (root : String) : List Node :=
  graph.filter (fun n => (reach graph root).contains n.id)

def ownMarks (held : List ((String × Option String) × Marks)) (n : Node) : List Marks :=
  (n.clauses.filter (fun c => !(n.exempt.contains c))).map (fun c =>
    if n.partialClauses.contains c then partialCap ((marksAt held (n.id, c)).getD Marks.empty)
    else (marksAt held (n.id, c)).getD Marks.empty)

def reachMarks (graph : List Node) (held : List ((String × Option String) × Marks))
    (root : String) : List Marks :=
  (members graph root).bind (ownMarks held)

def figureOf (graph : List Node) (held : List ((String × Option String) × Marks))
    (floor : Marks) (root : String) : Figure :=
  let all := reachMarks graph held root
  { met := (all.filter (fun m => m.holds floor)).length,
    total := all.length,
    exact := graph.any (·.id == root) && (members graph root).all (·.complete) }

def rollUpTree (graph : List Node) (held : List ((String × Option String) × Marks))
    (floor : Marks) : Nat → List String → String → RollUp
  | 0, _, id => .mk id (combine (reachMarks graph held id)) (figureOf graph held floor id) []
  | fuel + 1, path, id =>
    .mk id (combine (reachMarks graph held id)) (figureOf graph held floor id)
      ((graph.filter (fun n => n.refines.contains id && !((path ++ [id]).contains n.id))).map
        (fun n => rollUpTree graph held floor fuel (path ++ [id]) n.id))

/-- @models REQ-ROLLUP.open_is_lower_bound
@models REQ-ROLLUP.exempt_leaves_denominator
@models REQ-ROLLUP.partial_capped
@models REQ-ROLLUP.deterministic_order
@models REQ-ROLLUP.counted_once -/
def rollUp (nodes : List Node) (held : List ((String × Option String) × Marks))
    (root : String) (floor : Marks) : RollUp :=
  let graph := canonNodes nodes
  rollUpTree graph (canonHeld held) floor (graph.length + 1) [] root

/-- @proves REQ-ROLLUP.never_complete_when_open -/
theorem an_unclaimed_decomposition_is_never_complete (met total : Nat) :
    Figure.isComplete { met := met, total := total, exact := false } = false := by
  simp [Figure.isComplete]

/-- @proves REQ-ROLLUP.counted_once -/
theorem marks_are_over_the_reachable_set (nodes : List Node)
    (held : List ((String × Option String) × Marks)) (root : String) (floor : Marks) :
    (rollUp nodes held root floor).marks =
      combine (reachMarks (canonNodes nodes) (canonHeld held) root) := by
  simp [rollUp, rollUpTree, RollUp.marks]

theorem codesLe_total (xs : List Nat) :
    ∀ ys, codesLe xs ys = true ∨ codesLe ys xs = true := by
  induction xs
  case nil =>
    intro ys
    left
    simp [codesLe]
  case cons a xs ih =>
    intro ys
    cases ys
    case nil =>
      right
      simp [codesLe]
    case cons b ys =>
      simp only [codesLe, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq]
      refine (Nat.lt_trichotomy a b).elim (fun h => ?_) (fun h => ?_)
      · exact Or.inl (Or.inl h)
      · refine h.elim (fun he => ?_) (fun hb => ?_)
        · refine (ih ys).elim (fun hx => ?_) (fun hy => ?_)
          · exact Or.inl (Or.inr ⟨he, hx⟩)
          · exact Or.inr (Or.inr ⟨he.symm, hy⟩)
        · exact Or.inr (Or.inl hb)

theorem codesLe_trans (xs : List Nat) :
    ∀ ys zs, codesLe xs ys = true → codesLe ys zs = true → codesLe xs zs = true := by
  induction xs
  case nil =>
    intro ys zs _ _
    simp [codesLe]
  case cons a xs ih =>
    intro ys zs h1 h2
    cases ys
    case nil =>
      exact absurd h1 (Bool.false_ne_true)
    case cons b ys =>
      cases zs
      case nil =>
        exact absurd h2 (Bool.false_ne_true)
      case cons c zs =>
        simp only [codesLe, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq] at h1 h2
        simp only [codesLe, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq]
        refine h1.elim (fun hab => ?_) (fun hab => ?_)
        · refine h2.elim (fun hbc => ?_) (fun hbc => ?_)
          · exact Or.inl (Nat.lt_trans hab hbc)
          · exact Or.inl (Nat.lt_of_lt_of_le hab (Nat.le_of_eq hbc.1))
        · refine h2.elim (fun hbc => ?_) (fun hbc => ?_)
          · exact Or.inl (Nat.lt_of_le_of_lt (Nat.le_of_eq hab.1) hbc)
          · exact Or.inr ⟨Eq.trans hab.1 hbc.1, ih ys zs hab.2 hbc.2⟩

theorem codesLe_antisymm (xs : List Nat) :
    ∀ ys, codesLe xs ys = true → codesLe ys xs = true → xs = ys := by
  induction xs
  case nil =>
    intro ys _ h2
    cases ys
    case nil => rfl
    case cons _ _ => exact absurd h2 (Bool.false_ne_true)
  case cons a xs ih =>
    intro ys h1 h2
    cases ys
    case nil => exact absurd h1 (Bool.false_ne_true)
    case cons b ys =>
      simp only [codesLe, Bool.or_eq_true, Bool.and_eq_true, decide_eq_true_eq] at h1 h2
      refine h1.elim (fun hab => ?_) (fun hab => ?_)
      · refine h2.elim (fun hba => ?_) (fun hba => ?_)
        · exact absurd (Nat.lt_trans hab hba) (Nat.lt_irrefl a)
        · exact absurd (Nat.lt_of_lt_of_le hab (Nat.le_of_eq hba.1)) (Nat.lt_irrefl a)
      · refine h2.elim (fun hba => ?_) (fun hba => ?_)
        · exact absurd (Nat.lt_of_le_of_lt (Nat.le_of_eq hab.1) hba) (Nat.lt_irrefl a)
        · rw [hab.1, ih ys hab.2 hba.2]

theorem chars_of_codes (xs : List Char) :
    ∀ ys, xs.map Char.toNat = ys.map Char.toNat → xs = ys := by
  induction xs
  case nil =>
    intro ys h
    cases ys
    case nil => rfl
    case cons y ys => exact absurd h (List.cons_ne_nil _ _).symm
  case cons x xs ih =>
    intro ys h
    cases ys
    case nil => exact absurd h (List.cons_ne_nil _ _)
    case cons y ys =>
      have parts := List.cons.inj h
      have same : x = y := Char.eq_of_val_eq (UInt32.eq_of_val_eq (Fin.eq_of_val_eq parts.1))
      rw [same, ih ys parts.2]

theorem string_of_codes (a b : String) (h : codes a = codes b) : a = b := by
  cases a
  cases b
  exact congrArg String.mk (chars_of_codes _ _ h)

theorem eq_of_same_id (l : List Node) (distinct : List.Pairwise (fun a b => Not (a.id = b.id)) l)
    (a b : Node) (ha : List.Mem a l) (hb : List.Mem b l) (same : a.id = b.id) : a = b := by
  induction l
  case nil => exact absurd ha (List.not_mem_nil a)
  case cons x rest ih =>
    have d := List.pairwise_cons.mp distinct
    cases ha
    case head =>
      cases hb
      case head => rfl
      case tail hb => exact absurd same (d.1 b hb)
    case tail ha =>
      cases hb
      case head => exact absurd same.symm (d.1 a ha)
      case tail hb => exact ih d.2 ha hb

theorem dedupLast_of_distinct (l : List Node)
    (distinct : List.Pairwise (fun a b => Not (a.id = b.id)) l) : dedupLast l = l := by
  induction l
  case nil => rfl
  case cons n rest ih =>
    have d := List.pairwise_cons.mp distinct
    have none : rest.any (fun m => m.id == n.id) = false := by
      rw [List.any_eq]
      simp only [beq_iff_eq, decide_eq_false_iff_not]
      intro found
      exact found.elim (fun m hm => d.1 m hm.1 hm.2.symm)
    simp only [dedupLast, none, ih d.2]
    rfl

def idRel (a b : Node) : Prop := idLe a b = true

theorem sorted_eq (one two : List Node)
    (w : ∀ a b, List.Mem a one → List.Mem b two → idRel a b → idRel b a → a = b)
    (h1 : List.Pairwise idRel one) (h2 : List.Pairwise idRel two) (p : List.Perm one two) :
    one = two :=
  List.Perm.eq_of_sorted w h1 h2 p

theorem canonNodes_perm (l other : List Node) (p : List.Perm l other)
    (distinct : List.Pairwise (fun a b => Not (a.id = b.id)) l) :
    canonNodes l = canonNodes other := by
  have distinctOther := p.pairwise distinct (fun h e => h e.symm)
  simp only [canonNodes, dedupLast_of_distinct l distinct, dedupLast_of_distinct other distinctOther]
  have trans : ∀ (a b c : Node), idLe a b = true → idLe b c = true → idLe a c = true :=
    fun a b c => codesLe_trans (codes a.id) (codes b.id) (codes c.id)
  have total : ∀ (a b : Node), (!idLe a b) = true → idLe b a = true := by
    intro a b h
    refine (codesLe_total (codes a.id) (codes b.id)).elim (fun hl => ?_) id
    simp only [idLe, hl, Bool.not_true] at h
    exact absurd h Bool.false_ne_true
  apply sorted_eq
  · intro a b ha hb hab hba
    exact eq_of_same_id l distinct a b (List.mem_mergeSort.mp ha)
      ((p.mem_iff).mpr (List.mem_mergeSort.mp hb))
      (string_of_codes _ _ (codesLe_antisymm _ _ hab hba))
  · exact List.mergeSort_sorted trans total l
  · exact List.mergeSort_sorted trans total other
  · exact (List.mergeSort_perm idLe l).trans (p.trans (List.mergeSort_perm idLe other).symm)

/-- @proves REQ-ROLLUP.deterministic_order -/
theorem rollUp_perm (nodes other : List Node) (p : List.Perm nodes other)
    (distinct : List.Pairwise (fun a b => Not (a.id = b.id)) nodes)
    (held : List ((String × Option String) × Marks)) (root : String) (floor : Marks) :
    rollUp nodes held root floor = rollUp other held root floor := by
  simp only [rollUp, canonNodes_perm nodes other p distinct]

def dedupe (xs : List String) : List String :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

/-- @models REQ-ROLLUP.untraced_counted -/
def fileCoverage (scanned claimed : List String) : Figure :=
  let scanned := dedupe scanned
  let claimed := (dedupe claimed).filter (scanned.contains ·)
  { met := claimed.length, total := scanned.length, exact := true }

/-- @proves REQ-ROLLUP.exempt_leaves_denominator -/
theorem an_exempt_clause_is_not_counted (held : List ((String × Option String) × Marks)) :
    ownMarks held { id := "R", complete := true, clauses := [some "a", some "b"],
                    exempt := [some "b"], partialClauses := [], refines := [] }
      = [(marksAt held ("R", some "a")).getD Marks.empty] := by
  simp [ownMarks]

/-- @proves REQ-ROLLUP.partial_capped -/
theorem a_partial_clause_is_never_tested_or_proved (reached : Marks) :
    ownMarks [(("R", some "a"), reached)]
        { id := "R", complete := true, clauses := [some "a"], exempt := [],
          partialClauses := [some "a"], refines := [] }
      = [partialCap reached] ∧
    (partialCap reached).holds ⟨false, false, true, false, false⟩ = false ∧
    (partialCap reached).holds ⟨false, false, false, true, false⟩ = false := by
  refine ⟨?_, ?_, ?_⟩
  · simp [ownMarks, marksAt]
  · simp [partialCap, Marks.holds]
  · simp [partialCap, Marks.holds]

def partHolds (floor p : Marks) : Prop :=
  (floor.annotated = true → p.annotated = true) ∧ (floor.judged = true → p.judged = true) ∧
    (floor.tested = true → (p.tested || p.structural) = true) ∧
    (floor.proved = true → (p.proved || p.structural) = true)

theorem both_parts (floor a b : Marks) (h : partHolds floor (Marks.both a b)) :
    partHolds floor a ∧ partHolds floor b := by
  rcases a with ⟨a1, a2, a3, a4, a5⟩
  rcases b with ⟨b1, b2, b3, b4, b5⟩
  obtain ⟨h1, h2, h3, h4⟩ := h
  simp only [Marks.both] at h1 h2 h3 h4
  refine ⟨⟨?_, ?_, ?_, ?_⟩, ⟨?_, ?_, ?_, ?_⟩⟩ <;> intro hf
  · have := h1 hf; revert this; cases a1 <;> cases b1 <;> simp
  · have := h2 hf; revert this; cases a2 <;> cases b2 <;> simp
  · have := h3 hf; revert this; cases a3 <;> cases a5 <;> cases b3 <;> cases b5 <;> simp
  · have := h4 hf; revert this; cases a4 <;> cases a5 <;> cases b4 <;> cases b5 <;> simp
  · have := h1 hf; revert this; cases a1 <;> cases b1 <;> simp
  · have := h2 hf; revert this; cases a2 <;> cases b2 <;> simp
  · have := h3 hf; revert this; cases a3 <;> cases a5 <;> cases b3 <;> cases b5 <;> simp
  · have := h4 hf; revert this; cases a4 <;> cases a5 <;> cases b4 <;> cases b5 <;> simp

theorem fold_parts (floor : Marks) (rest : List Marks) : ∀ acc : Marks,
    partHolds floor (rest.foldl Marks.both acc) → partHolds floor acc ∧ ∀ p ∈ rest, partHolds floor p := by
  induction rest with
  | nil => intro acc h; exact ⟨h, by simp⟩
  | cons x xs ih =>
    intro acc h
    have ⟨hacc, hxs⟩ := ih (Marks.both acc x) h
    have ⟨ha, hx⟩ := both_parts floor acc x hacc
    refine ⟨ha, ?_⟩
    intro p hp
    cases hp with
    | head => exact hx
    | tail _ hp => exact hxs p hp

theorem holds_part (floor m : Marks) (h : m.holds floor = true) : partHolds floor m := by
  rcases m with ⟨m1, m2, m3, m4, m5⟩
  rcases floor with ⟨f1, f2, f3, f4, f5⟩
  simp only [Marks.holds, Bool.and_eq_true] at h
  obtain ⟨⟨⟨h1, h2⟩, h3⟩, h4⟩ := h
  refine ⟨?_, ?_, ?_, ?_⟩ <;> simp only [] <;> intro hf <;> subst hf
  · revert h1; cases m1 <;> simp
  · revert h2; cases m2 <;> simp
  · revert h3; cases m3 <;> simp
  · revert h4; cases m4 <;> simp

/-- @proves REQ-ROLLUP.every_part_holds -/
theorem the_aggregate_holds_only_what_every_part_holds (parts : List Marks) (floor : Marks)
    (h : (combine parts).holds floor = true) :
    (∀ p ∈ parts, partHolds floor p) ∧
    (parts = [] → (floor.annotated || floor.judged || floor.tested || floor.proved) = false) := by
  cases parts with
  | nil =>
    refine ⟨by simp, fun _ => ?_⟩
    rcases floor with ⟨f1, f2, f3, f4, f5⟩
    revert h
    cases f1 <;> cases f2 <;> cases f3 <;> cases f4 <;> simp [combine, Marks.holds, Marks.empty]
  | cons first rest =>
    refine ⟨?_, by simp⟩
    have whole := holds_part floor _ h
    simp only [combine, List.foldl] at whole
    have ⟨hacc, hrest⟩ := fold_parts floor rest _ whole
    have ⟨_, hfirst⟩ := both_parts floor Marks.neutral first hacc
    intro p hp
    cases hp with
    | head => exact hfirst
    | tail _ hp => exact hrest p hp

theorem growFor_keeps (graph : List Node) (k : Nat) :
    ∀ found : List String, ∀ x ∈ found, x ∈ growFor graph k found := by
  induction k with
  | zero => intro found x hx; exact hx
  | succ _ ih =>
    intro found x hx
    exact ih (grow graph found) x (by simp [grow, hx])

theorem root_reached (graph : List Node) (root : String) : root ∈ reach graph root :=
  growFor_keeps graph _ [root] root (by simp)

theorem dedupLast_sub (l : List Node) : ∀ n ∈ dedupLast l, n ∈ l := by
  induction l with
  | nil => simp [dedupLast]
  | cons m rest ih =>
    intro n hn
    unfold dedupLast at hn
    split at hn
    · exact List.mem_cons_of_mem _ (ih n hn)
    · cases hn with
      | head => exact List.mem_cons_self _ _
      | tail _ hn => exact List.mem_cons_of_mem _ (ih n hn)

theorem dedupLast_keeps_id (l : List Node) : ∀ n ∈ l, ∃ m ∈ dedupLast l, m.id = n.id := by
  induction l with
  | nil => simp
  | cons x rest ih =>
    intro n hn
    unfold dedupLast
    split
    · rename_i dup
      cases hn with
      | head =>
        simp only [List.any_eq_true, beq_iff_eq] at dup
        obtain ⟨m, hm, same⟩ := dup
        obtain ⟨m', hm', same'⟩ := ih m hm
        exact ⟨m', hm', same'.trans same⟩
      | tail _ hn => exact ih n hn
    · cases hn with
      | head => exact ⟨x, List.mem_cons_self _ _, rfl⟩
      | tail _ hn =>
        obtain ⟨m, hm, same⟩ := ih n hn
        exact ⟨m, List.mem_cons_of_mem _ hm, same⟩

/-- @proves REQ-ROLLUP.open_is_lower_bound -/
theorem an_open_root_is_provisional (nodes : List Node)
    (held : List ((String × Option String) × Marks)) (root : String) (floor : Marks)
    (declared : ∃ n ∈ nodes, n.id = root)
    (open_ : ∀ n ∈ nodes, n.id = root → n.complete = false) :
    (rollUp nodes held root floor).covered.exact = false ∧
    Figure.isComplete (rollUp nodes held root floor).covered = false := by
  have exact : (rollUp nodes held root floor).covered.exact = false := by
    obtain ⟨n, hn, hid⟩ := declared
    obtain ⟨m, hm, same⟩ := dedupLast_keeps_id nodes n hn
    have inGraph : m ∈ canonNodes nodes := List.mem_mergeSort.mpr hm
    have incomplete : m.complete = false := open_ m (dedupLast_sub nodes m hm) (same.trans hid)
    have member : m ∈ members (canonNodes nodes) root := by
      simp only [members, List.mem_filter]
      refine ⟨inGraph, ?_⟩
      rw [same.trans hid]
      exact List.elem_eq_true_of_mem (root_reached _ root)
    have notAll : (members (canonNodes nodes) root).all (·.complete) = false := by
      rw [Bool.eq_false_iff]
      intro hall
      rw [List.all_eq_true] at hall
      have := hall m member
      rw [incomplete] at this
      exact Bool.false_ne_true this
    cases f : (canonNodes nodes).length <;>
      simp [rollUp, rollUpTree, RollUp.covered, figureOf, notAll, f]
  refine ⟨exact, ?_⟩
  simp [Figure.isComplete, exact]

theorem dedupe_go (xs : List String) : ∀ acc : List String, acc.Nodup →
    (xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) acc).Nodup ∧
    ∀ y, y ∈ xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) acc ↔
      y ∈ acc ∨ y ∈ xs := by
  induction xs with
  | nil => intro acc h; simp [h]
  | cons x rest ih =>
    intro acc h
    simp only [List.foldl]
    split
    · rename_i has
      have ⟨n, m⟩ := ih acc h
      refine ⟨n, fun y => ?_⟩
      rw [m y]
      have hx : x ∈ acc := List.mem_of_elem_eq_true has
      constructor
      · rintro (a | a)
        · exact Or.inl a
        · exact Or.inr (List.mem_cons_of_mem _ a)
      · rintro (a | a)
        · exact Or.inl a
        · cases a with
          | head => exact Or.inl hx
          | tail _ a => exact Or.inr a
    · rename_i hasnt
      have notIn : x ∉ acc := fun hx => hasnt (List.elem_eq_true_of_mem hx)
      have nd : (acc ++ [x]).Nodup := by
        unfold List.Nodup
        rw [List.pairwise_append]
        refine ⟨h, List.pairwise_singleton _ x, ?_⟩
        intro a ha b hb
        simp only [List.mem_singleton] at hb
        subst hb
        intro e
        subst e
        exact notIn ha
      have ⟨n, m⟩ := ih (acc ++ [x]) nd
      refine ⟨n, fun y => ?_⟩
      rw [m y]
      simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · rintro ((a | a) | a)
        · exact Or.inl a
        · exact Or.inr (Or.inl a)
        · exact Or.inr (Or.inr a)
      · rintro (a | (a | a))
        · exact Or.inl (Or.inl a)
        · exact Or.inl (Or.inr a)
        · exact Or.inr a

theorem dedupe_nodup (xs : List String) : (dedupe xs).Nodup :=
  (dedupe_go xs [] List.nodup_nil).1

theorem mem_dedupe (xs : List String) (y : String) : y ∈ dedupe xs ↔ y ∈ xs := by
  have := (dedupe_go xs [] List.nodup_nil).2 y
  simp only [dedupe] at *
  rw [this]
  simp

theorem nodup_sub_length (l : List String) : ∀ m : List String, l.Nodup → (∀ x ∈ l, x ∈ m) →
    l.length ≤ m.length := by
  induction l with
  | nil => intro m _ _; simp
  | cons x rest ih =>
    intro m nd sub
    have ⟨notIn, ndRest⟩ := List.nodup_cons.mp nd
    have xm : x ∈ m := sub x (List.mem_cons_self _ _)
    have sub' : ∀ y ∈ rest, y ∈ m.erase x := by
      intro y hy
      have ne : y ≠ x := fun e => notIn (e ▸ hy)
      exact (List.mem_erase_of_ne ne).mpr (sub y (List.mem_cons_of_mem _ hy))
    have := ih (m.erase x) ndRest sub'
    rw [List.length_erase_of_mem xm] at this
    have pos : 0 < m.length := List.length_pos_of_mem xm
    simp only [List.length_cons]
    omega

/-- @proves REQ-ROLLUP.untraced_counted -/
theorem an_unclaimed_file_stays_in_the_denominator (scanned claimed : List String)
    (file : String) (inTree : file ∈ scanned) (unclaimed : file ∉ claimed) :
    (fileCoverage scanned claimed).total = (fileCoverage scanned []).total ∧
    (fileCoverage scanned claimed).met < (fileCoverage scanned claimed).total := by
  refine ⟨rfl, ?_⟩
  simp only [fileCoverage]
  have fileIn : file ∈ dedupe scanned := (mem_dedupe scanned file).mpr inTree
  have nd : ((dedupe claimed).filter ((dedupe scanned).contains ·)).Nodup :=
    List.Pairwise.filter _ (dedupe_nodup claimed)
  have sub : ∀ x ∈ (dedupe claimed).filter ((dedupe scanned).contains ·),
      x ∈ (dedupe scanned).erase file := by
    intro x hx
    rw [List.mem_filter] at hx
    have xc : x ∈ claimed := (mem_dedupe claimed x).mp hx.1
    have ne : x ≠ file := fun e => unclaimed (e ▸ xc)
    exact (List.mem_erase_of_ne ne).mpr (List.mem_of_elem_eq_true hx.2)
  have le := nodup_sub_length _ _ nd sub
  rw [List.length_erase_of_mem fileIn] at le
  have pos : 0 < (dedupe scanned).length := List.length_pos_of_mem fileIn
  omega

end TraceLean.Rollup
