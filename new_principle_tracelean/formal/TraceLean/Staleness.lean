import Lean

namespace TraceLean.Staleness

open Lean (ToJson FromJson)

inductive Staleness where
  | inputChanged (name : String)
  | linkRetargeted
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Record where
  linkHash : String
  inputs : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def lookup (current : List (String × String)) (name : String) : Option String :=
  (current.find? (·.1 == name)).map (·.2)

/-- @models REQ-STALE.change_invalidates
@models REQ-STALE.retarget_invalidates -/
def staleness (record : Record) (liveLinkHashes : List String)
    (current : List (String × String)) : Option Staleness :=
  if !(liveLinkHashes.contains record.linkHash) then
    some .linkRetargeted
  else
    match record.inputs.find? (fun input => lookup current input.1 != some input.2) with
    | some (name, _) => some (.inputChanged name)
    | none => none

/-- @proves REQ-STALE.retarget_invalidates -/
theorem retargeted_is_stale (record : Record) (current : List (String × String))
    (h : Not (List.Mem record.linkHash ([] : List String))) :
    staleness record [] current = some .linkRetargeted := by
  simp [staleness]

structure Sweep where
  valid : List Record := []
  stale : List (Record × Staleness) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-STALE.scanner_never_writes
@models REQ-STALE.stale_is_visible -/
def sweep : List Record → List String → List (String × String) → Sweep
  | [], _, _ => {}
  | record :: rest, liveLinkHashes, current =>
    let tail := sweep rest liveLinkHashes current
    match staleness record liveLinkHashes current with
    | none => { tail with valid := record :: tail.valid }
    | some why => { tail with stale := (record, why) :: tail.stale }

/-- @proves REQ-STALE.stale_is_visible -/
theorem a_sweep_loses_nothing (liveLinkHashes : List String)
    (current : List (String × String)) :
    ∀ records : List Record,
      (sweep records liveLinkHashes current).valid.length
        + (sweep records liveLinkHashes current).stale.length = records.length := by
  intro records
  induction records
  case nil => simp [sweep]
  case cons record rest ih =>
    simp only [sweep]
    split <;> simp_all <;> omega

/-- @models REQ-STALE.no_silent_revalidation -/
def revalidated (stale : List Record) (produced : List Record) : List Record :=
  produced.filter (fun fresh => stale.any (·.linkHash == fresh.linkHash))

/-- @proves REQ-STALE.no_silent_revalidation -/
theorem nothing_revives_itself (stale : List Record) :
    revalidated stale [] = [] := by
  simp [revalidated]

structure HeldRun where
  op : String
  agreed : Bool
  record : Record
  current : List (String × String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- @models REQ-STALE.agreed_not_rerun -/
def rerun (held : List HeldRun) (op : String) (live : List String) (again : Bool) : Bool :=
  again || !(held.any (fun h => h.op == op && h.agreed && (staleness h.record live h.current).isNone))

/-- @proves REQ-STALE.change_invalidates -/
theorem a_changed_input_invalidates (record : Record) (live : List String)
    (current : List (String × String)) (name hash : String)
    (held : List.Mem (name, hash) record.inputs) (moved : lookup current name ≠ some hash) :
    staleness record live current ≠ none := by
  unfold staleness
  split
  · simp
  · split
    · simp
    · rename_i nothing
      have kept := List.find?_eq_none.1 nothing (name, hash) held
      simp at kept
      exact absurd kept moved

/-- @proves REQ-STALE.scanner_never_writes -/
theorem a_sweep_returns_only_what_it_was_given (live : List String)
    (current : List (String × String)) :
    ∀ records : List Record,
      (∀ r, List.Mem r (sweep records live current).valid → List.Mem r records) ∧
      (∀ p, List.Mem p (sweep records live current).stale → List.Mem p.1 records) := by
  intro records
  induction records
  case nil =>
    constructor
    · intro r hr
      nomatch hr
    · intro p hp
      nomatch hp
  case cons record rest ih =>
    simp only [sweep]
    split
    · constructor
      · intro r hr
        cases hr
        · exact List.Mem.head _
        · rename_i h
          exact List.Mem.tail _ (ih.1 r h)
      · intro p hp
        exact List.Mem.tail _ (ih.2 p hp)
    · constructor
      · intro r hr
        exact List.Mem.tail _ (ih.1 r hr)
      · intro p hp
        cases hp
        · exact List.Mem.head _
        · rename_i h
          exact List.Mem.tail _ (ih.2 p h)

/-- @proves REQ-STALE.agreed_not_rerun -/
theorem run_again_only_when_nothing_valid_agreed (held : List HeldRun) (op : String)
    (live : List String) :
    rerun held op live true = true ∧
    (rerun held op live false = false ↔
      ∃ h, List.Mem h held ∧ h.op = op ∧ h.agreed = true ∧ staleness h.record live h.current = none) := by
  constructor
  · simp [rerun]
  · simp [rerun, List.any_eq_true, and_assoc]
    exact Iff.rfl
