import Lean

/-!
# Evidence algebra

What a claim is worth, and how per-bond worth combines. Models `REQ-EVID`.

The whole content is the refusal to average: a proved model whose implementation
nobody bound to it must read `L4 model · L1 code`, and the minimum is the only
aggregate that preserves what its inputs mean.
-/

namespace TraceLean.Evidence

open Lean (Json ToJson FromJson)

-- Encodings are derived rather than written: the wire shape is then a
-- consequence of the datatype, not a second description of it that can drift.

/-- @models REQ-EVID.ladder -/
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
none.

A bond with no record is not missing from the aggregate — it contributes the
lowest level, which is what makes an unchecked bond visible rather than absent.

@models REQ-EVID.absent_is_lowest
-/
def bondLevel (records : List Record) (b : Bond) : Level :=
  records.foldl (fun acc r => if r.bond = b then Level.max acc r.level else acc) Level.L1

/--
A link's assurance: the minimum over the three bonds.

@models REQ-EVID.weakest_link
@models REQ-EVID.monotone
-/
def assurance (records : List Record) : Level :=
  allBonds.foldl (fun acc b => Level.min acc (bondLevel records b)) Level.L4

/--
The per-bond chain, in `allBonds` order. The same information the single value
collapses, kept presentable.

@models REQ-EVID.chain_rendered
-/
def chain (records : List Record) : List Level :=
  allBonds.map (bondLevel records)

/-! ## Properties -/

namespace Level

theorem le_refl (a : Level) : a ≤ a := Nat.le_refl _

theorem le_trans {a b c : Level} (h1 : a ≤ b) (h2 : b ≤ c) : a ≤ c :=
  Nat.le_trans h1 h2

theorem min_le_left (a b : Level) : Level.min a b ≤ a := by
  unfold Level.min
  split
  · exact le_refl a
  · next h => exact Nat.le_of_lt (Nat.lt_of_not_le h)

theorem min_le_right (a b : Level) : Level.min a b ≤ b := by
  unfold Level.min
  split
  · next h => exact h
  · exact le_refl b

/-- A minimum is one of its arguments. This is what rules out an average: the
result is always a level something actually established. -/
theorem min_eq (a b : Level) : Level.min a b = a ∨ Level.min a b = b := by
  unfold Level.min
  split
  · exact Or.inl rfl
  · exact Or.inr rfl

theorem toNat_le_four (a : Level) : a.toNat ≤ 4 := by cases a <;> simp [toNat]

theorem eq_L4_of_four_le {a : Level} (h : Level.L4.toNat ≤ a.toNat) : a = Level.L4 := by
  cases a <;> simp_all [toNat]

/-- `L4` is the top, so taking a minimum against it is the identity. -/
theorem min_L4_left (a : Level) : Level.min Level.L4 a = a := by
  unfold Level.min
  split
  · next h => exact (eq_L4_of_four_le h).symm
  · rfl

end Level

/--
Assurance never exceeds any bond: the aggregate is bounded by its weakest part,
which is what "never an average" means operationally. With three bonds, a link
is exactly as good as its worst-evidenced one.

@proves REQ-EVID.weakest_link
-/
theorem assurance_le_bond (records : List Record) (b : Bond) :
    assurance records ≤ bondLevel records b := by
  cases b
  case requirementModel =>
    exact Level.le_trans (Level.min_le_left _ _)
      (Level.le_trans (Level.min_le_left _ _) (Level.min_le_right _ _))
  case modelImpl =>
    exact Level.le_trans (Level.min_le_left _ _) (Level.min_le_right _ _)
  case modelProof =>
    exact Level.min_le_right _ _

/--
Assurance is one of the bond levels, never a value between them. An average
could invent a level nothing established; a minimum cannot.

@proves REQ-EVID.weakest_link
-/
theorem assurance_mem_chain (records : List Record) :
    List.Mem (assurance records) (chain records) := by
  have h : assurance records =
      Level.min (Level.min (bondLevel records .requirementModel)
        (bondLevel records .modelImpl)) (bondLevel records .modelProof) := by
    simp [assurance, allBonds, List.foldl, Level.min_L4_left]
  simp only [chain, allBonds, List.map, h]
  -- `Or.elim` rather than `rcases … with h | h`: the same case split, written in
  -- the subset of Lean the annotation grammar reads (ADR-0008).
  refine (Level.min_eq (Level.min (bondLevel records .requirementModel)
      (bondLevel records .modelImpl)) (bondLevel records .modelProof)).elim ?_ ?_
  · intro h1
    rw [h1]
    refine (Level.min_eq (bondLevel records .requirementModel)
        (bondLevel records .modelImpl)).elim ?_ ?_
    · intro h2
      rw [h2]
      exact List.Mem.head _
    · intro h2
      rw [h2]
      exact List.Mem.tail _ (List.Mem.head _)
  · intro h1
    rw [h1]
    exact List.Mem.tail _ (List.Mem.tail _ (List.Mem.head _))

/-! ## Monotonicity

`REQ-EVID.monotone`. Adding a record never lowers an assurance. This is what
keeps the algebra sane under incremental work: somebody proving a theorem, or a
differential run finishing, can only move a figure up, so nobody has to reason
about evidence that made things worse.

It follows from the shape rather than from care: each bond takes a *maximum*
over its records, so one more record can only raise it, and the aggregate is a
minimum over values that only rose.
-/

/-- One more record never lowers a bond's level. -/
theorem bond_level_is_monotone (records : List Record) (r : Record) (b : Bond) :
    (bondLevel records b).toNat ≤ (bondLevel (records ++ [r]) b).toNat := by
  simp only [bondLevel, List.foldl_append, List.foldl_cons, List.foldl_nil]
  split
  · unfold Level.max
    split <;> omega
  · omega

/-- And therefore never lowers an assurance.

@proves REQ-EVID.monotone -/
theorem adding_a_record_never_lowers_assurance (records : List Record) (r : Record) :
    (assurance records).toNat ≤ (assurance (records ++ [r])).toNat := by
  have h := fun b => bond_level_is_monotone records r b
  simp only [assurance, allBonds, List.foldl_cons, List.foldl_nil]
  have h1 := h Bond.requirementModel
  have h2 := h Bond.modelImpl
  have h3 := h Bond.modelProof
  unfold Level.min
  split <;> split <;> split <;> split <;> split <;> split <;> omega

end TraceLean.Evidence
