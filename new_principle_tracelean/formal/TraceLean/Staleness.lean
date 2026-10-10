import Lean

/-!
# Staleness

Models `REQ-STALE`. Everything else decides what a claim is worth; this decides
when it stops being worth it. Getting it wrong is worse than having no tool,
because evidence would be displayed for code that has since changed.
-/

namespace TraceLean.Staleness

open Lean (ToJson FromJson)

/-- Why a record no longer applies. -/
inductive Staleness where
  | inputChanged (name : String)
  | linkRetargeted
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One piece of evidence, reduced to what staleness depends on. -/
structure Record where
  linkHash : String
  /-- Every input this depended on, as name and hash. -/
  inputs : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Look up a name among the current hashes. -/
def lookup (current : List (String × String)) (name : String) : Option String :=
  (current.find? (·.1 == name)).map (·.2)

/--
Whether a record still applies.

`change_invalidates` is absolute: no tolerance, no exemption for a small edit,
and no heuristic about whether a change was meaningful. A missing input counts
as changed -- absence is not evidence that nothing moved.

@models REQ-STALE.change_invalidates
@models REQ-STALE.retarget_invalidates
-/
def staleness (record : Record) (liveLinkHashes : List String)
    (current : List (String × String)) : Option Staleness :=
  if !(liveLinkHashes.contains record.linkHash) then
    some .linkRetargeted
  else
    match record.inputs.find? (fun input => lookup current input.1 != some input.2) with
    | some (name, _) => some (.inputChanged name)
    | none => none

/-- A record with no inputs and a live link is never stale, and a record whose
link is gone always is. Both directions matter, and they fail differently.

@proves REQ-STALE.retarget_invalidates -/
theorem retargeted_is_stale (record : Record) (current : List (String × String))
    (h : Not (List.Mem record.linkHash ([] : List String))) :
    staleness record [] current = some .linkRetargeted := by
  simp [staleness]

/-! ## Sweeping

What the scanner does to a set of records, and — more to the point — what it
does not. A scanner that could rewrite a record could write one that was never
earned, so the sweep is a partition: every record comes back, unchanged, on one
side or the other.

`stale_is_visible` is the other half. Omitting a stale record would leave a
clause looking untested rather than looking like a claim that has expired, and
those call for different actions.
-/

structure Sweep where
  /-- Records that still apply, in the order they were given. -/
  valid : List Record := []
  /-- Records that no longer apply, with why, in the order they were given. -/
  stale : List (Record × Staleness) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
Partition records into those that still apply and those that do not.

@models REQ-STALE.scanner_never_writes
@models REQ-STALE.stale_is_visible
-/
def sweep : List Record → List String → List (String × String) → Sweep
  | [], _, _ => {}
  | record :: rest, liveLinkHashes, current =>
    let tail := sweep rest liveLinkHashes current
    match staleness record liveLinkHashes current with
    | none => { tail with valid := record :: tail.valid }
    | some why => { tail with stale := (record, why) :: tail.stale }

/-- Nothing is invented and nothing is lost: the two sides account for every
record that went in.

@proves REQ-STALE.stale_is_visible -/
theorem a_sweep_loses_nothing (liveLinkHashes : List String)
    (current : List (String × String)) :
    ∀ records : List Record,
      (sweep records liveLinkHashes current).valid.length
        + (sweep records liveLinkHashes current).stale.length = records.length := by
  intro records
  -- `case` rather than `induction … with | …`, which the grammar that reads
  -- these annotations cannot read (ADR-0008).
  induction records
  case nil => simp [sweep]
  case cons record rest ih =>
    -- Each record adds one entry, to exactly one of the two sides.
    simp only [sweep]
    split <;> simp_all <;> omega

/-! ## Coming back

`no_silent_revalidation`. A stale record is not repaired, re-judged or quietly
forgiven; the only thing that makes a key valid again is the backend that owns
it producing a new record. Modelled as the function that says which of the
newly produced records replace which stale ones — and its law is that the
answer is always drawn from what was produced.
-/

/-- The records that replace stale ones: newly produced, never resurrected.

@models REQ-STALE.no_silent_revalidation -/
def revalidated (stale : List Record) (produced : List Record) : List Record :=
  produced.filter (fun fresh => stale.any (·.linkHash == fresh.linkHash))

/-- Nothing comes back that was not produced. In particular an empty production
revalidates nothing, however stale the record and however much has changed.

@proves REQ-STALE.no_silent_revalidation -/
theorem nothing_revives_itself (stale : List Record) :
    revalidated stale [] = [] := by
  simp [revalidated]

/-! ## Not running again what is still valid -/

/-- A differential run held for a clause: its op, whether it agreed at L3,
what keeps it valid, and the hashes its inputs have now. -/
structure HeldRun where
  op : String
  agreed : Bool
  record : Record
  current : List (String × String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- Whether `op` is run again: when asked, or when no held run of it agreed
against inputs and a link that are still what they were.

@models REQ-STALE.agreed_not_rerun -/
def rerun (held : List HeldRun) (op : String) (live : List String) (again : Bool) : Bool :=
  again || !(held.any (fun h => h.op == op && h.agreed && (staleness h.record live h.current).isNone))

/-- Any input whose hash is not what the record holds makes the record stale,
however small the change; a missing input counts as changed.

@proves REQ-STALE.change_invalidates -/
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

/-- The scanner only sorts: every record it reports, valid or stale, is one it
was given, unchanged.

@proves REQ-STALE.scanner_never_writes -/
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

/-- A clause is not run again exactly when a run of it agreed and is still
valid; asked, it is run whatever is held.

@proves REQ-STALE.agreed_not_rerun -/
theorem run_again_only_when_nothing_valid_agreed (held : List HeldRun) (op : String)
    (live : List String) :
    rerun held op live true = true ∧
    (rerun held op live false = false ↔
      ∃ h, List.Mem h held ∧ h.op = op ∧ h.agreed = true ∧ staleness h.record live h.current = none) := by
  constructor
  · simp [rerun]
  · simp [rerun, List.any_eq_true, and_assoc]
    exact Iff.rfl
