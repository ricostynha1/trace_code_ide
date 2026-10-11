import Lean

/-!
# Evidence algebra

What a claim is backed by. Models `REQ-EVID`.

Four marks — annotated, judged, tested, proved — each held or not on its own.
A model can be proved and differentially tested before anybody judged it; a
single level on a ladder would read that as "annotated only", and an average
would invent a figure nothing established. `AJTP` with gaps says exactly what
is held and what is missing.
-/

namespace TraceLean.Evidence

open Lean (Json ToJson FromJson)

-- Encodings are derived rather than written: the wire shape is then a
-- consequence of the datatype, not a second description of it that can drift.

/-- What kind of record a piece of evidence is: a judgement is the second, a
differential run the third, a proof the fourth. Ordered so a record can be
capped below what it claims; never ranked across bonds. -/
inductive Level where
  | L1 | L2 | L3 | L4
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-EVID.bonds_separate -/
inductive Bond where
  | requirementModel
  | modelImpl
  | modelProof
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One evidence record: a level established on one bond. -/
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

/-- The three bonds, in a fixed order so that folding over them is deterministic. -/
def allBonds : List Bond := [.requirementModel, .modelImpl, .modelProof]

/--
The level established on one bond: the best record for it, or `L1` when there is
none, which sets no mark.

@models REQ-EVID.absent_is_unset
-/
def bondLevel (records : List Record) (b : Bond) : Level :=
  records.foldl (fun acc r => if r.bond = b then Level.max acc r.level else acc) Level.L1

/-- What evidence a clause holds, each kind on its own.

@models REQ-EVID.marks -/
structure Marks where
  annotated : Bool
  judged : Bool
  tested : Bool
  proved : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Which of the four marks a position of `AJTP` is. -/
inductive Mark where
  | annotated | judged | tested | proved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Marks

/-- Each mark in its position, with whether it is held and its letter. -/
def each (m : Marks) : List (Mark × Bool × Char) :=
  [(.annotated, m.annotated, 'A'), (.judged, m.judged, 'J'), (.tested, m.tested, 'T'),
   (.proved, m.proved, 'P')]

/-- No mark at all. -/
def empty : Marks := ⟨false, false, false, false⟩

/-- Every mark `floor` asks for is held by `m`. -/
def holds (m floor : Marks) : Bool :=
  (m.annotated || !floor.annotated) && (m.judged || !floor.judged) &&
    (m.tested || !floor.tested) && (m.proved || !floor.proved)

/-- The marks both hold. -/
def both (a b : Marks) : Marks :=
  ⟨a.annotated && b.annotated, a.judged && b.judged, a.tested && b.tested, a.proved && b.proved⟩

end Marks

/--
A clause's marks, from whether something claims it and its records: each bond
sets its own mark and no other.

@models REQ-EVID.mark_per_bond
@models REQ-EVID.monotone
-/
def marks (records : List Record) (annotated : Bool) : Marks :=
  ⟨annotated,
   decide (2 ≤ (bondLevel records .requirementModel).toNat),
   decide (3 ≤ (bondLevel records .modelImpl).toNat),
   decide (4 ≤ (bondLevel records .modelProof).toNat)⟩

/-- One position of the written marks: its letter, or a dash. -/
def letter (held : Bool) (c : Char) : Char := if held then c else '-'

/--
Marks in fixed positions, a dash where one is not held: `AJTP`, `A-TP`, `A---`.

@models REQ-EVID.marks_written
-/
def written (m : Marks) : String :=
  String.mk [letter m.annotated 'A', letter m.judged 'J', letter m.tested 'T', letter m.proved 'P']

/-! ## Properties -/

/-- Records for one combination of marks: bit 1 a judgement, bit 2 a run, bit 3
a proof. -/
def recordsFor (bits : Nat) : List Record :=
  (if bits / 2 % 2 == 1 then [⟨.requirementModel, .L2⟩] else []) ++
  (if bits / 4 % 2 == 1 then [⟨.modelImpl, .L3⟩] else []) ++
  (if bits / 8 % 2 == 1 then [⟨.modelProof, .L4⟩] else [])

/-- Every one of the sixteen combinations of marks is reachable, so none implies
another.

@proves REQ-EVID.marks -/
theorem every_combination_is_reachable :
    ((List.range 16).map (fun bits => written (marks (recordsFor bits) (bits % 2 == 1)))).eraseDups.length
      = 16 := by
  native_decide

/-- Each mark is read from its own bond and nothing else, and a record below
what its mark needs sets nothing.

@proves REQ-EVID.mark_per_bond -/
theorem each_mark_reads_its_own_bond (records : List Record) (a : Bool) :
    (marks records a).judged = decide (2 ≤ (bondLevel records .requirementModel).toNat) ∧
    (marks records a).tested = decide (3 ≤ (bondLevel records .modelImpl).toNat) ∧
    (marks records a).proved = decide (4 ≤ (bondLevel records .modelProof).toNat) ∧
    written (marks [⟨.modelProof, .L4⟩] true) = "A--P" ∧
    written (marks [⟨.requirementModel, .L1⟩, ⟨.modelImpl, .L2⟩] true) = "A---" := by
  refine ⟨rfl, rfl, rfl, ?_, ?_⟩ <;> native_decide

/-! ## Monotonicity

`REQ-EVID.monotone`. Adding a record never clears a mark. Each bond takes a
*maximum* over its records, so one more record can only raise it, and a mark is
a threshold on a value that only rose.
-/

/-- One more record never lowers a bond's level. -/
theorem bond_level_is_monotone (records : List Record) (r : Record) (b : Bond) :
    (bondLevel records b).toNat ≤ (bondLevel (records ++ [r]) b).toNat := by
  simp only [bondLevel, List.foldl_append, List.foldl_cons, List.foldl_nil]
  split
  · unfold Level.max
    split <;> omega
  · omega

/-- And therefore never clears a mark.

@proves REQ-EVID.monotone -/
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

/-- A bond nothing has recorded sets no mark.

@proves REQ-EVID.absent_is_unset -/
theorem no_records_set_no_mark (a : Bool) : marks [] a = ⟨a, false, false, false⟩ := by
  cases a <;> rfl

/-- A record on one bond leaves every other bond's level as it was.

@proves REQ-EVID.bonds_separate -/
theorem a_record_moves_only_its_own_bond (records : List Record) (b other : Bond) (l : Level)
    (h : other ≠ b) :
    bondLevel (records ++ [⟨other, l⟩]) b = bondLevel records b := by
  simp [bondLevel, List.foldl_append, h]

/-- Marks are written in fixed positions, a dash for each one not held.

@proves REQ-EVID.marks_written -/
theorem marks_are_written_in_place :
    written Marks.empty = "----" ∧ written ⟨true, false, true, true⟩ = "A-TP" ∧
    written ⟨true, true, true, true⟩ = "AJTP" := by
  native_decide

end TraceLean.Evidence
