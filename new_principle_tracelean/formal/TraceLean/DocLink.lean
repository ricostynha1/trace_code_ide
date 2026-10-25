import Lean

namespace TraceLean.DocLink

open Lean (ToJson FromJson)

structure DocLink where
  file : String
  target : String
  recordedHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive DocState where
  | current
  | inReview (was now : String)
  | dangling
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def lookupHash (current : List (String × String)) (target : String) : Option String :=
  (current.find? (·.1 == target)).map (·.2)

/-- @models REQ-DOCLINK.hash_moves_review
@models REQ-DOCLINK.dangling_reported -/
def docState (link : DocLink) (current : List (String × String)) : DocState :=
  match lookupHash current link.target with
  | none => .dangling
  | some now => if now == link.recordedHash then .current else .inReview link.recordedHash now

/-- @models REQ-DOCLINK.review_is_not_error -/
def docBlocks : DocState → Bool
  | .dangling => true
  | _ => false

/-- @proves REQ-DOCLINK.review_is_not_error -/
theorem review_is_distinct_and_does_not_block (was now : String) (h : was ≠ now) :
    docState ⟨"d.md", "T", was⟩ [("T", now)] = .inReview was now ∧
    docBlocks (.inReview was now) = false := by
  constructor
  · simp [docState, lookupHash, beq_iff_eq, Ne.symm h]
  · rfl

/-- @proves REQ-DOCLINK.hash_moves_review -/
theorem unmoved_is_current (hash : String) :
    docState ⟨"d.md", "T", hash⟩ [("T", hash)] = .current := by
  simp [docState, lookupHash]

def stripQuotes (s : String) : String :=
  let isQuote (c : Char) := c == '"' || c == '\''
  let chars := s.toList.dropWhile isQuote
  String.mk (chars.reverse.dropWhile isQuote).reverse

def trimRight (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

def splitFirst (s : String) (sep : Char) : Option (String × String) :=
  match s.toList.findIdx? (· == sep) with
  | none => none
  | some i => some (String.mk (s.toList.take i), String.mk (s.toList.drop (i + 1)))

def splitLast (s : String) (sep : Char) : Option (String × String) :=
  let chars := s.toList
  match (chars.reverse.findIdx? (· == sep)) with
  | none => none
  | some j =>
    let i := chars.length - 1 - j
    some (String.mk (chars.take i), String.mk (chars.drop (i + 1)))

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

/-- @models REQ-DOCLINK.declares_target
@models REQ-DOCLINK.records_hash
@models REQ-DOCLINK.decisions_exempt -/
def declaredIn (lines : List String) : List (String × String) :=
  let lines := (String.intercalate "\n" lines).splitOn "\n"
  match docFrontmatter lines with
  | none => []
  | some frontmatter =>
    let state := frontmatter.foldl readDocLine {}
    state.targets.map (fun target =>
      (target, ((state.hashes.find? (·.1 == target)).map (·.2)).getD ""))

/-- @models REQ-DOCLINK.confirmation_is_human -/
def recordedFrontmatter (targets : List String) (current : List (String × String)) :
    String :=
  let header := "describes: [" ++ String.intercalate ", " targets ++ "]\ndescribed_hash:\n"
  targets.foldl
    (fun out target =>
      let hash := ((current.find? (·.1 == target)).map (·.2)).getD "?"
      out ++ "  " ++ target ++ ": " ++ hash ++ "\n")
    header

/-- @proves REQ-DOCLINK.decisions_exempt -/
theorem a_decision_record_declares_nothing :
    declaredIn ["---", "adr: 11", "affects: [REQ-X]", "---", "# A decision"] = [] := by
  native_decide

/-- @proves REQ-DOCLINK.confirmation_is_human -/
theorem confirming_an_unknown_target_says_so :
    recordedFrontmatter ["a"] [] = "describes: [a]\ndescribed_hash:\n  a: ?\n" := by
  native_decide

/-- @proves REQ-DOCLINK.declares_target -/
theorem a_document_declares_its_targets :
    (declaredIn ["---", "describes: [REQ-A, src/x.rs::f]", "---", "# Doc"]).map (·.1)
      = ["REQ-A", "src/x.rs::f"] := by
  native_decide

/-- @proves REQ-DOCLINK.records_hash -/
theorem a_document_records_each_targets_hash :
    declaredIn ["---", "describes: [REQ-A, REQ-B]", "described_hash:", "  REQ-A: 3c66", "---"]
      = [("REQ-A", "3c66"), ("REQ-B", "")] := by
  native_decide

end TraceLean.DocLink
