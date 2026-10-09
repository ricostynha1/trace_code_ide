import TraceLean.HistoryView

/-!
# What a filtered history view must show

A specification for `REQ-UNDO.filtered_view`, written from the clause rather
than from `shownPoints`: the nodes the filter keeps, in order, each under its
nearest kept ancestor, and the one nearest the workspace's position marked.

"Nearest" is stated by walking parent links any number of steps, with no bound:
the model bounds its walk by the number of points, and the proof below is what
says that bound loses nothing.

The proofs are written in the subset of Lean the annotation grammar reads
(ADR-0008): `cases` on `Classical.em`, `Bool.eq_false_or_eq_true` or
`none_or_some` rather than `by_cases h :` or `cases h : e`, and `case` arms
rather than `with |`.
-/

namespace TraceLean.SpecHistory

open TraceLean.HistoryView

/-- The point recorded for a node: the first one naming it. -/
def pointOf (ps : List Point) (n : Nat) : Option Point :=
  ps.find? (fun p => p.node == n)

/-- Whether the view keeps a point: every node, the nodes touching one file, or
the nodes at which the work was saved. -/
def Kept (f : Filter) (file : String) (p : Point) : Prop :=
  match f with
  | .all => True
  | .file => p.file = some file
  | .saved => p.saved = true

/-- A node the view keeps. A node no point records is not one. -/
def KeptNode (ps : List Point) (f : Filter) (file : String) (n : Nat) : Prop :=
  ∃ p, pointOf ps n = some p ∧ Kept f file p

/-- One step up: the node a node was made after. -/
def parentOf (ps : List Point) : Option Nat → Option Nat
  | none => none
  | some n =>
    match pointOf ps n with
    | none => none
    | some p => p.parent

/-- `k` steps up from `start`. -/
def up (ps : List Point) : Nat → Option Nat → Option Nat
  | 0, start => start
  | k + 1, start => up ps k (parentOf ps start)

/-- `r` is the nearest kept node from `start` up, `start` included: the first
kept node the walk up meets, or nothing when it meets none. -/
def NearestKept (ps : List Point) (f : Filter) (file : String) (start r : Option Nat) : Prop :=
  (∃ k n, up ps k start = some n ∧ KeptNode ps f file n ∧
    (∀ j m, j < k → up ps j start = some m → ¬ KeptNode ps f file m) ∧ r = some n) ∨
  ((∀ k n, up ps k start = some n → ¬ KeptNode ps f file n) ∧ r = none)

/-- `ys` shows the points of `rest` the view keeps, in order: each as it is,
under its nearest kept ancestor, and marked exactly when it is the kept node
nearest the workspace's position `pos`. -/
def ViewOf (ps : List Point) (f : Filter) (file : String) (pos : Option Nat) :
    List Point → List Point → Prop
  | [], ys => ys = []
  | p :: rest, ys =>
    (Kept f file p ∧ ∃ q qs, ys = q :: qs ∧
      q.node = p.node ∧ q.said = p.said ∧ q.file = p.file ∧ q.saved = p.saved ∧
      NearestKept ps f file p.parent q.parent ∧
      (q.here = true ↔ NearestKept ps f file pos (some p.node)) ∧
      ViewOf ps f file pos rest qs) ∨
    (¬ Kept f file p ∧ ViewOf ps f file pos rest ys)

/-- The node the workspace is at: the first point marked as here, or none when
the workspace is at the tree as opened. -/
def position (ps : List Point) : Option Nat :=
  (ps.find? (fun p => p.here)).map (fun p => p.node)

/-- `y` is the history shown through `filter`: the kept nodes, in order, each
under its nearest shown ancestor, and the one nearest the workspace's position
marked.

@specifies REQ-UNDO.filtered_view -/
def FilteredView (points : List Point) (filter : Filter) (file : String) (y : List Point) : Prop :=
  ViewOf points filter file (position points) points y

/-! ## The walk up -/

/-- An option is empty or holds something: the split `cases h : o` would give,
with the equation kept. -/
theorem none_or_some {α : Type} (o : Option α) : o = none ∨ ∃ x, o = some x := by
  cases o
  case none => exact Or.inl rfl
  case some x => exact Or.inr ⟨x, rfl⟩

theorem keep_iff (f : Filter) (file : String) (p : Point) :
    keep f file p = true ↔ Kept f file p := by
  cases f
  · simp [keep, Kept]
  · simp [keep, Kept]
  · simp [keep, Kept]

theorem not_kept (f : Filter) (file : String) (p : Point) (h : keep f file p = false) :
    ¬ Kept f file p := by
  intro k
  have t := (keep_iff f file p).2 k
  rw [h] at t
  cases t

theorem up_none (ps : List Point) (k : Nat) : up ps k none = none := by
  induction k
  case zero => rfl
  case succ k ih => simp [up, parentOf, ih]

theorem up_add (ps : List Point) (a b : Nat) (s : Option Nat) :
    up ps (a + b) s = up ps a (up ps b s) := by
  induction b generalizing s
  case zero => rfl
  case succ b ih =>
    show up ps (a + b) (parentOf ps s) = up ps a (up ps b (parentOf ps s))
    exact ih (parentOf ps s)

theorem nearest_unique (ps : List Point) (f : Filter) (file : String) (s r1 r2 : Option Nat)
    (h1 : NearestKept ps f file s r1) (h2 : NearestKept ps f file s r2) : r1 = r2 := by
  cases h1
  case inl found1 =>
    rcases found1 with ⟨k1, n1, u1, c1, m1, e1⟩
    cases h2
    case inl found2 =>
      rcases found2 with ⟨k2, n2, u2, c2, m2, e2⟩
      rw [e1, e2]
      cases Nat.lt_trichotomy k1 k2
      case inl lt => exact absurd c1 (m2 k1 n1 lt u1)
      case inr rest =>
        cases rest
        case inl eq =>
          rw [eq, u2] at u1
          cases u1
          rfl
        case inr gt => exact absurd c2 (m1 k2 n2 gt u2)
    case inr missed2 =>
      rcases missed2 with ⟨a2, _⟩
      exact absurd c1 (a2 k1 n1 u1)
  case inr missed1 =>
    rcases missed1 with ⟨a1, e1⟩
    cases h2
    case inl found2 =>
      rcases found2 with ⟨k2, n2, u2, c2, _, _⟩
      exact absurd c2 (a1 k2 n2 u2)
    case inr missed2 =>
      rcases missed2 with ⟨_, e2⟩
      rw [e1, e2]

/-! The model's walk one step at a time, so the proofs below rewrite with an
equation rather than simplifying a hypothesis. -/

theorem walk_unrecorded (ps : List Point) (f : Filter) (file : String) (fuel s : Nat)
    (g : ps.find? (fun p => p.node == s) = none) :
    keptAncestor ps f file (fuel + 1) (some s) = none := by
  simp [keptAncestor, g]

theorem walk_kept (ps : List Point) (f : Filter) (file : String) (fuel s : Nat) (p : Point)
    (g : ps.find? (fun p => p.node == s) = some p) (kp : keep f file p = true) :
    keptAncestor ps f file (fuel + 1) (some s) = some s := by
  simp [keptAncestor, g, kp]

theorem walk_skipped (ps : List Point) (f : Filter) (file : String) (fuel s : Nat) (p : Point)
    (g : ps.find? (fun p => p.node == s) = some p) (kp : keep f file p = false) :
    keptAncestor ps f file (fuel + 1) (some s) = keptAncestor ps f file fuel p.parent := by
  simp [keptAncestor, g, kp]

/-- What the model's walk finds is the first kept node up. -/
theorem kept_found (ps : List Point) (f : Filter) (file : String) :
    ∀ (fuel : Nat) (s : Option Nat) (n : Nat),
      keptAncestor ps f file fuel s = some n →
      ∃ k, up ps k s = some n ∧ KeptNode ps f file n ∧
        (∀ j m, j < k → up ps j s = some m → ¬ KeptNode ps f file m) := by
  intro fuel
  induction fuel
  case zero =>
    intro s n h
    exact absurd h (by simp [keptAncestor])
  case succ fuel ih =>
    intro s n h
    cases s
    case none => exact absurd h (by simp [keptAncestor])
    case some s =>
      cases none_or_some (ps.find? (fun p => p.node == s))
      case inl g =>
        rw [walk_unrecorded ps f file fuel s g] at h
        cases h
      case inr found =>
        rcases found with ⟨p, g⟩
        cases Bool.eq_false_or_eq_true (keep f file p)
        case inl kp =>
          rw [walk_kept ps f file fuel s p g kp] at h
          cases h
          refine ⟨0, rfl, ⟨p, g, (keep_iff f file p).1 kp⟩, ?_⟩
          intro j m lt
          omega
        case inr kp =>
          rw [walk_skipped ps f file fuel s p g kp] at h
          rcases ih p.parent n h with ⟨k, u, c, m⟩
          have step : parentOf ps (some s) = p.parent := by
            simp [parentOf, pointOf, g]
          refine ⟨k + 1, ?_, c, ?_⟩
          · show up ps k (parentOf ps (some s)) = some n
            rw [step]
            exact u
          · intro j m2 lt uj
            cases j
            case zero =>
              intro kn
              rcases kn with ⟨p2, g2, k2⟩
              have e : some s = some m2 := uj
              cases e
              rw [pointOf, g] at g2
              cases g2
              exact not_kept f file p kp k2
            case succ j =>
              have uj2 : up ps j p.parent = some m2 := by
                rw [← step]
                exact uj
              exact m j m2 (by omega) uj2

/-- When the model's walk finds nothing, nothing kept is met within its fuel. -/
theorem kept_missed (ps : List Point) (f : Filter) (file : String) :
    ∀ (fuel : Nat) (s : Option Nat),
      keptAncestor ps f file fuel s = none →
      ∀ k n, k < fuel → up ps k s = some n → ¬ KeptNode ps f file n := by
  intro fuel
  induction fuel
  case zero =>
    intro s _ k n lt
    omega
  case succ fuel ih =>
    intro s h k n lt u
    cases s
    case none =>
      rw [up_none] at u
      cases u
    case some s =>
      cases none_or_some (ps.find? (fun p => p.node == s))
      case inl g =>
        have step : parentOf ps (some s) = none := by
          simp [parentOf, pointOf, g]
        cases k
        case zero =>
          intro kn
          rcases kn with ⟨p2, g2, _⟩
          have e : some s = some n := u
          cases e
          rw [pointOf, g] at g2
          cases g2
        case succ k =>
          have u2 : up ps k (parentOf ps (some s)) = some n := u
          rw [step, up_none] at u2
          cases u2
      case inr found =>
        rcases found with ⟨p, g⟩
        cases Bool.eq_false_or_eq_true (keep f file p)
        case inl kp =>
          rw [walk_kept ps f file fuel s p g kp] at h
          cases h
        case inr kp =>
          rw [walk_skipped ps f file fuel s p g kp] at h
          have step : parentOf ps (some s) = p.parent := by
            simp [parentOf, pointOf, g]
          cases k
          case zero =>
            intro kn
            rcases kn with ⟨p2, g2, k2⟩
            have e : some s = some n := u
            cases e
            rw [pointOf, g] at g2
            cases g2
            exact not_kept f file p kp k2
          case succ k =>
            have u2 : up ps k (parentOf ps (some s)) = some n := u
            rw [step] at u2
            exact ih p.parent h k n (by omega) u2

/-! ## The bound loses nothing

A walk that meets a kept node meets one within as many steps as there are
points: before that, two steps would land on the same node, and the walk from
there repeats. -/

/-- `k + 1` values, pairwise different, all found in `l`, need `l` to have at
least `k + 1` entries. -/
theorem distinct_fit (v : Nat → Option Nat) :
    ∀ (k : Nat) (l : List (Option Nat)),
      (∀ i j, i < j → j ≤ k → v i ≠ v j) →
      (∀ i, i ≤ k → List.Mem (v i) l) →
      k + 1 ≤ l.length := by
  intro k
  induction k
  case zero =>
    intro l _ inl
    have := List.length_pos_of_mem (inl 0 (Nat.le_refl 0))
    omega
  case succ k ih =>
    intro l dist inl
    have top : List.Mem (v (k + 1)) l := inl (k + 1) (Nat.le_refl _)
    have shorter := List.length_erase_of_mem top
    have pos := List.length_pos_of_mem top
    have rest : k + 1 ≤ (l.erase (v (k + 1))).length := by
      apply ih
      · intro i j lt le
        exact dist i j lt (by omega)
      · intro i le
        have ne : v i ≠ v (k + 1) := dist i (k + 1) (by omega) (Nat.le_refl _)
        exact (List.mem_erase_of_ne ne).2 (inl i (by omega))
    omega

/-- A node the walk reaches before its last step is a recorded point. -/
theorem reached_is_recorded (ps : List Point) (s : Option Nat) (k : Nat) (n : Nat)
    (u : up ps k s = some n) :
    ∀ i, i < k → ∃ m p, up ps i s = some m ∧ pointOf ps m = some p := by
  intro i lt
  have chain : up ps k s = up ps (k - (i + 1)) (up ps 1 (up ps i s)) := by
    rw [← up_add, ← up_add]
    congr 1
    omega
  cases none_or_some (up ps i s)
  case inl g =>
    have gone : up ps k s = none := by
      rw [chain, g]
      simp [up, parentOf, up_none]
    rw [gone] at u
    cases u
  case inr found =>
    rcases found with ⟨m, g⟩
    cases none_or_some (pointOf ps m)
    case inl h =>
      have gone : up ps k s = none := by
        rw [chain, g]
        simp [up, parentOf, h, up_none]
      rw [gone] at u
      cases u
    case inr known =>
      rcases known with ⟨p, h⟩
      exact ⟨m, p, g, h⟩

theorem recorded_is_listed (ps : List Point) (m : Nat) (p : Point)
    (h : pointOf ps m = some p) : List.Mem (some m) (ps.map (fun p => some p.node)) := by
  have found : ps.find? (fun p => p.node == m) = some p := h
  have mem := List.mem_of_find?_eq_some found
  have same := List.find?_some found
  have eq : p.node = m := by simpa using same
  rw [← eq]
  exact List.mem_map_of_mem (fun p => some p.node) mem

theorem kept_within (ps : List Point) (f : Filter) (file : String) (s : Option Nat) :
    ∀ k n, up ps k s = some n → KeptNode ps f file n →
      ∃ k2 n2, k2 < ps.length ∧ up ps k2 s = some n2 ∧ KeptNode ps f file n2 := by
  intro k
  induction k using Nat.strongRecOn
  case ind k ih =>
    intro n u c
    cases Classical.em (k < ps.length)
    case inl small => exact ⟨k, n, small, u, c⟩
    case inr large =>
      cases Classical.em (∃ i j, i < j ∧ j ≤ k ∧ up ps i s = up ps j s)
      case inl rep =>
        rcases rep with ⟨i, j, lt, le, same⟩
        have back : up ps k s = up ps (k - j + i) s := by
          have a : up ps k s = up ps (k - j) (up ps j s) := by
            rw [← up_add]
            congr 1
            omega
          rw [a, ← same, ← up_add]
        rw [back] at u
        exact ih (k - j + i) (by omega) n u c
      case inr rep =>
        have dist : ∀ i j, i < j → j ≤ k → up ps i s ≠ up ps j s := by
          intro i j lt le e
          exact rep ⟨i, j, lt, le, e⟩
        have listed : ∀ i, i ≤ k → List.Mem (up ps i s) (ps.map (fun p => some p.node)) := by
          intro i le
          cases Nat.lt_or_ge i k
          case inl lt =>
            rcases reached_is_recorded ps s k n u i lt with ⟨m, p, g, h⟩
            rw [g]
            exact recorded_is_listed ps m p h
          case inr ge =>
            have e : i = k := by omega
            subst e
            rcases c with ⟨p, h, _⟩
            rw [u]
            exact recorded_is_listed ps n p h
        have fit := distinct_fit (fun i => up ps i s) k _ dist listed
        simp at fit
        omega

/-- The model's walk, with its fuel, answers the unbounded question. -/
theorem kept_ancestor_nearest (ps : List Point) (f : Filter) (file : String) (s : Option Nat) :
    NearestKept ps f file s (keptAncestor ps f file (ps.length + 1) s) := by
  cases none_or_some (keptAncestor ps f file (ps.length + 1) s)
  case inr found =>
    rcases found with ⟨n, g⟩
    rcases kept_found ps f file (ps.length + 1) s n g with ⟨k, u, c, m⟩
    rw [g]
    exact Or.inl ⟨k, n, u, c, m, rfl⟩
  case inl g =>
    rw [g]
    refine Or.inr ⟨?_, rfl⟩
    intro k n u c
    rcases kept_within ps f file s k n u c with ⟨k2, n2, lt, u2, c2⟩
    exact kept_missed ps f file (ps.length + 1) s g k2 n2 (by omega) u2 c2

/-! ## The view -/

theorem view_meets (ps : List Point) (f : Filter) (file : String) (pos : Option Nat) :
    ∀ rest : List Point,
      ViewOf ps f file pos rest
        ((rest.filter (keep f file)).map (fun p =>
          { p with parent := keptAncestor ps f file (ps.length + 1) p.parent,
                   here := some p.node == keptAncestor ps f file (ps.length + 1) pos })) := by
  intro rest
  induction rest
  case nil => rfl
  case cons p rest ih =>
    cases Bool.eq_false_or_eq_true (keep f file p)
    case inl kp =>
      refine Or.inl ⟨(keep_iff f file p).1 kp, ?_⟩
      simp only [List.filter, kp, List.map]
      refine ⟨_, _, rfl, rfl, rfl, rfl, rfl, kept_ancestor_nearest ps f file p.parent, ?_, ih⟩
      have near := kept_ancestor_nearest ps f file pos
      constructor
      · intro h
        have e : some p.node = keptAncestor ps f file (ps.length + 1) pos := by
          simpa using h
        rw [e]
        exact near
      · intro h
        have e := nearest_unique ps f file pos _ _ h near
        simp [e]
    case inr kp =>
      refine Or.inr ⟨not_kept f file p kp, ?_⟩
      have drop : (p :: rest).filter (keep f file) = rest.filter (keep f file) := by
        simp [List.filter, kp]
      rw [drop]
      exact ih

theorem point_ext (a b : Point) (n : a.node = b.node) (pa : a.parent = b.parent)
    (h : a.here = b.here) (s : a.said = b.said) (fl : a.file = b.file)
    (sv : a.saved = b.saved) : a = b := by
  cases a
  cases b
  simp_all

theorem view_unique (ps : List Point) (f : Filter) (file : String) (pos : Option Nat) :
    ∀ rest ys1 ys2, ViewOf ps f file pos rest ys1 → ViewOf ps f file pos rest ys2 → ys1 = ys2 := by
  intro rest
  induction rest
  case nil =>
    intro ys1 ys2 h1 h2
    have a : ys1 = [] := h1
    have b : ys2 = [] := h2
    rw [a, b]
  case cons p rest ih =>
    intro ys1 ys2 h1 h2
    cases h1
    case inl shown1 =>
      rcases shown1 with ⟨k1, q1, qs1, e1, n1, s1, f1, v1, pa1, here1, r1⟩
      cases h2
      case inl shown2 =>
        rcases shown2 with ⟨_, q2, qs2, e2, n2, s2, f2, v2, pa2, here2, r2⟩
        rw [e1, e2]
        have tail := ih qs1 qs2 r1 r2
        have parents := nearest_unique ps f file p.parent _ _ pa1 pa2
        have heres : q1.here = q2.here := Bool.eq_iff_iff.2 (Iff.trans here1 (Iff.symm here2))
        rw [point_ext q1 q2 (n1.trans n2.symm) parents heres (s1.trans s2.symm)
          (f1.trans f2.symm) (v1.trans v2.symm), tail]
      case inr hidden2 =>
        rcases hidden2 with ⟨k2, _⟩
        exact absurd k1 k2
    case inr hidden1 =>
      rcases hidden1 with ⟨k1, r1⟩
      cases h2
      case inl shown2 =>
        rcases shown2 with ⟨k2, _⟩
        exact absurd k2 k1
      case inr hidden2 =>
        rcases hidden2 with ⟨_, r2⟩
        exact ih ys1 ys2 r1 r2

/-- The filtered view meets its specification, and nothing else does.

@pins REQ-UNDO.filtered_view -/
theorem filtered_view_pinned :
    (∀ x1 x2 x3, FilteredView x1 x2 x3 (shownPoints x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, FilteredView x1 x2 x3 y1 → FilteredView x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3
    exact view_meets x1 x2 x3 (position x1) x1
  · intro x1 x2 x3 y1 y2 h1 h2
    exact view_unique x1 x2 x3 (position x1) x1 y1 y2 h1 h2

end TraceLean.SpecHistory
