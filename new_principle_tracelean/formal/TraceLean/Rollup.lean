import TraceLean.Evidence

open Lean (Json ToJson FromJson toJson)

/-!
# Roll-up

Models `REQ-ROLLUP`. Where a wrong number does the most damage, because a
percentage is the one output people quote without reading what produced it.
-/

namespace TraceLean.Rollup

open TraceLean.Evidence

/--
Aggregate over children by taking the minimum.

An average invents a level nothing established; a minimum is always a level
something did.

@models REQ-ROLLUP.min_not_mean
@models ARCH-HONEST.weakest_link
-/
def combine (levels : List Level) : Level :=
  levels.foldl Level.min Level.L4 |> fun result =>
    if levels.isEmpty then Level.L1 else result

/-- Nothing to aggregate is the lowest, not the highest: an empty list is an
absence of evidence, and the top of the ladder is the last thing it should
report.

@proves REQ-ROLLUP.min_not_mean -/
theorem combine_empty : combine [] = Level.L1 := by
  simp [combine]

/-- A single level aggregates to itself. -/
theorem combine_singleton (l : Level) : combine [l] = Level.min Level.L4 l := by
  simp [combine]

/-! ## Rolling a declared graph up

**A decomposition nobody claimed complete is provisional.** If two clauses of
a requirement are written and both are met, "100%" is what has been written
down; an unwritten clause has no evidence, so writing it can only bring the
figure down. The figure is marked, never rendered bare.

**An exemption leaves the denominator; a partial does not.** Exempting a clause
is saying it does not apply, so counting it would permanently cap the figure.
Marking one partial is saying it is half done, so it stays, capped below the
level at which anything ran.

**Each requirement's figures are over its own reachable set.** A set, not a
walk: in a diamond (B and C refine A, D refines both) D is in B's set and in
C's, once in A's, and nothing depends on which parent was visited first.
-/

/-- A percentage is a rendering, so the figure itself is a fraction. Two
languages rounding one number differently would be a divergence about nothing. -/
structure Figure where
  met : Nat
  total : Nat
  /-- The author claimed the clauses exhaust the requirement. -/
  exact : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A requirement whose decomposition is unclaimed can never read as done.

@models REQ-ROLLUP.never_complete_when_open -/
def Figure.isComplete (f : Figure) : Bool := f.exact && f.total > 0 && f.met == f.total

/-- A figure as it is written down: an open one is marked provisional, with the
direction it can move. `≤`, because an unwritten clause can only lower it.

@models ARCH-HONEST.lower_bound_marked -/
def render (f : Figure) : String :=
  let percent := if f.total == 0 then 0 else (f.met * 200 + f.total) / (f.total * 2)
  if f.exact then s!"{percent}%" else s!"≤ {percent}% (provisional)"

/-- An open figure is never written as an exact one.

@proves ARCH-HONEST.lower_bound_marked -/
theorem an_open_figure_is_marked (met total : Nat) :
    render { met := met, total := total, exact := false } =
      s!"≤ {if total == 0 then 0 else (met * 200 + total) / (total * 2)}% (provisional)" := by
  simp [render]

/-- A requirement reduced to what a roll-up depends on. -/
structure Node where
  id : String
  complete : Bool
  clauses : List (Option String)
  exempt : List (Option String)
  /-- Clauses claimed only in part: counted, and capped. -/
  partialClauses : List (Option String)
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Assurance of one node, and of everything under it. -/
inductive RollUp where
  | mk (id : String) (assurance : Level) (covered : Figure) (children : List RollUp)
  deriving Repr, Inhabited

def RollUp.assurance : RollUp → Level
  | .mk _ a _ _ => a

def RollUp.covered : RollUp → Figure
  | .mk _ _ f _ => f

/-- Written by hand because a derived encoding for a recursive type would nest
its own constructor name at every level, and the implementation's is flat. -/
partial def RollUp.toJsonValue : RollUp → Json
  | .mk id assurance covered children =>
    Json.mkObj [("id", Json.str id), ("assurance", toJson assurance),
                ("covered", toJson covered),
                ("children", Json.arr (children.map RollUp.toJsonValue).toArray)]

instance : ToJson RollUp := ⟨RollUp.toJsonValue⟩

def levelAt (levels : List ((String × Option String) × Level)) (key : String × Option String)
    : Option Level :=
  (levels.find? (·.1 == key)).map (·.2)

/-! ### A canonical graph

The order requirements are declared in must not matter, so the graph is put
in one order first: the last declaration of an identifier kept (as a map
keeps it), sorted by identifier. Identifiers compare by code point, which is
the order of their UTF-8 bytes, which is the order the implementation's map
keeps. -/

/-- Lexicographic order on code points. -/
def codesLe : List Nat → List Nat → Bool
  | [], _ => true
  | _ :: _, [] => false
  | a :: xs, b :: ys => decide (a < b) || (decide (a = b) && codesLe xs ys)

def codes (s : String) : List Nat := s.data.map Char.toNat

def idLe (a b : Node) : Bool := codesLe (codes a.id) (codes b.id)

/-- The last declaration of each identifier, in the order of those last ones. -/
def dedupLast : List Node → List Node
  | [] => []
  | n :: rest => if rest.any (fun m => m.id == n.id) then dedupLast rest else n :: dedupLast rest

def canonNodes (nodes : List Node) : List Node :=
  (dedupLast nodes).mergeSort idLe

def canonLevels (levels : List ((String × Option String) × Level))
    : List ((String × Option String) × Level) :=
  levels.foldl
    (fun acc kv => if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
                   else acc ++ [kv]) []

/-! ### The reachable set -/

/-- One step out: every requirement refining something already found. -/
def grow (graph : List Node) (found : List String) : List String :=
  found ++ ((graph.filter (fun n =>
    !(found.contains n.id) && n.refines.any (fun p => found.contains p))).map (·.id))

def growFor (graph : List Node) : Nat → List String → List String
  | 0, found => found
  | k + 1, found => growFor graph k (grow graph found)

/-- The requirements reachable from `root` by "is refined by", root included.
Each step adds at least one identifier of the graph or changes nothing, so as
many steps as the graph has nodes reach the fixpoint, cycles included. -/
def reach (graph : List Node) (root : String) : List String :=
  growFor graph (graph.length + 1) [root]

/-- The members of a requirement's set, each once: the graph is canonical, so
each identifier appears in it once. -/
def members (graph : List Node) (root : String) : List Node :=
  graph.filter (fun n => (reach graph root).contains n.id)

/-- What one requirement contributes on its own: the level of each clause that
counts. Exempt clauses leave; partial ones stay, capped at L2. -/
def ownLevels (levels : List ((String × Option String) × Level)) (n : Node) : List Level :=
  (n.clauses.filter (fun c => !(n.exempt.contains c))).map (fun c =>
    if n.partialClauses.contains c then Level.min ((levelAt levels (n.id, c)).getD Level.L1) Level.L2
    else (levelAt levels (n.id, c)).getD Level.L1)

/-- Every counted clause level over a requirement's reachable set. -/
def reachLevels (graph : List Node) (levels : List ((String × Option String) × Level))
    (root : String) : List Level :=
  (members graph root).bind (ownLevels levels)

def figureOf (graph : List Node) (levels : List ((String × Option String) × Level))
    (floor : Level) (root : String) : Figure :=
  let all := reachLevels graph levels root
  { met := (all.filter (fun l => decide (floor ≤ l))).length,
    total := all.length,
    exact := graph.any (·.id == root) && (members graph root).all (·.complete) }

/-- The tree shown: every requirement under every parent it refines, sorted; a
child already on the path is left out, so a cycle cannot loop. Each node's
figures are over its own set, so where it is shown changes nothing. -/
def rollUpTree (graph : List Node) (levels : List ((String × Option String) × Level))
    (floor : Level) : Nat → List String → String → RollUp
  | 0, _, id => .mk id (combine (reachLevels graph levels id)) (figureOf graph levels floor id) []
  | fuel + 1, path, id =>
    .mk id (combine (reachLevels graph levels id)) (figureOf graph levels floor id)
      ((graph.filter (fun n => n.refines.contains id && !((path ++ [id]).contains n.id))).map
        (fun n => rollUpTree graph levels floor fuel (path ++ [id]) n.id))

/--
Roll a declared graph up.

@models REQ-ROLLUP.open_is_lower_bound
@models REQ-ROLLUP.exempt_leaves_denominator
@models REQ-ROLLUP.partial_capped
@models REQ-ROLLUP.deterministic_order
@models REQ-ROLLUP.counted_once
-/
def rollUp (nodes : List Node) (levels : List ((String × Option String) × Level))
    (root : String) (floor : Level) : RollUp :=
  let graph := canonNodes nodes
  rollUpTree graph (canonLevels levels) floor (graph.length + 1) [] root

/-- An unclaimed decomposition never renders as finished, whatever its figure.

@proves REQ-ROLLUP.never_complete_when_open -/
theorem an_unclaimed_decomposition_is_never_complete (met total : Nat) :
    Figure.isComplete { met := met, total := total, exact := false } = false := by
  simp [Figure.isComplete]

/-- A requirement's assurance is the aggregate of every counted clause over its
reachable set — the set, each member once, whatever path reaches it.

@proves REQ-ROLLUP.counted_once -/
theorem assurance_is_over_the_reachable_set (nodes : List Node)
    (levels : List ((String × Option String) × Level)) (root : String) (floor : Level) :
    (rollUp nodes levels root floor).assurance =
      combine (reachLevels (canonNodes nodes) (canonLevels levels) root) := by
  simp [rollUp, rollUpTree, RollUp.assurance]

/-! ### The order of declaration does not matter -/

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

/-- Distinct identifiers name distinct nodes. -/
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

/-- Two lists sorted by identifier holding the same nodes are one list. -/
theorem sorted_eq (one two : List Node)
    (w : ∀ a b, List.Mem a one → List.Mem b two → idRel a b → idRel b a → a = b)
    (h1 : List.Pairwise idRel one) (h2 : List.Pairwise idRel two) (p : List.Perm one two) :
    one = two :=
  List.Perm.eq_of_sorted w h1 h2 p

/-- Declaring the same requirements in another order gives the same graph.

A graph declaring one identifier twice is a `DuplicateId` fault, and there the
last declaration is the one kept, so the order is part of what was declared. -/
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

/-- The roll-up does not depend on the order requirements are declared in.

@proves REQ-ROLLUP.deterministic_order -/
theorem rollUp_perm (nodes other : List Node) (p : List.Perm nodes other)
    (distinct : List.Pairwise (fun a b => Not (a.id = b.id)) nodes)
    (levels : List ((String × Option String) × Level)) (root : String) (floor : Level) :
    rollUp nodes levels root floor = rollUp other levels root floor := by
  simp only [rollUp, canonNodes_perm nodes other p distinct]

/-! ## Coverage over files

`untraced_counted`. A file nothing claims is in the denominator. The
alternative — counting only annotated files — is the figure that rises when
somebody deletes an annotation, and a coverage number that improves when you do
less work is worse than no number at all.

Exact, because the denominator is not a claim anybody made: it is the set of
files that were scanned, which is known.
-/

def dedupe (xs : List String) : List String :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

/-- How many of the files scanned are claimed by at least one annotation.

@models REQ-ROLLUP.untraced_counted -/
def fileCoverage (scanned claimed : List String) : Figure :=
  let scanned := dedupe scanned
  -- A claim about a file nothing scanned is not coverage of anything, so the
  -- numerator is an intersection rather than a count of claims.
  let claimed := (dedupe claimed).filter (scanned.contains ·)
  { met := claimed.length, total := scanned.length, exact := true }

/-- Scanning nothing covers nothing, and the figure says so rather than
reporting a vacuous hundred per cent.

@proves REQ-ROLLUP.untraced_counted -/
theorem nothing_scanned_is_not_complete :
    (fileCoverage [] []).isComplete = false := by
  native_decide

/-- A file nothing claims lowers the figure, which is the whole point.

@proves REQ-ROLLUP.untraced_counted -/
theorem an_unclaimed_file_is_counted :
    fileCoverage ["a.rs", "b.rs"] ["a.rs"] = { met := 1, total := 2, exact := true } := by
  native_decide

/-- An exempt clause contributes nothing -- neither a level nor a place in the
denominator -- while its sibling is still counted.

@proves REQ-ROLLUP.exempt_leaves_denominator -/
theorem an_exempt_clause_is_not_counted (levels : List ((String × Option String) × Level)) :
    ownLevels levels { id := "R", complete := true, clauses := [some "a", some "b"],
                       exempt := [some "b"], partialClauses := [], refines := [] }
      = [(levelAt levels ("R", some "a")).getD Level.L1] := by
  simp [ownLevels]

/-- A partial clause stays counted, at whatever it reached but never above L2,
so it never meets a floor of L3.

@proves REQ-ROLLUP.partial_capped -/
theorem a_partial_clause_is_capped_at_L2 (reached : Level) :
    ownLevels [(("R", some "a"), reached)]
        { id := "R", complete := true, clauses := [some "a"], exempt := [],
          partialClauses := [some "a"], refines := [] }
      = [Level.min reached Level.L2] ∧
    (Level.min reached Level.L2).toNat ≤ Level.L2.toNat := by
  constructor
  · simp [ownLevels, levelAt]
  · cases reached <;> simp [Level.min, Level.toNat]

/-- A roll-up over a decomposition nobody claimed complete is not exact, so it
renders marked provisional, as a figure that can only fall.

@proves REQ-ROLLUP.open_is_lower_bound -/
theorem an_open_roll_up_is_provisional :
    (rollUp [{ id := "R", complete := false, clauses := [some "a"], exempt := [],
               partialClauses := [], refines := [] }]
            [(("R", some "a"), Level.L4)] "R" Level.L3).covered.exact = false ∧
    (rollUp [{ id := "R", complete := true, clauses := [some "a"], exempt := [],
               partialClauses := [], refines := [] }]
            [(("R", some "a"), Level.L4)] "R" Level.L3).covered.exact = true := by
  native_decide

end TraceLean.Rollup
