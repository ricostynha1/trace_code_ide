import Lean

namespace TraceLean.Requirement

open Lean (ToJson FromJson)

inductive Decomposition where
  | complete
  | «open»
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Status where
  | draft
  | approved
  | linked
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive FrontmatterKind where
  | indentedWithoutKey
  | notAKeyValue
  | emptyId
  | unclosedFrontmatter
  | unknownKey
  | duplicateClause
  | duplicateNarrowing
  | missingText
  | invalidKey
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Parsed where
  isRequirement : Bool := false
  id : String := ""
  title : String := ""
  refines : List String := []
  decomposition : Decomposition := .«open»
  status : Status := .draft
  clauses : List (String × String) := []
  narrowings : List (String × List (String × String)) := []
  addressable : List (Option String) := []
  problems : List (Nat × FrontmatterKind) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def trimQuotes (s : String) : String :=
  let isQuote (c : Char) := c == '"' || c == '\''
  let chars := s.toList.dropWhile isQuote
  String.mk (chars.reverse.dropWhile isQuote).reverse

def splitOnFirst (s : String) (sep : Char) : Option (String × String) :=
  let chars := s.toList
  match chars.findIdx? (· == sep) with
  | none => none
  | some i => some (String.mk (chars.take i), String.mk (chars.drop (i + 1)))

def trimEnd (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

def trimStart (s : String) : String :=
  String.mk (s.toList.dropWhile Char.isWhitespace)

def insertSorted (entries : List (String × String)) (key value : String) :
    List (String × String) :=
  ((entries.filter (·.1 != key)) ++ [(key, value)]).mergeSort (fun a b => a.1 ≤ b.1)

def lookup? (entries : List (String × String)) (key : String) : Option String :=
  (entries.find? (·.1 == key)).map (·.2)

def splitFrontmatter (lines : List String) : Option (List String × List String) :=
  match lines with
  | [] => none
  | first :: rest =>
    if first != "---" || rest.isEmpty then none
    else
      match rest.findIdx? (fun line => trimEnd line == "---") with
      | none => none
      | some i => some (rest.take i, rest.drop (i + 1))

def fenceUnclosed (lines : List String) : Bool :=
  match lines with
  | [] => false
  | first :: rest => first == "---" && !rest.isEmpty && (splitFrontmatter lines).isNone

def knownKeys : List String := ["id", "title", "refines", "status", "decomposition", "clauses"]

def validKey (key : String) : Bool :=
  !key.isEmpty && key.all (fun c => c.isAlphanum || c == '_')

def indentOf (line : String) : Nat :=
  (line.toList.takeWhile (fun c => c == ' ' || c == '\t')).length

structure Block where
  clause : String
  indent : Nat
  line : Nat
  hasText : Bool := false
  deriving Inhabited

structure Fields where
  scalars : List (String × String) := []
  lists : List (String × List String) := []
  maps : List (String × List (String × String)) := []
  narrowings : List (String × List (String × String)) := []
  unknown : List (Nat × String) := []
  problems : List (Nat × FrontmatterKind) := []
  current : Option String := none
  block : Option Block := none
  deriving Inhabited

def listInsert (entries : List (String × List String)) (key : String)
    (value : List String) : List (String × List String) :=
  (entries.filter (·.1 != key)) ++ [(key, value)]

def mapEntry (entries : List (String × List (String × String))) (key : String) :
    List (String × String) :=
  ((entries.find? (·.1 == key)).map (·.2)).getD []

def mapInsert (entries : List (String × List (String × String))) (key : String)
    (inner : List (String × String)) : List (String × List (String × String)) :=
  (entries.filter (·.1 != key)) ++ [(key, inner)]

def problem (fields : Fields) (lineNo : Nat) (kind : FrontmatterKind) : Fields :=
  { fields with problems := fields.problems ++ [(lineNo, kind)] }

def narrowingsOf (fields : Fields) (clause : String) : List (String × String) :=
  ((fields.narrowings.find? (·.1 == clause)).map (·.2)).getD []

def closeBlock (fields : Fields) : Fields :=
  match fields.block with
  | none => fields
  | some b =>
    let cleared := { fields with block := none }
    let text := (lookup? (mapEntry fields.maps "clauses") b.clause).getD ""
    if text.isEmpty then problem cleared b.line .missingText else cleared

private def setText (fields : Fields) (b : Block) (lineNo : Nat) (value : String) : Fields :=
  let noted := if b.hasText then problem fields lineNo .duplicateNarrowing else fields
  let entries := insertSorted (mapEntry noted.maps "clauses") b.clause value
  { noted with
    maps := mapInsert noted.maps "clauses" entries
    block := some { b with hasText := true } }

private def addNarrowing (fields : Fields) (b : Block) (lineNo : Nat) (key value : String) :
    Fields :=
  let existing := narrowingsOf fields b.clause
  let noted := if existing.any (·.1 == key) then problem fields lineNo .duplicateNarrowing else fields
  { noted with narrowings := mapInsert noted.narrowings b.clause (insertSorted existing key value) }

private def readNarrowing (fields : Fields) (b : Block) (lineNo : Nat) (line : String) : Fields :=
  match splitOnFirst line.trim ':' with
  | none => problem fields lineNo .notAKeyValue
  | some (rawKey, rawValue) =>
    let key := rawKey.trim
    let value := rawValue.trim
    let checked := if validKey key then fields else problem fields lineNo .invalidKey
    if key == "text" then setText checked b lineNo value
    else addNarrowing checked b lineNo key value

private def readClause (fields : Fields) (lineNo indent : Nat) (key value : String) : Fields :=
  let checked := if validKey key then fields else problem fields lineNo .invalidKey
  let existing := mapEntry checked.maps "clauses"
  let noted := if existing.any (·.1 == key) then problem checked lineNo .duplicateClause else checked
  let opened : Option Block :=
    if value.isEmpty then some { clause := key, indent := indent, line := lineNo } else none
  { noted with
    maps := mapInsert noted.maps "clauses" (insertSorted existing key value)
    narrowings := noted.narrowings.filter (·.1 != key)
    block := opened }

private def readEntry (fields : Fields) (mapKey : String) (lineNo : Nat) (line : String) :
    Fields :=
  match splitOnFirst line.trim ':' with
  | none => problem fields lineNo .notAKeyValue
  | some (k, v) =>
    if mapKey == "clauses" then readClause fields lineNo (indentOf line) k.trim v.trim
    else
      { fields with
        maps := mapInsert fields.maps mapKey (insertSorted (mapEntry fields.maps mapKey) k.trim v.trim) }

private def readNested (fields : Fields) (mapKey : String) (lineNo : Nat) (line : String) :
    Fields :=
  match fields.block with
  | some b =>
    if indentOf line > b.indent then readNarrowing fields b lineNo line
    else readEntry (closeBlock fields) mapKey lineNo line
  | none => readEntry fields mapKey lineNo line

private def readIndented (fields : Fields) (lineNo : Nat) (line : String) : Fields :=
  match fields.current with
  | none => problem fields lineNo .indentedWithoutKey
  | some mapKey => readNested fields mapKey lineNo line

private def noteUnknown (fields : Fields) (lineNo : Nat) (key : String) : Fields :=
  if knownKeys.contains key then fields
  else { fields with unknown := fields.unknown ++ [(lineNo, key)] }

private def readKeyValue (fields : Fields) (lineNo : Nat) (line : String) : Fields :=
  let cleared := { closeBlock fields with current := none }
  match splitOnFirst line ':' with
  | none => { cleared with problems := cleared.problems ++ [(lineNo, .notAKeyValue)] }
  | some (rawKey, rawValue) =>
    let key := rawKey.trim
    let value := rawValue.trim
    let cleared := noteUnknown cleared lineNo key
    match value.isEmpty with
    | true => { cleared with current := some key }
    | false =>
      match value.startsWith "[" && value.endsWith "]" with
      | true =>
        let inner := (value.drop 1).dropRight 1
        let items := (inner.splitOn ",").map (fun item => trimQuotes item.trim)
        { cleared with lists := listInsert cleared.lists key (items.filter (!·.isEmpty)) }
      | false =>
        { cleared with scalars := insertSorted cleared.scalars key (trimQuotes value) }

def readLine (fields : Fields) (lineNo : Nat) (raw : String) : Fields :=
  let line := trimEnd raw
  match line.trim.isEmpty || (trimStart line).startsWith "#" with
  | true => fields
  | false =>
    match line.startsWith " " || line.startsWith "\t" with
    | true => readIndented fields lineNo line
    | false => readKeyValue fields lineNo line

def readFrontmatter (lines : List String) : Fields :=
  closeBlock ((lines.enum).foldl (fun fields pair => readLine fields (pair.1 + 2) pair.2) {})

def unknownProblems (fields : Fields) (inRequirement : Bool) : List (Nat × FrontmatterKind) :=
  (fields.unknown.filter (fun u => inRequirement || knownKeys.contains u.2.toLower)).map
    (fun u => (u.1, FrontmatterKind.unknownKey))

def sortedNarrowings (fields : Fields) : List (String × List (String × String)) :=
  (fields.narrowings.filter (fun n => !n.2.isEmpty)).mergeSort (fun a b => a.1 ≤ b.1)

def firstHeading (body : List String) : Option String :=
  (body.find? (·.startsWith "# ")).map (fun line => (line.drop 2).trim)

private def decompositionOf (fields : Fields) : Decomposition :=
  match lookup? fields.scalars "decomposition" with
  | some d => if d.trim.toLower == "complete" then .complete else .«open»
  | none => .«open»

private def statusOf (fields : Fields) : Status :=
  match lookup? fields.scalars "status" with
  | some raw =>
    let s := raw.trim.toLower
    if s == "approved" then .approved else if s == "linked" then .linked else .draft
  | none => .draft

/-- @models REQ-REQDOC.id_is_identity
@models REQ-REQDOC.clauseless_uniform
@models REQ-REQDOC.decomposition_claimed
@models REQ-REQDOC.malformed_reported
@models REQ-REQDOC.narrowings_nest
@models REQ-REQDOC.clause_unique -/
def parseLines (lines : List String) : Parsed :=
  let lines := (String.intercalate "\n" lines).splitOn "\n"
  match splitFrontmatter lines with
  | none => if fenceUnclosed lines then { problems := [(1, .unclosedFrontmatter)] } else {}
  | some (frontmatter, body) =>
    let fields := readFrontmatter frontmatter
    match lookup? fields.scalars "id" with
    | none => { problems := fields.problems ++ unknownProblems fields false }
    | some id =>
      if id.isEmpty then
        { problems := fields.problems ++ unknownProblems fields false ++ [(1, .emptyId)] }
      else
        let clauses := mapEntry fields.maps "clauses"
        { isRequirement := true
          id := id
          title := (lookup? fields.scalars "title").getD ((firstHeading body).getD id)
          refines := (((fields.lists.find? (·.1 == "refines")).map (·.2)).getD [])
          decomposition := decompositionOf fields
          status := statusOf fields
          clauses := clauses
          narrowings := sortedNarrowings fields
          addressable :=
            if clauses.isEmpty then [none] else clauses.map (fun c => some c.1)
          problems := fields.problems ++ unknownProblems fields true }

/-- @proves REQ-REQDOC.malformed_reported -/
theorem prose_is_not_a_requirement :
    parseLines ["# Notes", "", "prose"] = {} := by
  native_decide

/-- @proves REQ-REQDOC.clauseless_uniform -/
theorem no_clauses_is_one_implicit_clause :
    (parseLines ["---", "id: REQ-X", "---", "# X"]).addressable = [none] := by
  native_decide

/-- @proves REQ-REQDOC.decomposition_claimed -/
theorem decomposition_defaults_to_open :
    (parseLines ["---", "id: REQ-X", "---"]).decomposition = Decomposition.«open» := by
  native_decide

/-- @proves REQ-REQDOC.malformed_reported -/
theorem an_unclosed_fence_is_reported :
    (parseLines ["---", "id: REQ-X"]).problems = [(1, FrontmatterKind.unclosedFrontmatter)] := by
  native_decide

/-- @proves REQ-REQDOC.narrowings_nest -/
theorem a_narrowing_is_not_addressable :
    (parseLines ["---", "id: REQ-X", "clauses:", "  c:", "    text: T", "    empty: E", "---"]).addressable
      = [some "c"] := by
  native_decide

/-- @proves REQ-REQDOC.clause_unique -/
theorem a_clause_declared_twice_is_reported :
    (parseLines ["---", "id: REQ-X", "clauses:", "  c: A", "  c: B", "---"]).problems
      = [(5, FrontmatterKind.duplicateClause)] := by
  native_decide

/-- @models REQ-REQDOC.id_unique -/
def duplicateIds (declared : List (String × String)) : List (String × String) :=
  (declared.foldl
    (fun (acc : List String × List (String × String)) entry =>
      let (seen, out) := acc
      if seen.contains entry.1 then (seen, out ++ [entry])
      else (seen ++ [entry.1], out))
    ([], [])).2

/-- @proves REQ-REQDOC.id_unique -/
theorem one_declaration_is_not_a_duplicate (id file : String) :
    duplicateIds [(id, file)] = [] := by
  simp [duplicateIds]

/-- @proves REQ-REQDOC.id_is_identity -/
theorem the_declared_id_is_the_identity :
    (parseLines ["---", "id: REQ-X", "---", "# REQ-Y"]).id = "REQ-X" ∧
    (parseLines ["---", "id: REQ-X", "---", "# REQ-Y"]).isRequirement = true ∧
    (parseLines ["---", "title: REQ-Y", "---", "# REQ-Y"]).isRequirement = false := by
  native_decide

end TraceLean.Requirement
