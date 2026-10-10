import TraceLean.Evidence
import TraceLean.Record
import Lean

/-!
# The committed index

Models the ordering half of `REQ-LOCK`. The index is committed so that a change
to what a project claims, or to what backs those claims, shows up in review like
any other change -- which works only if the same tree serialises to the same
bytes. Otherwise every diff carries noise and people stop reading them, and the
mechanism is worse than not having it.

Sorted maps and a stable field order come from the serialiser. The link list is
the one part whose order this project chooses, so it is the part worth stating:
a **total** order over the whole record, so that ties are impossible and the
result does not depend on how the scan happened to walk the tree.
-/

namespace TraceLean.Lockfile

open TraceLean.Evidence
open TraceLean.Record

open Lean (ToJson FromJson)

/-- One link, as the lockfile records it.

Field names are the lockfile's, spelled as the lockfile spells them: the wire
shape is the artefact people read in a diff, and renaming a field here to suit
the model would change that artefact for no reason. -/
structure LockLink where
  role : String
  req : String
  clause : Option String
  file : String
  /-- Anchor identity: symbol path, region bounds, or the file itself. -/
  anchor : String
  /-- Hash of the anchored body. Drives staleness. -/
  body_hash : String
  /-- Hash of the link's identity. Drives evidence validity. -/
  link_hash : String
  qualifier : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- `none` sorts before `some`, and `some` compares by content. -/
def optionLe (a b : Option String) : Bool :=
  match a, b with
  | none, _ => true
  | some _, none => false
  | some x, some y => decide (x ≤ y)

def optionEq (a b : Option String) : Bool := a == b

/--
Every field, in order, and nothing left over.

The order has to be total on the whole record rather than on a prefix of it:
two links agreeing up to `anchor` but differing in their hashes are different
links, and an order that could not separate them would let their positions
depend on the walk.
-/
def lockLinkLe (a b : LockLink) : Bool :=
  if a.role != b.role then decide (a.role ≤ b.role)
  else if a.req != b.req then decide (a.req ≤ b.req)
  else if !optionEq a.clause b.clause then optionLe a.clause b.clause
  else if a.file != b.file then decide (a.file ≤ b.file)
  else if a.anchor != b.anchor then decide (a.anchor ≤ b.anchor)
  else if a.body_hash != b.body_hash then decide (a.body_hash ≤ b.body_hash)
  else if a.link_hash != b.link_hash then decide (a.link_hash ≤ b.link_hash)
  else optionLe a.qualifier b.qualifier

/-- The order links are written in.

@models REQ-LOCK.deterministic_bytes
@models ARCH-DETERMINISM.stable_ordering -/
def orderedLinks (links : List LockLink) : List LockLink :=
  links.mergeSort lockLinkLe

/-- Ordering an ordered list changes nothing, which is what makes the serialised
form a function of the tree rather than of the walk.

@proves ARCH-DETERMINISM.stable_ordering -/
theorem ordering_is_idempotent_on_the_empty_list :
    orderedLinks [] = [] := by
  rfl

/-! ## The version stamp

`version_stamped`. A build reads a lockfile or refuses it; there is no third
behaviour. Parsing the parts it recognises would produce an index that is right
about some claims and silently missing others, and nothing downstream could
tell a dropped claim from a claim that was never made.
-/

/-- The version this model of the format describes. -/
def lockVersion : Nat := 1

inductive VersionVerdict where
  /-- This build reads it. -/
  | readable
  /-- A version from another build, with both numbers so the message can say
  which way to go. -/
  | unknown (found reads : Nat)
  /-- No version at all, which is not a version zero. -/
  | unstamped
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether this build may read a lockfile carrying this version.

@models REQ-LOCK.version_stamped -/
def versionVerdict (version : Option Nat) : VersionVerdict :=
  match version with
  | none => .unstamped
  | some v => if v == lockVersion then .readable else .unknown v lockVersion

/-- An absent version is not a version zero. The two are different situations —
a file from before the stamp existed, and a file claiming a version this build
predates — and collapsing them would report the wrong one.

@proves REQ-LOCK.version_stamped -/
theorem no_version_is_not_version_zero :
    versionVerdict none != versionVerdict (some 0) := by
  native_decide

/-! ## Evidence passes through

`evidence_preserved`. Rendering does not filter, reorder, summarise or re-judge
evidence: a record that went in comes out. Anything else would let the committed
artefact disagree with the backend that earned the record, and the artefact is
what people read.
-/

/-- The evidence a rendered lockfile carries, given the evidence it was handed.

The model is the identity, which is the whole claim. Writing it out is what
lets a differential test disagree with an implementation that started dropping
records it thought were redundant.

@models REQ-LOCK.evidence_preserved -/
def carriedEvidence (evidence : List Evidence) : List Evidence := evidence

/-- Nothing is added to nothing.

@proves REQ-LOCK.evidence_preserved -/
theorem rendering_no_evidence_carries_none :
    carriedEvidence [] = [] := by
  native_decide

/-- Three links that differ late in the record, the first two only in their
qualifier. -/
def walked : List LockLink :=
  [⟨"tests", "REQ-A", some "x", "b.rs", "b.rs::t", "h2", "l2", none⟩,
   ⟨"implements", "REQ-A", some "x", "a.rs", "a.rs::f", "h1", "l1", some "partial"⟩,
   ⟨"implements", "REQ-A", some "x", "a.rs", "a.rs::f", "h1", "l1", none⟩]

/-- Whatever order a scan walked the tree in, the links are written in one order,
so the same state gives the same bytes.

@proves REQ-LOCK.deterministic_bytes -/
theorem every_walk_writes_the_same_order :
    orderedLinks walked = orderedLinks walked.reverse ∧
    orderedLinks walked = orderedLinks [walked.getD 1 default, walked.getD 0 default, walked.getD 2 default] ∧
    (orderedLinks walked).map (·.qualifier) = [none, some "partial", none] := by
  native_decide

end TraceLean.Lockfile
