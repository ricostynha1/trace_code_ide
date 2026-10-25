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

/-- @proves REQ-ROLLUP.every_part_holds -/
theorem combine_holds_what_every_part_holds :
    combine [] = Marks.empty ∧
    written (combine [⟨true, true, true, true, false⟩, ⟨true, true, true, true, false⟩,
      ⟨true, true, true, false, false⟩]) = "AJT-" ∧
    written (combine [⟨true, true, false, false, false⟩, ⟨true, false, false, true, false⟩]) = "A---" ∧
    written (combine [⟨true, true, true, false, false⟩, ⟨true, true, false, false, true⟩]) = "AJT-" ∧
    written (combine [⟨true, true, false, true, true⟩, ⟨true, true, false, false, true⟩]) = "AJ··" := by
  native_decide

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

/-- @proves REQ-ROLLUP.untraced_counted -/
theorem nothing_scanned_is_not_complete :
    (fileCoverage [] []).isComplete = false := by
  native_decide

/-- @proves REQ-ROLLUP.untraced_counted -/
theorem an_unclaimed_file_is_counted :
    fileCoverage ["a.rs", "b.rs"] ["a.rs"] = { met := 1, total := 2, exact := true } := by
  native_decide

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

/-- @proves REQ-ROLLUP.open_is_lower_bound -/
theorem an_open_roll_up_is_provisional :
    (rollUp [{ id := "R", complete := false, clauses := [some "a"], exempt := [],
               partialClauses := [], refines := [] }]
            [(("R", some "a"), ⟨true, true, true, true, false⟩)] "R" ⟨false, false, true, false, false⟩).covered.exact
      = false ∧
    (rollUp [{ id := "R", complete := true, clauses := [some "a"], exempt := [],
               partialClauses := [], refines := [] }]
            [(("R", some "a"), ⟨true, true, true, true, false⟩)] "R" ⟨false, false, true, false, false⟩).covered.exact
      = true := by
  native_decide

end TraceLean.Rollup
