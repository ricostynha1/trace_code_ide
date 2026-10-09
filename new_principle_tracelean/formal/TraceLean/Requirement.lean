import Lean

/-!
# Requirement documents

Models `REQ-REQDOC`. A requirement is identified by a declared `id`, never by
its filename — so parsing takes the text and nothing else, and a model that
agreed with an implementation which had started consulting the path would be
impossible to write.

The frontmatter grammar is deliberately tiny: `key: value`, `key: [a, b]`, one
level of nesting under a bare key, and under `clauses:` a clause written as a
block of `text:` and narrowings (ADR-0016). There is no YAML dependency, which
is what makes the failure modes small enough to enumerate.
-/

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

/-- What is wrong with a line of frontmatter. -/
inductive FrontmatterKind where
  /-- An indented entry with no map key above it. -/
  | indentedWithoutKey
  /-- A line that is not `key: value`. -/
  | notAKeyValue
  /-- `id:` is present and empty, so the document claims to be a requirement
  and does not say which one. -/
  | emptyId
  /-- The opening `---` is never closed, so nothing in the document is read. -/
  | unclosedFrontmatter
  /-- A top-level key the format does not have: in a requirement, or one that
  differs from a requirement key only in case. -/
  | unknownKey
  /-- A clause key declared twice in one document. -/
  | duplicateClause
  /-- A narrowing, or `text`, given twice in one clause's block. -/
  | duplicateNarrowing
  /-- A clause written as a block with no, or an empty, `text:`. -/
  | missingText
  /-- A clause or narrowing key outside `[A-Za-z0-9_]`. -/
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
  /-- Each clause that has narrowings, with them, both sorted by key. -/
  narrowings : List (String × List (String × String)) := []
  /-- The clauses a link may attach to: the declared keys, or one implicit
  clause when there are none. Never a narrowing. -/
  addressable : List (Option String) := []
  /-- Line and kind only. Two implementations cannot be expected to phrase a
  complaint the same way, and the phrasing is not the claim. -/
  problems : List (Nat × FrontmatterKind) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-! ## Small string helpers

Written out rather than reached for, because the exact trimming rules are part
of the grammar: a value is trimmed and then stripped of one layer of matching
quotes, and nothing else happens to it.
-/

/-- Strip every leading and trailing `"` or `'`. -/
def trimQuotes (s : String) : String :=
  let isQuote (c : Char) := c == '"' || c == '\''
  let chars := s.toList.dropWhile isQuote
  String.mk (chars.reverse.dropWhile isQuote).reverse

/-- Split at the first `:`, if there is one. -/
def splitOnFirst (s : String) (sep : Char) : Option (String × String) :=
  let chars := s.toList
  match chars.findIdx? (· == sep) with
  | none => none
  | some i => some (String.mk (chars.take i), String.mk (chars.drop (i + 1)))

/-- Rust's `trim_end`: trailing whitespace only. -/
def trimEnd (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

def trimStart (s : String) : String :=
  String.mk (s.toList.dropWhile Char.isWhitespace)

/-- Association-list insert, later wins, kept sorted by key — which is what a
map does, and what the implementation's `BTreeMap` does for free. -/
def insertSorted (entries : List (String × String)) (key value : String) :
    List (String × String) :=
  ((entries.filter (·.1 != key)) ++ [(key, value)]).mergeSort (fun a b => a.1 ≤ b.1)

def lookup? (entries : List (String × String)) (key : String) : Option String :=
  (entries.find? (·.1 == key)).map (·.2)

/-! ## The frontmatter fence

A document is a candidate only if its first line is `---` and a later line is
`---` on its own. Anything else is ordinary markdown, and ordinary markdown is
not a malformed requirement.
-/

/-- The frontmatter lines and the body lines, if the fence is well-formed. -/
def splitFrontmatter (lines : List String) : Option (List String × List String) :=
  match lines with
  | [] => none
  | first :: rest =>
    if first != "---" || rest.isEmpty then none
    else
      match rest.findIdx? (fun line => trimEnd line == "---") with
      | none => none
      | some i => some (rest.take i, rest.drop (i + 1))

/-- A document that opens a fence and never closes it. Ordinary markdown does
not open with `---`; one that does and reads as nothing is the silent skip
`malformed_reported` forbids. -/
def fenceUnclosed (lines : List String) : Bool :=
  match lines with
  | [] => false
  | first :: rest => first == "---" && !rest.isEmpty && (splitFrontmatter lines).isNone

/-! ## The frontmatter grammar -/

/-- The top-level keys of a requirement's frontmatter. -/
def knownKeys : List String := ["id", "title", "refines", "status", "decomposition", "clauses"]

/-- Whether a clause or narrowing key can be named by an annotation. -/
def validKey (key : String) : Bool :=
  !key.isEmpty && key.all (fun c => c.isAlphanum || c == '_')

def indentOf (line : String) : Nat :=
  (line.toList.takeWhile (fun c => c == ' ' || c == '\t')).length

/-- A clause being read as a block: its key, the indentation of its line, and
that line, so a block missing its `text:` is reported where it starts. -/
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
  /-- Clause key to its narrowings. -/
  narrowings : List (String × List (String × String)) := []
  /-- Top-level keys the format does not have, with their lines; whether each
  is reported is decided once the document is known to be a requirement. -/
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

/-- End the block being read, reporting it if it never said what the clause
is. -/
def closeBlock (fields : Fields) : Fields :=
  match fields.block with
  | none => fields
  | some b =>
    let cleared := { fields with block := none }
    let text := (lookup? (mapEntry fields.maps "clauses") b.clause).getD ""
    if text.isEmpty then problem cleared b.line .missingText else cleared

/-- A block's `text:` line: the clause itself. -/
private def setText (fields : Fields) (b : Block) (lineNo : Nat) (value : String) : Fields :=
  let noted := if b.hasText then problem fields lineNo .duplicateNarrowing else fields
  let entries := insertSorted (mapEntry noted.maps "clauses") b.clause value
  { noted with
    maps := mapInsert noted.maps "clauses" entries
    block := some { b with hasText := true } }

/-- A block's other lines: narrowings of the clause. -/
private def addNarrowing (fields : Fields) (b : Block) (lineNo : Nat) (key value : String) :
    Fields :=
  let existing := narrowingsOf fields b.clause
  let noted := if existing.any (·.1 == key) then problem fields lineNo .duplicateNarrowing else fields
  { noted with narrowings := mapInsert noted.narrowings b.clause (insertSorted existing key value) }

/-- One line inside a clause's block. -/
private def readNarrowing (fields : Fields) (b : Block) (lineNo : Nat) (line : String) : Fields :=
  match splitOnFirst line.trim ':' with
  | none => problem fields lineNo .notAKeyValue
  | some (rawKey, rawValue) =>
    let key := rawKey.trim
    let value := rawValue.trim
    let checked := if validKey key then fields else problem fields lineNo .invalidKey
    if key == "text" then setText checked b lineNo value
    else addNarrowing checked b lineNo key value

/-- One entry under `clauses:`. With no value it opens a block; declared again
it replaces the earlier declaration whole, narrowings too, and is reported. -/
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

/-- One entry under a top-level map key. -/
private def readEntry (fields : Fields) (mapKey : String) (lineNo : Nat) (line : String) :
    Fields :=
  match splitOnFirst line.trim ':' with
  | none => problem fields lineNo .notAKeyValue
  | some (k, v) =>
    if mapKey == "clauses" then readClause fields lineNo (indentOf line) k.trim v.trim
    else
      { fields with
        maps := mapInsert fields.maps mapKey (insertSorted (mapEntry fields.maps mapKey) k.trim v.trim) }

/-- An indented line: inside the open block if deeper than its clause, else an
entry of the map above. -/
private def readNested (fields : Fields) (mapKey : String) (lineNo : Nat) (line : String) :
    Fields :=
  match fields.block with
  | some b =>
    if indentOf line > b.indent then readNarrowing fields b lineNo line
    else readEntry (closeBlock fields) mapKey lineNo line
  | none => readEntry fields mapKey lineNo line

/-- Read one line of frontmatter into the fields so far. -/
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

/-- One frontmatter line, read into the fields so far.

Split into three definitions rather than one nest of `if`s and `let`s: the
grammar that reads these annotations cannot follow a `let` inside a branch of an
`if` whose other branch also binds one (ADR-0008). -/
def readLine (fields : Fields) (lineNo : Nat) (raw : String) : Fields :=
  let line := trimEnd raw
  match line.trim.isEmpty || (trimStart line).startsWith "#" with
  | true => fields
  | false =>
    match line.startsWith " " || line.startsWith "\t" with
    | true => readIndented fields lineNo line
    | false => readKeyValue fields lineNo line

/-- Every frontmatter line, in order. Line numbers are file lines: one for the
opening fence, one for counting from one. -/
def readFrontmatter (lines : List String) : Fields :=
  closeBlock ((lines.enum).foldl (fun fields pair => readLine fields (pair.1 + 2) pair.2) {})

/-- The unknown keys to report: all of them in a requirement, and otherwise
those that are a requirement key misspelt in case (`iD:`), which is how a
document stops being a requirement without anyone noticing. -/
def unknownProblems (fields : Fields) (inRequirement : Bool) : List (Nat × FrontmatterKind) :=
  (fields.unknown.filter (fun u => inRequirement || knownKeys.contains u.2.toLower)).map
    (fun u => (u.1, FrontmatterKind.unknownKey))

/-- Each clause that has narrowings, sorted by clause. -/
def sortedNarrowings (fields : Fields) : List (String × List (String × String)) :=
  (fields.narrowings.filter (fun n => !n.2.isEmpty)).mergeSort (fun a b => a.1 ≤ b.1)

def firstHeading (body : List String) : Option String :=
  (body.find? (·.startsWith "# ")).map (fun line => (line.drop 2).trim)

/-- How much of a requirement's decomposition its author says is finished.

A definition rather than a `match` written as a field value, which the grammar
that reads these annotations cannot follow (ADR-0008). -/
private def decompositionOf (fields : Fields) : Decomposition :=
  match lookup? fields.scalars "decomposition" with
  | some d => if d.trim.toLower == "complete" then .complete else .«open»
  | none => .«open»

/-- Where a requirement stands. Anything unrecognised is a draft. -/
private def statusOf (fields : Fields) : Status :=
  match lookup? fields.scalars "status" with
  | some raw =>
    let s := raw.trim.toLower
    if s == "approved" then .approved else if s == "linked" then .linked else .draft
  | none => .draft

/--
Parse a document given as lines.

@models REQ-REQDOC.id_is_identity
@models REQ-REQDOC.clauseless_uniform
@models REQ-REQDOC.decomposition_claimed
@models REQ-REQDOC.malformed_reported
@models REQ-REQDOC.narrowings_nest
@models REQ-REQDOC.clause_unique
-/
def parseLines (lines : List String) : Parsed :=
  -- The argument is a document, joined with newlines. An element carrying a
  -- newline of its own is therefore several lines, which is how a generator
  -- produces a whole well-formed document as one draw.
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

/-- Ordinary markdown is not a malformed requirement.

@proves REQ-REQDOC.malformed_reported -/
theorem prose_is_not_a_requirement :
    parseLines ["# Notes", "", "prose"] = {} := by
  native_decide

/-- A requirement declaring no clauses is still addressable, as one implicit
clause — so a link to the requirement itself has somewhere to attach.

@proves REQ-REQDOC.clauseless_uniform -/
theorem no_clauses_is_one_implicit_clause :
    (parseLines ["---", "id: REQ-X", "---", "# X"]).addressable = [none] := by
  native_decide

/-- Completeness is claimed, never assumed.

@proves REQ-REQDOC.decomposition_claimed -/
theorem decomposition_defaults_to_open :
    (parseLines ["---", "id: REQ-X", "---"]).decomposition = Decomposition.«open» := by
  native_decide

/-- A fence opened and never closed is reported, not read as nothing.

@proves REQ-REQDOC.malformed_reported -/
theorem an_unclosed_fence_is_reported :
    (parseLines ["---", "id: REQ-X"]).problems = [(1, FrontmatterKind.unclosedFrontmatter)] := by
  native_decide

/-- A narrowing is part of its clause and never a clause a link can name.

@proves REQ-REQDOC.narrowings_nest -/
theorem a_narrowing_is_not_addressable :
    (parseLines ["---", "id: REQ-X", "clauses:", "  c:", "    text: T", "    empty: E", "---"]).addressable
      = [some "c"] := by
  native_decide

/-- A clause key declared twice is reported where it is declared again.

@proves REQ-REQDOC.clause_unique -/
theorem a_clause_declared_twice_is_reported :
    (parseLines ["---", "id: REQ-X", "clauses:", "  c: A", "  c: B", "---"]).problems
      = [(5, FrontmatterKind.duplicateClause)] := by
  native_decide

/-! ## Identity

`id_unique`. Identity is the declared `id`, so two documents declaring one are
the same requirement said twice — a fault whichever file it is in. The first
declaration wins and the later ones are reported, so the file named is the one
being ignored.
-/

/-- Identifiers declared more than once, each with the file of the later
declaration.

@models REQ-REQDOC.id_unique -/
def duplicateIds (declared : List (String × String)) : List (String × String) :=
  (declared.foldl
    (fun (acc : List String × List (String × String)) entry =>
      let (seen, out) := acc
      if seen.contains entry.1 then (seen, out ++ [entry])
      else (seen ++ [entry.1], out))
    ([], [])).2

/-- One declaration is never a duplicate of itself.

@proves REQ-REQDOC.id_unique -/
theorem one_declaration_is_not_a_duplicate (id file : String) :
    duplicateIds [(id, file)] = [] := by
  simp [duplicateIds]

end TraceLean.Requirement
