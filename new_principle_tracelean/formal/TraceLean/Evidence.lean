import Lean

namespace TraceLean.Evidence

open Lean (Json ToJson FromJson)

inductive Level where
  | L1 | L2 | L3 | L4
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-EVID.bonds_separate -/
inductive Bond where
  | requirementModel
  | modelImpl
  | modelProof
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Record where
  bond : Bond
  level : Level
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Level

def toNat : Level → Nat
  | L1 => 1 | L2 => 2 | L3 => 3 | L4 => 4

instance : LE Level := ⟨fun a b => a.toNat ≤ b.toNat⟩
instance : LT Level := ⟨fun a b => a.toNat < b.toNat⟩
instance (a b : Level) : Decidable (a ≤ b) := inferInstanceAs (Decidable (a.toNat ≤ b.toNat))

def max (a b : Level) : Level := if a.toNat ≤ b.toNat then b else a
def min (a b : Level) : Level := if a.toNat ≤ b.toNat then a else b

end Level

/-- @models REQ-EVID.absent_is_unset -/
def bondLevel (records : List Record) (b : Bond) : Level :=
  records.foldl (fun acc r => if r.bond = b then Level.max acc r.level else acc) Level.L1

/-- @models REQ-EVID.marks -/
structure Marks where
  annotated : Bool
  judged : Bool
  tested : Bool
  proved : Bool
  structural : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Mark where
  | annotated | judged | tested | proved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Marks

def each (m : Marks) : List (Mark × Bool × Char) :=
  [(.annotated, m.annotated, 'A'), (.judged, m.judged, 'J'), (.tested, m.tested, 'T'),
   (.proved, m.proved, 'P')]

def empty : Marks := ⟨false, false, false, false, false⟩

def neutral : Marks := ⟨true, true, false, false, true⟩

def applies (m : Marks) : Mark → Bool
  | .tested => !m.structural
  | .proved => !m.structural
  | _ => true

def holds (m floor : Marks) : Bool :=
  (m.annotated || !floor.annotated) && (m.judged || !floor.judged) &&
    (m.tested || !floor.tested) && (m.proved || !floor.proved)

def both (a b : Marks) : Marks :=
  let s := a.structural && b.structural
  ⟨a.annotated && b.annotated, a.judged && b.judged,
   !s && (a.tested || a.structural) && (b.tested || b.structural),
   !s && (a.proved || a.structural) && (b.proved || b.structural), s⟩

end Marks

/-- @models REQ-EVID.mark_per_bond
@models REQ-EVID.monotone -/
def marks (records : List Record) (annotated : Bool) : Marks :=
  ⟨annotated,
   decide (2 ≤ (bondLevel records .requirementModel).toNat),
   decide (3 ≤ (bondLevel records .modelImpl).toNat),
   decide (4 ≤ (bondLevel records .modelProof).toNat), false⟩

/-- @models REQ-EVID.structural_not_applicable -/
def structural (m : Marks) : Marks := { m with tested := false, proved := false, structural := true }

def letter (held : Bool) (c : Char) : Char := if held then c else '-'

def letterOf (m : Marks) (each : Mark × Bool × Char) : Char :=
  if m.applies each.1 then letter each.2.1 each.2.2 else '·'

/-- @models REQ-EVID.marks_written -/
def written (m : Marks) : String :=
  String.mk (m.each.map (letterOf m))

/-- @proves REQ-EVID.mark_per_bond -/
theorem each_mark_reads_its_own_bond (records : List Record) (a : Bool) :
    (marks records a).judged = decide (2 ≤ (bondLevel records .requirementModel).toNat) ∧
    (marks records a).tested = decide (3 ≤ (bondLevel records .modelImpl).toNat) ∧
    (marks records a).proved = decide (4 ≤ (bondLevel records .modelProof).toNat) :=
  ⟨rfl, rfl, rfl⟩

theorem bond_level_is_monotone (records : List Record) (r : Record) (b : Bond) :
    (bondLevel records b).toNat ≤ (bondLevel (records ++ [r]) b).toNat := by
  simp only [bondLevel, List.foldl_append, List.foldl_cons, List.foldl_nil]
  split
  · unfold Level.max
    split <;> omega
  · omega

/-- @proves REQ-EVID.monotone -/
theorem adding_a_record_never_clears_a_mark (records : List Record) (r : Record) (a : Bool) :
    (marks (records ++ [r]) a).holds (marks records a) = true := by
  have kept : ∀ x y n : Nat, x ≤ y → (decide (n ≤ y) || !decide (n ≤ x)) = true := by
    intro x y n hxy
    by_cases h : n ≤ x
    · have : n ≤ y := Nat.le_trans h hxy
      simp [this]
    · simp [h]
  simp only [marks, Marks.holds,
    kept _ _ 2 (bond_level_is_monotone records r .requirementModel),
    kept _ _ 3 (bond_level_is_monotone records r .modelImpl),
    kept _ _ 4 (bond_level_is_monotone records r .modelProof)]
  cases a <;> rfl

/-- @proves REQ-EVID.absent_is_unset -/
theorem no_records_set_no_mark (a : Bool) : marks [] a = ⟨a, false, false, false, false⟩ := by
  cases a <;> rfl

/-- @proves REQ-EVID.bonds_separate -/
theorem a_record_moves_only_its_own_bond (records : List Record) (b other : Bond) (l : Level)
    (h : other ≠ b) :
    bondLevel (records ++ [⟨other, l⟩]) b = bondLevel records b := by
  simp [bondLevel, List.foldl_append, h]

/-- @proves REQ-EVID.structural_not_applicable -/
theorem testing_and_proving_do_not_apply_by_reading (m other : Marks) (h : other.structural = false) :
    (structural m).tested = false ∧ (structural m).proved = false ∧
    (structural m).holds ⟨false, false, true, false, false⟩ = false ∧
    (Marks.both (structural m) other).tested = other.tested ∧
    (Marks.both (structural m) other).proved = other.proved ∧
    (Marks.both other (structural m)).tested = other.tested := by
  refine ⟨rfl, rfl, ?_, ?_, ?_, ?_⟩
  · simp [structural, Marks.holds]
  · simp [structural, Marks.both, h]
  · simp [structural, Marks.both, h]
  · simp [structural, Marks.both, h]


def recordsFor (j t p : Bool) : List Record :=
  (if j then [⟨.requirementModel, .L2⟩] else []) ++ (if t then [⟨.modelImpl, .L3⟩] else []) ++
    (if p then [⟨.modelProof, .L4⟩] else [])

/-- @proves REQ-EVID.marks -/
theorem every_combination_of_marks_is_reachable (a j t p : Bool) :
    (∃ m : Marks, m.annotated = a ∧ m.judged = j ∧ m.tested = t ∧ m.proved = p) ∧
    ∃ records, (marks records a).annotated = a ∧ (marks records a).judged = j ∧
      (marks records a).tested = t ∧ (marks records a).proved = p := by
  refine ⟨⟨⟨a, j, t, p, false⟩, rfl, rfl, rfl, rfl⟩, recordsFor j t p, rfl, ?_⟩
  cases j <;> cases t <;> cases p <;> exact ⟨rfl, rfl, rfl⟩

/-- @proves REQ-EVID.marks_written -/
theorem marks_written_in_fixed_positions (m : Marks) :
    (written m).toList =
      [if m.annotated then 'A' else '-', if m.judged then 'J' else '-',
       if m.structural then '·' else if m.tested then 'T' else '-',
       if m.structural then '·' else if m.proved then 'P' else '-'] ∧
    (written m).length = 4 := by
  have h : (written m).toList =
      [if m.annotated then 'A' else '-', if m.judged then 'J' else '-',
       if m.structural then '·' else if m.tested then 'T' else '-',
       if m.structural then '·' else if m.proved then 'P' else '-'] := by
    cases m with
    | mk a j t p s => cases s <;> rfl
  refine ⟨h, ?_⟩
  show (written m).toList.length = 4
  rw [h]
  rfl

end TraceLean.Evidence
