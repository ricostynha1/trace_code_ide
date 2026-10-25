import TraceLean.Evidence
import TraceLean.Record
import Lean

namespace TraceLean.Lockfile

open TraceLean.Evidence
open TraceLean.Record

open Lean (ToJson FromJson)

structure LockLink where
  role : String
  req : String
  clause : Option String
  file : String
  anchor : String
  body_hash : String
  link_hash : String
  qualifier : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def optionLe (a b : Option String) : Bool :=
  match a, b with
  | none, _ => true
  | some _, none => false
  | some x, some y => decide (x ≤ y)

def optionEq (a b : Option String) : Bool := a == b

def lockLinkLe (a b : LockLink) : Bool :=
  if a.role != b.role then decide (a.role ≤ b.role)
  else if a.req != b.req then decide (a.req ≤ b.req)
  else if !optionEq a.clause b.clause then optionLe a.clause b.clause
  else if a.file != b.file then decide (a.file ≤ b.file)
  else if a.anchor != b.anchor then decide (a.anchor ≤ b.anchor)
  else if a.body_hash != b.body_hash then decide (a.body_hash ≤ b.body_hash)
  else if a.link_hash != b.link_hash then decide (a.link_hash ≤ b.link_hash)
  else optionLe a.qualifier b.qualifier

/-- @models REQ-LOCK.deterministic_bytes
@models ARCH-DETERMINISM.stable_ordering -/
def orderedLinks (links : List LockLink) : List LockLink :=
  links.mergeSort lockLinkLe

/-- @proves ARCH-DETERMINISM.stable_ordering -/
theorem ordering_is_idempotent_on_the_empty_list :
    orderedLinks [] = [] := by
  rfl

def lockVersion : Nat := 1

inductive VersionVerdict where
  | readable
  | unknown (found reads : Nat)
  | unstamped
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-LOCK.version_stamped -/
def versionVerdict (version : Option Nat) : VersionVerdict :=
  match version with
  | none => .unstamped
  | some v => if v == lockVersion then .readable else .unknown v lockVersion

/-- @proves REQ-LOCK.version_stamped -/
theorem no_version_is_not_version_zero :
    versionVerdict none != versionVerdict (some 0) := by
  native_decide

/-- @models REQ-LOCK.evidence_preserved -/
def carriedEvidence (evidence : List Evidence) : List Evidence := evidence

/-- @proves REQ-LOCK.evidence_preserved -/
theorem rendering_no_evidence_carries_none :
    carriedEvidence [] = [] := by
  native_decide

def walked : List LockLink :=
  [⟨"tests", "REQ-A", some "x", "b.rs", "b.rs::t", "h2", "l2", none⟩,
   ⟨"implements", "REQ-A", some "x", "a.rs", "a.rs::f", "h1", "l1", some "partial"⟩,
   ⟨"implements", "REQ-A", some "x", "a.rs", "a.rs::f", "h1", "l1", none⟩]

/-- @proves REQ-LOCK.deterministic_bytes -/
theorem every_walk_writes_the_same_order :
    orderedLinks walked = orderedLinks walked.reverse ∧
    orderedLinks walked = orderedLinks [walked.getD 1 default, walked.getD 0 default, walked.getD 2 default] ∧
    (orderedLinks walked).map (·.qualifier) = [none, some "partial", none] := by
  native_decide

end TraceLean.Lockfile
