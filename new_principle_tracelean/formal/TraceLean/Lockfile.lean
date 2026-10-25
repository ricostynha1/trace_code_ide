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

/-- @models REQ-LOCK.evidence_preserved -/
def carriedEvidence (evidence : List Evidence) : List Evidence := evidence

end TraceLean.Lockfile
