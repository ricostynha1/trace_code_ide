import TraceLean.Lines
import TraceLean.Protocol
import TraceLean.Refinement

/-!
# Specifications for reported sets

Three clauses ask for a set to be reported: the ops a project uses twice, the
tests that ran a clause's lines, and the refinements naming nothing. Each
specification says which members the set has, and that it is written each
member once, in ascending order. The order is the one presentational choice in
them: no requirement says it, and it is what makes the answer unique.

The lemmas first: the order on strings is a strict total order, two strictly
ascending lists with the same members are equal, and the project's own sorting
helpers produce such lists.
-/

namespace TraceLean.SpecSorted

open TraceLean.Strength (insertSorted sortedUnique)
open TraceLean.Lines
open TraceLean.Refinement

theorem mem_iff_mem {α : Type} (a : α) (l : List α) : List.Mem a l ↔ Membership.mem l a :=
  Iff.rfl

/-! ## The order on strings -/

theorem char_lt_trans {a b c : Char} (h1 : a < b) (h2 : b < c) : a < c := by
  have e1 : a.toNat < b.toNat := h1
  have e2 : b.toNat < c.toNat := h2
  show a.toNat < c.toNat
  omega

theorem char_not_lt_trans {a b c : Char} (h1 : ¬ a < b) (h2 : ¬ b < c) : ¬ a < c := by
  have e1 : ¬ a.toNat < b.toNat := h1
  have e2 : ¬ b.toNat < c.toNat := h2
  show ¬ a.toNat < c.toNat
  omega

theorem char_antisymm {a b : Char} (h1 : ¬ a < b) (h2 : ¬ b < a) : a = b := by
  have e1 : ¬ a.toNat < b.toNat := h1
  have e2 : ¬ b.toNat < a.toNat := h2
  have e : a.toNat = b.toNat := by
    omega
  exact Char.ext (UInt32.eq_of_val_eq (Fin.ext e))

theorem str_lt_trans {a b c : String} (h1 : a < b) (h2 : b < c) : a < c := by
  apply (String.lt_iff a c).2
  exact List.lt_trans' char_lt_trans char_not_lt_trans ((String.lt_iff a b).1 h1)
    ((String.lt_iff b c).1 h2)

theorem str_antisymm {a b : String} (h1 : ¬ a < b) (h2 : ¬ b < a) : a = b := by
  apply String.ext
  apply List.lt_antisymm' char_antisymm
  · intro h
    exact h1 ((String.lt_iff a b).2 h)
  · intro h
    exact h2 ((String.lt_iff b a).2 h)

theorem str_lt_asymm {a b : String} (h : a < b) : ¬ b < a := by
  intro h2
  exact String.lt_irrefl a (str_lt_trans h h2)

/-- `≤` on strings, as `¬ <`, is transitive. -/
theorem str_le_trans {a b c : String} (h1 : ¬ b < a) (h2 : ¬ c < b) : ¬ c < a := by
  intro hca
  refine (Classical.em (c < b)).elim ?_ ?_
  · intro hcb
    exact h2 hcb
  · intro hcb
    refine (Classical.em (b < c)).elim ?_ ?_
    · intro hbc
      exact h1 (str_lt_trans hbc hca)
    · intro hbc
      have e : c = b := str_antisymm hcb hbc
      rw [e] at hca
      exact h1 hca

/-! ## Strictly ascending lists -/

/-- Two lists strictly ascending under the same strict order, with the same
members, are equal. -/
theorem ascending_unique {α : Type} (lt : α → α → Prop) (irrefl : ∀ a, ¬ lt a a)
    (trans : ∀ a b c, lt a b → lt b c → lt a c) (l1 : List α) :
    ∀ l2, List.Pairwise lt l1 → List.Pairwise lt l2 →
      (∀ x, List.Mem x l1 ↔ List.Mem x l2) → l1 = l2 := by
  induction l1
  case nil =>
    intro l2 hp1 hp2 h
    clear hp1 hp2
    cases l2
    case nil => rfl
    case cons b t =>
      have hm := (h b).2 (List.Mem.head _)
      cases hm
  case cons a t1 ih =>
    intro l2 h1 h2 h
    cases l2
    · have hm := (h a).1 (List.Mem.head _)
      cases hm
    · rename_i b t2
      have p1 := List.pairwise_cons.1 h1
      have p2 := List.pairwise_cons.1 h2
      have hab : a = b := by
        have ha := (h a).1 (List.Mem.head _)
        have hb := (h b).2 (List.Mem.head _)
        cases ha
        · rfl
        · rename_i ma
          cases hb
          · rfl
          · rename_i mb
            exact absurd (trans a b a (p1.1 b mb) (p2.1 a ma)) (irrefl a)
      subst hab
      have ht : t1 = t2 := by
        apply ih t2 p1.2 p2.2
        intro x
        constructor
        · intro hx
          have hx2 := (h x).1 (List.Mem.tail _ hx)
          cases hx2
          · exact absurd (p1.1 a hx) (irrefl a)
          · rename_i m
            exact m
        · intro hx
          have hx2 := (h x).2 (List.Mem.tail _ hx)
          cases hx2
          · exact absurd (p2.1 a hx) (irrefl a)
          · rename_i m
            exact m
      rw [ht]

/-- Each member once, in ascending order. -/
def Ascending (l : List String) : Prop := List.Pairwise (fun a b => a < b) l

theorem ascending_eq (l1 l2 : List String) (h1 : Ascending l1) (h2 : Ascending l2)
    (h : ∀ x, List.Mem x l1 ↔ List.Mem x l2) : l1 = l2 :=
  ascending_unique (fun a b => a < b) String.lt_irrefl
    (fun _ _ _ hab hbc => str_lt_trans hab hbc) l1 l2 h1 h2 h

/-- A list sorted by the decided `≤` with no repeats is strictly ascending. -/
theorem ascending_of_sorted (l : List String)
    (hs : List.Pairwise (fun a b => decide (a ≤ b) = true) l) (hn : l.Nodup) : Ascending l := by
  have hb := List.Pairwise.and hs hn
  apply List.Pairwise.imp _ hb
  intro a b hab
  have hle : ¬ b < a := by
    simpa using hab.1
  refine (Classical.em (a < b)).elim ?_ ?_
  · intro h
    exact h
  · intro h
    exact absurd (str_antisymm h hle) hab.2

/-- Strings sorted by the decided `≤`, as `mergeSort` sorts them. -/
theorem merge_sorted_strings (l : List String) :
    List.Pairwise (fun a b => decide (a ≤ b) = true) (l.mergeSort (fun a b => decide (a ≤ b))) := by
  apply List.mergeSort_sorted
  · intro a b c hab hbc
    have h1 : ¬ b < a := by
      simpa using hab
    have h2 : ¬ c < b := by
      simpa using hbc
    have h3 := str_le_trans h1 h2
    simpa using h3
  · intro a b hab
    have h1 : ¬ a ≤ b := by
      simpa using hab
    have h2 : b < a := Classical.not_not.1 h1
    have h3 := str_lt_asymm h2
    simpa using h3

/-! ## The project's sorted insertion -/

theorem insert_spec (x : String) (l : List String) (h : Ascending l) :
    Ascending (insertSorted (fun a b => decide (a ≤ b)) l x) ∧
      ∀ y, List.Mem y (insertSorted (fun a b => decide (a ≤ b)) l x) ↔ (y = x ∨ List.Mem y l) := by
  induction l
  case nil =>
    simp only [insertSorted]
    constructor
    · exact List.Pairwise.cons (fun _ hm => absurd hm (List.not_mem_nil _)) List.Pairwise.nil
    · intro y
      constructor
      · intro hy
        cases hy
        · exact Or.inl rfl
        · rename_i m
          cases m
      · intro hy
        refine hy.elim ?_ ?_
        · intro e
          rw [e]
          exact List.Mem.head _
        · intro m
          cases m
  case cons z rest ih =>
    have hz := List.pairwise_cons.1 h
    have ihr := ih hz.2
    simp only [insertSorted]
    split
    · rename_i hxz
      have e : x = z := by
        simpa using hxz
      constructor
      · exact h
      · intro y
        constructor
        · intro hy
          exact Or.inr hy
        · intro hy
          refine hy.elim ?_ ?_
          · intro ey
            rw [ey, e]
            exact List.Mem.head _
          · intro m
            exact m
    · rename_i hxz
      have ne : x ≠ z := by
        simpa using hxz
      split
      · rename_i hle
        have nlt : ¬ z < x := by
          simpa using hle
        have lt : x < z := by
          refine (Classical.em (x < z)).elim ?_ ?_
          · intro hl
            exact hl
          · intro hl
            exact absurd (str_antisymm hl nlt) ne
        constructor
        · apply List.Pairwise.cons
          · intro w hw
            have hw2 := List.mem_cons.1 hw
            refine hw2.elim ?_ ?_
            · intro ew
              rw [ew]
              exact lt
            · intro mw
              exact str_lt_trans lt (hz.1 w mw)
          · exact h
        · intro y
          constructor
          · intro hy
            cases hy
            · exact Or.inl rfl
            · rename_i m
              exact Or.inr m
          · intro hy
            refine hy.elim ?_ ?_
            · intro ey
              rw [ey]
              exact List.Mem.head _
            · intro m
              exact List.Mem.tail _ m
      · rename_i hle
        have zx : z < x := by
          have hle2 : ¬ x ≤ z := by
            simpa using hle
          exact Classical.not_not.1 hle2
        constructor
        · apply List.Pairwise.cons
          · intro w hw
            have hw2 := (ihr.2 w).1 hw
            refine hw2.elim ?_ ?_
            · intro ew
              rw [ew]
              exact zx
            · intro mw
              exact hz.1 w mw
          · exact ihr.1
        · intro y
          constructor
          · intro hy
            cases hy
            · exact Or.inr (List.Mem.head _)
            · rename_i m
              have m2 := (ihr.2 y).1 m
              refine m2.elim ?_ ?_
              · intro ey
                exact Or.inl ey
              · intro my
                exact Or.inr (List.Mem.tail _ my)
          · intro hy
            refine hy.elim ?_ ?_
            · intro ey
              exact List.Mem.tail _ ((ihr.2 y).2 (Or.inl ey))
            · intro m
              cases m
              · exact List.Mem.head _
              · rename_i my
                exact List.Mem.tail _ ((ihr.2 y).2 (Or.inr my))

theorem fold_insert_spec (xs : List String) :
    ∀ acc, Ascending acc →
      Ascending (xs.foldl (insertSorted (fun a b => decide (a ≤ b))) acc) ∧
        ∀ y, List.Mem y (xs.foldl (insertSorted (fun a b => decide (a ≤ b))) acc) ↔
          (List.Mem y acc ∨ List.Mem y xs) := by
  induction xs
  case nil =>
    intro acc h
    constructor
    · exact h
    · intro y
      constructor
      · intro hy
        exact Or.inl hy
      · intro hy
        refine hy.elim ?_ ?_
        · intro m
          exact m
        · intro m
          cases m
  case cons x rest ih =>
    intro acc h
    have hi := insert_spec x acc h
    have hr := ih (insertSorted (fun a b => decide (a ≤ b)) acc x) hi.1
    constructor
    · exact hr.1
    · intro y
      rw [List.foldl_cons, hr.2 y, hi.2 y]
      constructor
      · intro hy
        refine hy.elim ?_ ?_
        · intro hy2
          refine hy2.elim ?_ ?_
          · intro ey
            rw [ey]
            exact Or.inr (List.Mem.head _)
          · intro m
            exact Or.inl m
        · intro m
          exact Or.inr (List.Mem.tail _ m)
      · intro hy
        refine hy.elim ?_ ?_
        · intro m
          exact Or.inl (Or.inr m)
        · intro m
          cases m
          · exact Or.inl (Or.inl rfl)
          · rename_i my
            exact Or.inr my

/-- What `sortedUnique` gives over strings: each member once, ascending. -/
theorem sorted_unique_spec (xs : List String) :
    Ascending (sortedUnique (fun a b => decide (a ≤ b)) xs) ∧
      ∀ y, List.Mem y (sortedUnique (fun a b => decide (a ≤ b)) xs) ↔ List.Mem y xs := by
  have h := fold_insert_spec xs [] List.Pairwise.nil
  constructor
  · exact h.1
  · intro y
    unfold sortedUnique
    rw [h.2 y]
    constructor
    · intro hy
      refine hy.elim ?_ ?_
      · intro m
        cases m
      · intro m
        exact m
    · intro hy
      exact Or.inr hy

/-! ## Ops named twice -/

/-- What the walk over the ops has seen so far, as the specification needs it. -/
def SeenSoFar (p seen dupes : List String) : Prop :=
  (∀ o, List.Mem o seen ↔ 0 < p.count o) ∧ (∀ o, List.Mem o dupes ↔ 1 < p.count o) ∧ dupes.Nodup

theorem count_snoc (p : List String) (op o : String) :
    (p ++ [op]).count o = p.count o + (if op = o then 1 else 0) := by
  rw [List.count_append, List.count_singleton]
  simp

theorem seen_step (p seen dupes : List String) (op : String) (h : SeenSoFar p seen dupes) :
    SeenSoFar (p ++ [op])
      (if seen.contains op then seen else seen ++ [op])
      (if seen.contains op then (if dupes.contains op then dupes else dupes ++ [op]) else dupes) := by
  have hs := h.1
  have hd := h.2.1
  have hn := h.2.2
  refine (Classical.em (List.Mem op seen)).elim ?_ ?_
  · intro hm
    have hc : seen.contains op = true := List.elem_iff.2 hm
    have hpos := (hs op).1 hm
    rw [hc, if_pos rfl, if_pos rfl]
    refine (Classical.em (List.Mem op dupes)).elim ?_ ?_
    · intro hdm
      have hdc : dupes.contains op = true := List.elem_iff.2 hdm
      have hgt := (hd op).1 hdm
      rw [hdc, if_pos rfl]
      refine ⟨?_, ?_, hn⟩
      · intro o
        rw [hs o, count_snoc]
        refine (Classical.em (op = o)).elim ?_ ?_
        · intro e
          rw [if_pos e, ← e]
          omega
        · intro e
          rw [if_neg e]
          omega
      · intro o
        rw [hd o, count_snoc]
        refine (Classical.em (op = o)).elim ?_ ?_
        · intro e
          rw [if_pos e, ← e]
          omega
        · intro e
          rw [if_neg e]
          omega
    · intro hdm
      have hdc : dupes.contains op = false := by
        apply Bool.eq_false_iff.2
        intro hc2
        exact hdm (List.elem_iff.1 hc2)
      rw [hdc]
      simp only [Bool.false_eq_true, if_false]
      refine ⟨?_, ?_, ?_⟩
      · intro o
        rw [hs o, count_snoc]
        refine (Classical.em (op = o)).elim ?_ ?_
        · intro e
          rw [if_pos e, ← e]
          omega
        · intro e
          rw [if_neg e]
          omega
      · intro o
        rw [mem_iff_mem, List.mem_append, ← mem_iff_mem, hd o, count_snoc]
        refine (Classical.em (op = o)).elim ?_ ?_
        · intro e
          rw [if_pos e, ← e]
          constructor
          · intro _
            omega
          · intro _
            exact Or.inr (List.mem_singleton.2 rfl)
        · intro e
          rw [if_neg e]
          constructor
          · intro hx
            refine hx.elim ?_ ?_
            · intro hx2
              omega
            · intro hx2
              have e2 := List.mem_singleton.1 hx2
              exact absurd e2.symm e
          · intro hx
            have hx2 : 1 < p.count o := by
              omega
            exact Or.inl hx2
      · apply List.pairwise_append.2
        refine ⟨hn, List.Pairwise.cons (fun _ hm2 => absurd hm2 (List.not_mem_nil _)) List.Pairwise.nil, ?_⟩
        intro a ha b hb
        have e := List.mem_singleton.1 hb
        rw [e]
        intro ea
        rw [ea] at ha
        exact hdm ha
  · intro hm
    have hc : seen.contains op = false := by
      apply Bool.eq_false_iff.2
      intro hc2
      exact hm (List.elem_iff.1 hc2)
    have hz : p.count op = 0 := by
      have hz2 : ¬ 0 < p.count op := by
        intro hz3
        exact hm ((hs op).2 hz3)
      omega
    rw [hc]
    simp only [Bool.false_eq_true, if_false]
    refine ⟨?_, ?_, hn⟩
    · intro o
      rw [mem_iff_mem, List.mem_append, ← mem_iff_mem, hs o, count_snoc]
      refine (Classical.em (op = o)).elim ?_ ?_
      · intro e
        rw [if_pos e, ← e]
        constructor
        · intro _
          omega
        · intro _
          exact Or.inr (List.mem_singleton.2 rfl)
      · intro e
        rw [if_neg e]
        constructor
        · intro hx
          refine hx.elim ?_ ?_
          · intro hx2
            omega
          · intro hx2
            have e2 := List.mem_singleton.1 hx2
            exact absurd e2.symm e
        · intro hx
          have hx2 : 0 < p.count o := by
            omega
          exact Or.inl hx2
    · intro o
      rw [hd o, count_snoc]
      refine (Classical.em (op = o)).elim ?_ ?_
      · intro e
        rw [if_pos e, ← e, hz]
        omega
      · intro e
        rw [if_neg e]
        omega

theorem seen_fold (f : List String × List String → String → List String × List String)
    (hf : ∀ acc op, f acc op =
      (if acc.1.contains op then acc.1 else acc.1 ++ [op],
        if acc.1.contains op then (if acc.2.contains op then acc.2 else acc.2 ++ [op]) else acc.2))
    (xs : List String) : ∀ p seen dupes, SeenSoFar p seen dupes →
      SeenSoFar (p ++ xs) (xs.foldl f (seen, dupes)).1 (xs.foldl f (seen, dupes)).2 := by
  induction xs
  case nil =>
    intro p seen dupes h
    simpa using h
  case cons op rest ih =>
    intro p seen dupes h
    rw [List.foldl_cons, hf]
    have h2 := ih (p ++ [op]) _ _ (seen_step p seen dupes op h)
    simpa using h2

theorem colliding_result (f : List String × List String → String → List String × List String)
    (hf : ∀ acc op, f acc op =
      (if acc.1.contains op then acc.1 else acc.1 ++ [op],
        if acc.1.contains op then (if acc.2.contains op then acc.2 else acc.2 ++ [op]) else acc.2))
    (ops : List String) :
    Ascending ((ops.foldl f ([], [])).2.mergeSort (fun a b => decide (a ≤ b))) ∧
      ∀ op, List.Mem op ((ops.foldl f ([], [])).2.mergeSort (fun a b => decide (a ≤ b))) ↔
        1 < ops.count op := by
  have base : SeenSoFar [] [] [] := by
    refine ⟨?_, ?_, List.Pairwise.nil⟩
    · intro o
      constructor
      · intro hm
        cases hm
      · intro hc
        simp at hc
    · intro o
      constructor
      · intro hm
        cases hm
      · intro hc
        simp at hc
  have inv := seen_fold f hf ops [] [] [] base
  have hperm := List.mergeSort_perm (fun a b => decide (a ≤ b)) (ops.foldl f ([], [])).2
  constructor
  · apply ascending_of_sorted
    · exact merge_sorted_strings _
    · exact (List.Perm.nodup_iff hperm).2 inv.2.2
  · intro op
    rw [mem_iff_mem, List.Perm.mem_iff hperm, ← mem_iff_mem, inv.2.1 op]
    simp

/-- The ops that more than one binding of a project names: each such op once,
in ascending order. An op named once is unique, and is not among them.

@specifies REQ-DRT-PROTO.ops_unique -/
def CollidingOps (ops : List String) (y : List String) : Prop :=
  Ascending y ∧ ∀ op, List.Mem op y ↔ 1 < ops.count op

/-- @pins REQ-DRT-PROTO.ops_unique -/
theorem ops_unique_pinned :
    (∀ x1, CollidingOps x1 (TraceLean.Protocol.duplicateOps x1)) ∧
    (∀ x1 y1 y2, CollidingOps x1 y1 → CollidingOps x1 y2 → y1 = y2) := by
  constructor
  · intro x1
    unfold CollidingOps TraceLean.Protocol.duplicateOps
    apply colliding_result
    intro acc op
    cases acc
    rename_i seen dupes
    refine (Bool.eq_false_or_eq_true (seen.contains op)).elim ?_ ?_
    · intro hc
      simp only [hc]
      rfl
    · intro hc
      simp only [hc]
      rfl
  · intro x1 y1 y2 h1 h2
    apply ascending_eq y1 y2 h1.1 h2.1
    intro x
    rw [h1.2 x, h2.2 x]

/-! ## A clause's line coverage -/

/-- How much of the lines from `start` to `stop` ran: how many executable lines
lie in the span, how many of those ran at all, and the tests that ran any of
them, each named once, in ascending order.

@specifies REQ-LINECOV.clause_summary -/
def ClauseSummary (lines : List LineHits) (start stop : Nat) (r : Reach) : Prop :=
  r.all = lines.countP (fun l => decide (start ≤ l.line ∧ l.line ≤ stop)) ∧
  r.run = lines.countP (fun l => decide (start ≤ l.line ∧ l.line ≤ stop ∧ 0 < l.hits)) ∧
  Ascending r.tests ∧
  ∀ t, List.Mem t r.tests ↔
    ∃ l, List.Mem l lines ∧ start ≤ l.line ∧ l.line ≤ stop ∧ ∃ n, List.Mem (t, n) l.tests

/-- @pins REQ-LINECOV.clause_summary -/
theorem clause_summary_pinned :
    (∀ x1 x2 x3, ClauseSummary x1 x2 x3 (spanCoverage x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, ClauseSummary x1 x2 x3 y1 → ClauseSummary x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3
    unfold ClauseSummary spanCoverage
    refine ⟨?_, ?_, ?_, ?_⟩
    · simp [List.countP_eq_length_filter]
    · simp [List.countP_eq_length_filter, List.filter_filter, Bool.and_comm, Bool.and_left_comm,
        Bool.and_assoc]
    · exact (sorted_unique_spec _).1
    · intro t
      rw [(sorted_unique_spec _).2 t]
      simp only [mem_iff_mem]
      simp [and_assoc]
  · intro x1 x2 x3 y1 y2 h1 h2
    have e1 : y1.all = y2.all := h1.1.trans h2.1.symm
    have e2 : y1.run = y2.run := h1.2.1.trans h2.2.1.symm
    have e3 : y1.tests = y2.tests := by
      apply ascending_eq y1.tests y2.tests h1.2.2.1 h2.2.2.1
      intro t
      rw [h1.2.2.2 t, h2.2.2.2 t]
    cases y1
    cases y2
    simp only [Reach.mk.injEq]
    exact ⟨e2, e1, e3⟩

/-! ## Refinements naming nothing -/

/-- `a` comes strictly before `b`: by the first member, then by the second. -/
def PairLt (a b : String × String) : Prop := a.1 < b.1 ∨ (a.1 = b.1 ∧ a.2 < b.2)

theorem pair_lt_irrefl (a : String × String) : ¬ PairLt a a := by
  intro h
  refine h.elim ?_ ?_
  · intro h1
    exact String.lt_irrefl _ h1
  · intro h1
    exact String.lt_irrefl _ h1.2

theorem pair_lt_trans (a b c : String × String) (h1 : PairLt a b) (h2 : PairLt b c) :
    PairLt a c := by
  refine h1.elim ?_ ?_
  · intro hab
    refine h2.elim ?_ ?_
    · intro hbc
      exact Or.inl (str_lt_trans hab hbc)
    · intro hbc
      have h3 : a.1 < c.1 := by
        rw [← hbc.1]
        exact hab
      exact Or.inl h3
  · intro hab
    refine h2.elim ?_ ?_
    · intro hbc
      have h3 : a.1 < c.1 := by
        rw [hab.1]
        exact hbc
      exact Or.inl h3
    · intro hbc
      exact Or.inr ⟨hab.1.trans hbc.1, str_lt_trans hab.2 hbc.2⟩

/-- The comparator `danglingIn` sorts with. -/
def pairCmp (a b : String × String) : Bool :=
  if a.1 != b.1 then decide (a.1 ≤ b.1) else decide (a.2 ≤ b.2)

/-- That comparator, read as a proposition. -/
theorem pair_cmp_iff (a b : String × String) :
    pairCmp a b = true ↔ (a.1 < b.1 ∨ (a.1 = b.1 ∧ ¬ b.2 < a.2)) := by
  unfold pairCmp
  refine (Classical.em (a.1 = b.1)).elim ?_ ?_
  · intro e
    have hb : (a.1 != b.1) = false := by
      simp [e]
    rw [hb]
    simp only [Bool.false_eq_true, if_false, decide_eq_true_eq]
    constructor
    · intro h
      exact Or.inr ⟨e, h⟩
    · intro h
      refine h.elim ?_ ?_
      · intro h1
        rw [e] at h1
        exact absurd h1 (String.lt_irrefl _)
      · intro h1
        exact h1.2
  · intro e
    have hb : (a.1 != b.1) = true := by
      simp [e]
    rw [hb]
    simp only [if_true, decide_eq_true_eq]
    constructor
    · intro h
      have h2 : ¬ b.1 < a.1 := h
      refine (Classical.em (a.1 < b.1)).elim ?_ ?_
      · intro h3
        exact Or.inl h3
      · intro h3
        exact absurd (str_antisymm h3 h2) e
    · intro h
      refine h.elim ?_ ?_
      · intro h1
        exact str_lt_asymm h1
      · intro h1
        exact absurd h1.1 e

theorem pair_cmp_trans (a b c : String × String) (hab : pairCmp a b = true)
    (hbc : pairCmp b c = true) : pairCmp a c = true := by
  have k1 := (pair_cmp_iff a b).1 hab
  have k2 := (pair_cmp_iff b c).1 hbc
  apply (pair_cmp_iff a c).2
  refine k1.elim ?_ ?_
  · intro h1
    refine k2.elim ?_ ?_
    · intro h2
      exact Or.inl (str_lt_trans h1 h2)
    · intro h2
      have h3 : a.1 < c.1 := by
        rw [← h2.1]
        exact h1
      exact Or.inl h3
  · intro h1
    refine k2.elim ?_ ?_
    · intro h2
      have h3 : a.1 < c.1 := by
        rw [h1.1]
        exact h2
      exact Or.inl h3
    · intro h2
      exact Or.inr ⟨h1.1.trans h2.1, str_le_trans h1.2 h2.2⟩

theorem pair_cmp_total (a b : String × String) (hab : (!pairCmp a b) = true) :
    pairCmp b a = true := by
  have hn : ¬ (a.1 < b.1 ∨ (a.1 = b.1 ∧ ¬ b.2 < a.2)) := by
    intro h
    have h2 := (pair_cmp_iff a b).2 h
    rw [h2] at hab
    cases hab
  apply (pair_cmp_iff b a).2
  refine (Classical.em (b.1 < a.1)).elim ?_ ?_
  · intro h
    exact Or.inl h
  · intro h
    have h3 : ¬ a.1 < b.1 := by
      intro h4
      exact hn (Or.inl h4)
    have e : b.1 = a.1 := str_antisymm h h3
    refine Or.inr ⟨e, ?_⟩
    intro h5
    apply hn
    exact Or.inr ⟨e.symm, str_lt_asymm h5⟩

theorem pair_cmp_sorted (l : List (String × String)) :
    List.Pairwise (fun a b => pairCmp a b = true) (l.mergeSort pairCmp) :=
  List.mergeSort_sorted pair_cmp_trans pair_cmp_total l

theorem pair_lt_of_sorted (l : List (String × String))
    (hs : List.Pairwise (fun a b => pairCmp a b = true) l)
    (hn : l.Nodup) : List.Pairwise PairLt l := by
  have hb := List.Pairwise.and hs hn
  apply List.Pairwise.imp _ hb
  intro a b hab
  have h1 := (pair_cmp_iff a b).1 hab.1
  refine h1.elim ?_ ?_
  · intro h
    exact Or.inl h
  · intro h
    refine Or.inr ⟨h.1, ?_⟩
    refine (Classical.em (a.2 < b.2)).elim ?_ ?_
    · intro h2
      exact h2
    · intro h2
      have e2 := str_antisymm h2 h.2
      apply absurd _ hab.2
      exact Prod.ext h.1 e2

theorem dedup_fold {α : Type} [BEq α] [LawfulBEq α] (d : List α → α → List α)
    (hd : ∀ acc p, d acc p = if acc.contains p then acc else acc ++ [p]) (l : List α) :
    ∀ acc, acc.Nodup → (l.foldl d acc).Nodup ∧
      ∀ x, List.Mem x (l.foldl d acc) ↔ (List.Mem x acc ∨ List.Mem x l) := by
  induction l
  case nil =>
    intro acc h
    constructor
    · exact h
    · intro x
      constructor
      · intro hx
        exact Or.inl hx
      · intro hx
        refine hx.elim ?_ ?_
        · intro m
          exact m
        · intro m
          cases m
  case cons p rest ih =>
    intro acc h
    have step : (d acc p).Nodup ∧ ∀ x, List.Mem x (d acc p) ↔ (List.Mem x acc ∨ x = p) := by
      rw [hd]
      refine (Classical.em (List.Mem p acc)).elim ?_ ?_
      · intro hm
        have hc : acc.contains p = true := List.elem_iff.2 hm
        rw [hc, if_pos rfl]
        refine ⟨h, ?_⟩
        intro x
        constructor
        · intro hx
          exact Or.inl hx
        · intro hx
          refine hx.elim ?_ ?_
          · intro m
            exact m
          · intro e
            rw [e]
            exact hm
      · intro hm
        have hc : acc.contains p = false := by
          apply Bool.eq_false_iff.2
          intro hc2
          exact hm (List.elem_iff.1 hc2)
        rw [hc]
        simp only [Bool.false_eq_true, if_false]
        constructor
        · apply List.pairwise_append.2
          refine ⟨h, List.Pairwise.cons (fun _ hm2 => absurd hm2 (List.not_mem_nil _)) List.Pairwise.nil, ?_⟩
          intro a ha b hb
          have e := List.mem_singleton.1 hb
          rw [e]
          intro ea
          rw [ea] at ha
          exact hm ha
        · intro x
          rw [mem_iff_mem, List.mem_append, List.mem_singleton]
          rfl
    have hr := ih (d acc p) step.1
    constructor
    · exact hr.1
    · intro x
      rw [List.foldl_cons, hr.2 x, step.2 x]
      constructor
      · intro hx
        refine hx.elim ?_ ?_
        · intro hx2
          refine hx2.elim ?_ ?_
          · intro m
            exact Or.inl m
          · intro e
            rw [e]
            exact Or.inr (List.Mem.head _)
        · intro m
          exact Or.inr (List.Mem.tail _ m)
      · intro hx
        refine hx.elim ?_ ?_
        · intro m
          exact Or.inl (Or.inl m)
        · intro m
          cases m
          · exact Or.inl (Or.inr rfl)
          · rename_i m2
            exact Or.inr m2

theorem append_fold {α β : Type} (f : List β → α → List β) (h : α → List β)
    (hf : ∀ acc e, f acc e = acc ++ h e) (l : List α) :
    ∀ acc, l.foldl f acc = acc ++ l.bind h := by
  induction l
  case nil =>
    intro acc
    simp
  case cons e rest ih =>
    intro acc
    rw [List.foldl_cons, ih, hf]
    simp

theorem any_false_iff (g : Graph) (q : String) :
    (g.any (fun x => x.1 == q)) = false ↔ ∀ e, List.Mem e g → e.1 ≠ q := by
  rw [Bool.eq_false_iff]
  constructor
  · intro h e he eq
    apply h
    have hb : (e.1 == q) = true := by
      simp [eq]
    exact List.any_eq_true.2 ⟨e, he, hb⟩
  · intro h ha
    refine (List.any_eq_true.1 ha).elim ?_
    intro e he
    have eq : e.1 = q := by
      simpa using he.2
    exact h e he.1 eq

/-- The refinements that name an identifier no requirement declares, as
(requirement, missing parent) pairs: each such pair once, in ascending order.

@specifies REQ-REQDOC.refines_resolves -/
def UnresolvedRefines (g : Graph) (y : List (String × String)) : Prop :=
  List.Pairwise PairLt y ∧
  ∀ child parent, List.Mem (child, parent) y ↔
    ((∃ parents, List.Mem (child, parents) g ∧ List.Mem parent parents) ∧
      ∀ e, List.Mem e g → e.1 ≠ parent)

theorem unresolved_result (g : Graph)
    (f : List (String × String) → String × List String → List (String × String))
    (hf : ∀ acc e, f acc e = acc ++ (e.2.filter (fun p => !hasNode g p)).map (fun p => (e.1, p)))
    (d : List (String × String) → String × String → List (String × String))
    (hd : ∀ acc p, d acc p = if acc.contains p then acc else acc ++ [p]) :
    UnresolvedRefines g (((g.foldl f []).foldl d []).mergeSort pairCmp) := by
  have hpairs := append_fold f (fun e => (e.2.filter (fun p => !hasNode g p)).map (fun p => (e.1, p)))
    hf g []
  have hdd := dedup_fold d hd (g.foldl f []) [] List.Pairwise.nil
  have hperm := List.mergeSort_perm pairCmp ((g.foldl f []).foldl d [])
  constructor
  · apply pair_lt_of_sorted
    · exact pair_cmp_sorted _
    · exact (List.Perm.nodup_iff hperm).2 hdd.1
  · intro child parent
    rw [mem_iff_mem, List.Perm.mem_iff hperm, ← mem_iff_mem, hdd.2, hpairs]
    simp only [mem_iff_mem]
    simp [hasNode]
    constructor
    · intro h
      refine h.elim ?_
      intro a h
      refine h.elim ?_
      intro b h
      have e := h.2.2
      rw [e] at h
      refine ⟨⟨b, h.1, h.2.1.1⟩, ?_⟩
      intro a2 b2 hm
      exact (any_false_iff g parent).1 h.2.1.2 (a2, b2) hm
    · intro h
      refine h.1.elim ?_
      intro b hb
      have hn : (g.any (fun x => x.1 == parent)) = false := by
        apply (any_false_iff g parent).2
        intro e he
        exact h.2 e.1 e.2 he
      exact ⟨child, b, hb.1, ⟨hb.2, hn⟩, rfl⟩

/-- @pins REQ-REQDOC.refines_resolves -/
theorem refines_resolves_pinned :
    (∀ x1, UnresolvedRefines x1 (danglingIn x1)) ∧
    (∀ x1 y1 y2, UnresolvedRefines x1 y1 → UnresolvedRefines x1 y2 → y1 = y2) := by
  constructor
  · intro x1
    unfold danglingIn
    apply unresolved_result
    · intro acc e
      rfl
    · intro acc p
      rfl
  · intro x1 y1 y2 h1 h2
    apply ascending_unique PairLt pair_lt_irrefl pair_lt_trans y1 y2 h1.1 h2.1
    intro x
    have k1 := h1.2 x.1 x.2
    have k2 := h2.2 x.1 x.2
    exact k1.trans k2.symm

end TraceLean.SpecSorted
