import Lean

/-!
# Documentation as a link

Models `REQ-DOCLINK`. A document is not prose beside a system; it is a claim
about a specific thing, and the thing has a hash. When the hash moves the claim
is unconfirmed — not wrong, not fatal, but no longer something anyone checked.

Three states, and the distinction between them is the whole requirement: an
implementation that collapsed *in review* into either neighbour would either
block on ordinary edits or hide staleness entirely.
-/

namespace TraceLean.DocLink

open Lean (ToJson FromJson)

/-- What a document claims to describe, and what that hashed to when written. -/
structure DocLink where
  file : String
  target : String
  recordedHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Where a document stands relative to what it describes. -/
inductive DocState where
  /-- The recorded hash matches. -/
  | current
  /-- The hash moved. The document may still be right; nobody confirmed it. -/
  | inReview (was now : String)
  /-- The target no longer exists. -/
  | dangling
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- First match wins; a repeated target is the caller's problem. -/
def lookupHash (current : List (String × String)) (target : String) : Option String :=
  (current.find? (·.1 == target)).map (·.2)

/--
Where one document stands.

A document with no recorded hash is in review rather than current: an empty
string is not a hash anybody confirmed against.

@models REQ-DOCLINK.hash_moves_review
@models REQ-DOCLINK.dangling_reported
-/
def docState (link : DocLink) (current : List (String × String)) : DocState :=
  match lookupHash current link.target with
  | none => .dangling
  | some now => if now == link.recordedHash then .current else .inReview link.recordedHash now

/-- Whether a state stops the build. Only a dangling target does: blocking on
*in review* would make people stop writing documentation, which is the failure
this requirement exists to avoid.

@models REQ-DOCLINK.review_is_not_error -/
def docBlocks : DocState → Bool
  | .dangling => true
  | _ => false

/-- *In review* is reachable and is neither of its neighbours. It is stated
as a theorem because the requirement is precisely that the three states stay
distinct.

@proves REQ-DOCLINK.review_is_not_error -/
theorem review_is_distinct_and_does_not_block (was now : String) (h : was ≠ now) :
    docState ⟨"d.md", "T", was⟩ [("T", now)] = .inReview was now ∧
    docBlocks (.inReview was now) = false := by
  constructor
  · simp [docState, lookupHash, beq_iff_eq, Ne.symm h]
  · rfl

/-- A hash that has not moved leaves the document current. The other half of
the same property: a mechanism that reported everything in review would be as
useless as one that reported nothing.

@proves REQ-DOCLINK.hash_moves_review -/
theorem unmoved_is_current (hash : String) :
    docState ⟨"d.md", "T", hash⟩ [("T", hash)] = .current := by
  simp [docState, lookupHash]

/-! ## What a document declares

`declares_target`, `records_hash` and `decisions_exempt`. The frontmatter
grammar is two keys: `describes`, naming one target or a list of them, and
`described_hash`, either a single hash when there is exactly one target or an
indented map keyed by target.

A decision record declares neither. It describes a moment rather than a
subsystem and names the requirements it affects, so it has no hashed target and
is not in review when anything changes — which is right: a decision that was
made is still a decision that was made.
-/

/-- Strip every leading and trailing `"` or `'`. -/
def stripQuotes (s : String) : String :=
  let isQuote (c : Char) := c == '"' || c == '\''
  let chars := s.toList.dropWhile isQuote
  String.mk (chars.reverse.dropWhile isQuote).reverse

def trimRight (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

/-- Split at the first occurrence of `sep`. -/
def splitFirst (s : String) (sep : Char) : Option (String × String) :=
  match s.toList.findIdx? (· == sep) with
  | none => none
  | some i => some (String.mk (s.toList.take i), String.mk (s.toList.drop (i + 1)))

/-- Split at the *last* occurrence of `sep`.

An anchor target contains `::`, so splitting a `target: hash` entry from the
left would cut the target in half. -/
def splitLast (s : String) (sep : Char) : Option (String × String) :=
  let chars := s.toList
  match (chars.reverse.findIdx? (· == sep)) with
  | none => none
  | some j =>
    let i := chars.length - 1 - j
    some (String.mk (chars.take i), String.mk (chars.drop (i + 1)))

/-- The frontmatter lines: everything between a leading `---` line and the next
line that starts with `---`.

Written to match the implementation exactly, including the part that is a
little odd: the closing marker is found as the text `\n---`, so a line merely
*starting* with three dashes closes the block. -/
def docFrontmatter (lines : List String) : Option (List String) :=
  match lines with
  | [] => none
  | first :: rest =>
    if first != "---" || rest.isEmpty then none
    else
      match (rest.drop 1).findIdx? (fun line => line.startsWith "---") with
      | none => none
      | some i => some (rest.take (i + 1))

structure Declaring where
  targets : List String := []
  hashes : List (String × String) := []
  inHashes : Bool := false
  deriving Inhabited

def hashInsert (entries : List (String × String)) (key value : String) :
    List (String × String) :=
  (entries.filter (·.1 != key)) ++ [(key, value)]

def readDocLine (state : Declaring) (raw : String) : Declaring :=
  let line := trimRight raw
  if line.trim.isEmpty then state
  else if line.startsWith " " || line.startsWith "\t" then
    if state.inHashes then
      match splitLast line.trim ':' with
      | some (target, hash) =>
        { state with hashes := hashInsert state.hashes target.trim hash.trim }
      | none => state
    else state
  else
    let state := { state with inHashes := false }
    match splitFirst line ':' with
    | none => state
    | some (key, value) =>
      if key.trim == "describes" then
        let value := value.trim
        if value.startsWith "[" && value.endsWith "]" then
          let inner := (value.drop 1).dropRight 1
          let items := ((inner.splitOn ",").map (fun item => stripQuotes item.trim)).filter
            (!·.isEmpty)
          { state with targets := state.targets ++ items }
        else if !value.isEmpty then
          { state with targets := state.targets ++ [stripQuotes value] }
        else state
      else if key.trim == "described_hash" then
        if value.trim.isEmpty then { state with inHashes := true }
        else if state.targets.length == 1 then
          { state with
            hashes := hashInsert state.hashes (state.targets.getD 0 "") value.trim }
        else state
      else state

/--
What a document declares: each target, with the hash recorded for it.

@models REQ-DOCLINK.declares_target
@models REQ-DOCLINK.records_hash
@models REQ-DOCLINK.decisions_exempt
-/
def declaredIn (lines : List String) : List (String × String) :=
  -- The argument is a document joined with newlines, so an element carrying a
  -- newline of its own is several lines.
  let lines := (String.intercalate "\n" lines).splitOn "\n"
  match docFrontmatter lines with
  | none => []
  | some frontmatter =>
    let state := frontmatter.foldl readDocLine {}
    state.targets.map (fun target =>
      (target, ((state.hashes.find? (·.1 == target)).map (·.2)).getD ""))

/-! ## Confirming

`confirmation_is_human`. A document returns to current only when a person has
read it against the changed code and re-recorded the hash. The model produces
the text to record; nothing in the system records it.
-/

/-- The frontmatter a document should carry, given what its targets hash to now.

A target nothing knows about is written as `?` rather than omitted: a person
re-recording needs to see that the thing being described has disappeared.

@models REQ-DOCLINK.confirmation_is_human -/
def recordedFrontmatter (targets : List String) (current : List (String × String)) :
    String :=
  let header := "describes: [" ++ String.intercalate ", " targets ++ "]\ndescribed_hash:\n"
  targets.foldl
    (fun out target =>
      let hash := ((current.find? (·.1 == target)).map (·.2)).getD "?"
      out ++ "  " ++ target ++ ": " ++ hash ++ "\n")
    header

/-- A decision record declares no target, so nothing about it goes into review.

@proves REQ-DOCLINK.decisions_exempt -/
theorem a_decision_record_declares_nothing :
    declaredIn ["---", "adr: 11", "affects: [REQ-X]", "---", "# A decision"] = [] := by
  native_decide

/-- Confirming produces text and changes nothing.

@proves REQ-DOCLINK.confirmation_is_human -/
theorem confirming_an_unknown_target_says_so :
    recordedFrontmatter ["a"] [] = "describes: [a]\ndescribed_hash:\n  a: ?\n" := by
  native_decide

end TraceLean.DocLink
