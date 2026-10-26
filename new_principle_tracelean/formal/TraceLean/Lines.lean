import TraceLean.Strength

namespace TraceLean.Lines

open Lean (ToJson FromJson)
open TraceLean.Strength (sortedUnique)

structure TestLines where
  test : String
  file : String
  lines : List (Nat × Nat)
  deriving Repr, Inhabited, ToJson, FromJson

structure LineHits where
  line : Nat
  hits : Nat
  tests : List (String × Nat)
  deriving Repr, Inhabited, DecidableEq, ToJson, FromJson

structure Reach where
  run : Nat
  all : Nat
  tests : List String
  deriving Repr, Inhabited, ToJson, FromJson

def sum (xs : List Nat) : Nat := xs.foldl (· + ·) 0

def countOf (runs : List TestLines) (test : String) (line : Nat) : Nat :=
  sum (runs.filter (fun r => r.test == test) |>.map (fun r =>
    sum (r.lines.filter (fun p => p.1 == line) |>.map (·.2))))

/-- @models REQ-LINECOV.per_test -/
def merged (runs : List TestLines) (file : String) : List LineHits :=
  let here := runs.filter (fun r => r.file == file)
  let lines := sortedUnique (fun a b => decide (a ≤ b)) (here.bind (fun r => r.lines.map (·.1)))
  lines.map (fun line =>
    let ran := here.filter (fun r => r.lines.any (fun p => p.1 == line && p.2 > 0))
    let names := sortedUnique (fun a b => decide (a ≤ b)) (ran.map (·.test))
    let tests := names.map (fun t => (t, countOf (here.map (fun r =>
      { r with lines := r.lines.filter (fun p => p.2 > 0) })) t line))
    { line := line, hits := sum (tests.map (·.2)), tests := tests })

/-- @models REQ-LINECOV.stale_hidden -/
def visible (measured : Option (String × List LineHits)) (hash : String)
    : Option (List LineHits) :=
  match measured with
  | none => none
  | some m => if m.1 == hash then some m.2 else none

/-- @models REQ-LINECOV.clause_summary -/
def spanCoverage (lines : List LineHits) (start stop : Nat) : Reach :=
  let inside := lines.filter (fun l => start ≤ l.line && l.line ≤ stop)
  { run := (inside.filter (fun l => l.hits > 0)).length
    all := inside.length
    tests := sortedUnique (fun a b => decide (a ≤ b)) (inside.bind (fun l => l.tests.map (·.1))) }

def said (line : LineHits) : String :=
  if line.tests.isEmpty then "no test runs this line"
  else
    "run " ++ toString line.hits ++ " times by " ++
      ", ".intercalate (line.tests.map fun (t : String × Nat) => t.1 ++ " ×" ++ toString t.2)

structure Marker where
  line : Nat
  hits : Nat
  said : String
  deriving Repr, Inhabited, ToJson, FromJson

/-- @models REQ-LINECOV.uncovered_shown -/
def markers (lines : List LineHits) (top height : Nat) : List Marker :=
  (lines.filter fun l => l.line ≥ 1 && top ≤ l.line - 1 && l.line - 1 < top + height).map fun l =>
    { line := l.line - 1, hits := l.hits, said := said l }

/-- @proves REQ-LINECOV.stale_hidden -/
theorem only_coverage_of_this_text_is_shown (hash other : String) (lines : List LineHits) :
    visible (some (hash, lines)) hash = some lines ∧
    (other ≠ hash → visible (some (other, lines)) hash = none) ∧
    visible none hash = none := by
  refine ⟨?_, ?_, ?_⟩
  · simp [visible]
  · intro moved
    simp [visible, moved]
  · simp [visible]

theorem mem_insertSorted {α : Type} [BEq α] [LawfulBEq α] (le : α → α → Bool) (a x : α) :
    ∀ xs : List α, a ∈ TraceLean.Strength.insertSorted le xs x ↔ a = x ∨ a ∈ xs := by
  intro xs
  induction xs with
  | nil => simp [TraceLean.Strength.insertSorted]
  | cons y rest ih =>
    unfold TraceLean.Strength.insertSorted
    by_cases hxy : (x == y) = true
    · have e : x = y := by simpa using hxy
      subst e
      simp
    · simp only [hxy]
      by_cases hle : le x y = true
      · simp [hle]
      · simp only [hle, List.mem_cons]
        simp only [if_false, Bool.false_eq_true, List.mem_cons]
        rw [ih]
        exact or_left_comm

theorem mem_fold_insertSorted {α : Type} [BEq α] [LawfulBEq α] (le : α → α → Bool) (a : α) :
    ∀ (xs acc : List α), a ∈ xs.foldl (TraceLean.Strength.insertSorted le) acc ↔ a ∈ acc ∨ a ∈ xs := by
  intro xs
  induction xs with
  | nil => simp
  | cons x rest ih =>
    intro acc
    simp only [List.foldl_cons, ih, mem_insertSorted, List.mem_cons]
    constructor
    · intro h
      rcases h with (h | h) | h
      · exact Or.inr (Or.inl h)
      · exact Or.inl h
      · exact Or.inr (Or.inr h)
    · intro h
      rcases h with h | h | h
      · exact Or.inl (Or.inr h)
      · exact Or.inl (Or.inl h)
      · exact Or.inr h

theorem mem_sortedUnique {α : Type} [BEq α] [LawfulBEq α] (le : α → α → Bool) (a : α)
    (xs : List α) : a ∈ sortedUnique le xs ↔ a ∈ xs := by
  simp [sortedUnique, mem_fold_insertSorted]

theorem fold_add_ge (xs : List Nat) : ∀ acc, acc ≤ xs.foldl (· + ·) acc := by
  induction xs with
  | nil => intro acc; simp
  | cons x rest ih =>
    intro acc
    simp only [List.foldl_cons]
    exact Nat.le_trans (Nat.le_add_right acc x) (ih (acc + x))

theorem fold_add_ge_mem (xs : List Nat) (n : Nat) (h : n ∈ xs) :
    ∀ acc, n ≤ xs.foldl (· + ·) acc := by
  induction xs with
  | nil => simp at h
  | cons x rest ih =>
    intro acc
    simp only [List.foldl_cons]
    simp only [List.mem_cons] at h
    rcases h with h | h
    · subst h
      exact Nat.le_trans (Nat.le_add_left n acc) (fold_add_ge rest (acc + n))
    · exact ih h (acc + x)

theorem sum_ge_mem (xs : List Nat) (n : Nat) (h : n ∈ xs) : n ≤ sum xs :=
  fold_add_ge_mem xs n h 0

/-- @proves REQ-LINECOV.per_test -/
theorem each_line_knows_its_tests (runs : List TestLines) (file : String) :
    (∀ lh ∈ merged runs file, lh.hits = sum (lh.tests.map (·.2))) ∧
    (∀ lh ∈ merged runs file, ∀ e ∈ lh.tests, 0 < e.2 ∧
      ∃ r ∈ runs, r.file = file ∧ r.test = e.1 ∧ ∃ n, (lh.line, n) ∈ r.lines ∧ 0 < n) ∧
    (∀ r ∈ runs, r.file = file → ∀ p ∈ r.lines, 0 < p.2 →
      ∃ lh ∈ merged runs file, lh.line = p.1 ∧ r.test ∈ lh.tests.map (·.1)) := by
  refine ⟨?_, ?_, ?_⟩
  · intro lh h
    simp only [merged, List.mem_map] at h
    obtain ⟨_, _, rfl⟩ := h
    rfl
  · intro lh h e he
    simp only [merged, List.mem_map] at h
    obtain ⟨line, _, rfl⟩ := h
    simp only [List.mem_map] at he
    obtain ⟨t, ht, rfl⟩ := he
    rw [mem_sortedUnique] at ht
    simp only [List.mem_map, List.mem_filter, List.any_eq_true, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq] at ht
    obtain ⟨r, ⟨⟨hr, hfile⟩, ⟨p, hp, hline, hpos⟩⟩, rfl⟩ := ht
    refine ⟨?_, r, hr, hfile, rfl, p.2, ?_, hpos⟩
    · simp only [countOf]
      apply Nat.lt_of_lt_of_le hpos
      refine Nat.le_trans (sum_ge_mem (List.map (·.2) ((r.lines.filter (fun q => q.2 > 0)).filter
        (fun q => q.1 == line))) p.2 ?_) (sum_ge_mem _ _ ?_)
      · simp only [List.mem_map, List.mem_filter]
        exact ⟨p, ⟨⟨hp, by simpa using hpos⟩, by simpa using hline⟩, rfl⟩
      · simp only [List.mem_map, List.mem_filter]
        exact ⟨{ r with lines := r.lines.filter (fun q => q.2 > 0) },
          ⟨⟨r, ⟨hr, by simpa using hfile⟩, rfl⟩, by simp⟩, rfl⟩
    · rw [← hline]
      exact hp
  · intro r hr hfile p hp hpos
    have hm : p.1 ∈ sortedUnique (fun a b => decide (a ≤ b))
        ((runs.filter (fun r => r.file == file)).bind (fun r => r.lines.map (·.1))) := by
      rw [mem_sortedUnique]
      simp only [List.mem_bind, List.mem_filter, List.mem_map]
      exact ⟨r, ⟨hr, by simpa using hfile⟩, p, hp, rfl⟩
    refine ⟨_, List.mem_map_of_mem _ hm, rfl, ?_⟩
    · simp only [List.map_map]
      show r.test ∈ List.map (fun t => t) _
      rw [List.map_id']
      rw [mem_sortedUnique]
      simp only [List.mem_map, List.mem_filter, List.any_eq_true, Bool.and_eq_true, beq_iff_eq,
        decide_eq_true_eq]
      exact ⟨r, ⟨⟨hr, by simpa using hfile⟩, p, hp, rfl, hpos⟩, rfl⟩

/-- @proves REQ-LINECOV.uncovered_shown -/
theorem every_visible_line_is_marked (lines : List LineHits) (top height : Nat) :
    ∀ l ∈ lines, 1 ≤ l.line → top ≤ l.line - 1 → l.line - 1 < top + height →
      ∃ m ∈ markers lines top height, m.line = l.line - 1 ∧ m.hits = l.hits ∧ m.said = said l ∧
        (l.tests = [] → m.said = "no test runs this line") := by
  intro l hl h1 h2 h3
  refine ⟨{ line := l.line - 1, hits := l.hits, said := said l }, ?_, rfl, rfl, rfl, ?_⟩
  · simp only [markers, List.mem_map, List.mem_filter]
    exact ⟨l, ⟨hl, by simp [h1, h2, h3]⟩, rfl⟩
  · intro empty
    simp [said, empty]

end TraceLean.Lines
