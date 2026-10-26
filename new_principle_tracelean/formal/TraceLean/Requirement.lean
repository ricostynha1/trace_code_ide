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

def Grows (f g : Fields) : Prop := ∃ e, g.problems = f.problems ++ e

theorem grows_refl (f : Fields) : Grows f f := ⟨[], by simp⟩

theorem grows_trans {f g h : Fields} (a : Grows f g) (b : Grows g h) : Grows f h := by
  obtain ⟨e1, h1⟩ := a
  obtain ⟨e2, h2⟩ := b
  exact ⟨e1 ++ e2, by rw [h2, h1, List.append_assoc]⟩

theorem grows_problem (f : Fields) (n : Nat) (k : FrontmatterKind) : Grows f (problem f n k) :=
  ⟨[(n, k)], rfl⟩

theorem grows_closeBlock (f : Fields) : Grows f (closeBlock f) := by
  unfold closeBlock Grows
  split
  · exact ⟨[], by simp⟩
  · dsimp only
    split <;> simp [problem]


theorem grows_setText (f : Fields) (b : Block) (n : Nat) (v : String) : Grows f (setText f b n v) := by
  unfold setText Grows
  dsimp only
  split <;> simp [problem]


theorem grows_addNarrowing (f : Fields) (b : Block) (n : Nat) (k v : String) :
    Grows f (addNarrowing f b n k v) := by
  unfold addNarrowing Grows
  dsimp only
  split <;> simp [problem]


theorem grows_ite_problem (c : Bool) (f : Fields) (n : Nat) (k : FrontmatterKind) :
    Grows f (if c = true then f else problem f n k) := by
  split
  · exact grows_refl _
  · exact grows_problem _ _ _

theorem grows_readNarrowing (f : Fields) (b : Block) (n : Nat) (line : String) :
    Grows f (readNarrowing f b n line) := by
  unfold readNarrowing
  split
  · exact grows_problem _ _ _
  · dsimp only
    split
    · exact grows_trans (grows_ite_problem _ _ _ _) (grows_setText _ _ _ _)
    · exact grows_trans (grows_ite_problem _ _ _ _) (grows_addNarrowing _ _ _ _ _)

theorem grows_readClause (f : Fields) (n i : Nat) (k v : String) :
    Grows f (readClause f n i k v) := by
  unfold readClause Grows
  dsimp only
  split <;> split <;> simp [problem]


theorem grows_readEntry (f : Fields) (m : String) (n : Nat) (line : String) :
    Grows f (readEntry f m n line) := by
  unfold readEntry
  split
  · exact grows_problem _ _ _
  · split
    · exact grows_readClause _ _ _ _ _
    · exact ⟨[], by simp⟩

theorem grows_readLine (f : Fields) (n : Nat) (raw : String) : Grows f (readLine f n raw) := by
  unfold readLine
  dsimp only
  split
  · exact grows_refl _
  · split
    · unfold readIndented
      split
      · exact grows_problem _ _ _
      · unfold readNested
        split
        · split
          · exact grows_readNarrowing _ _ _ _
          · exact grows_trans (grows_closeBlock _) (grows_readEntry _ _ _ _)
        · exact grows_readEntry _ _ _ _
    · unfold readKeyValue
      dsimp only
      have g0 : Grows f { closeBlock f with current := none } := by
        obtain ⟨e, he⟩ := grows_closeBlock f
        exact ⟨e, he⟩
      split
      · exact grows_trans g0 ⟨_, rfl⟩
      · have g1 : ∀ (g : Fields) (n : Nat) (k : String), Grows g (noteUnknown g n k) := by
          intro g n k
          unfold noteUnknown
          split
          · exact grows_refl _
          · exact ⟨[], by simp⟩
        rename_i rawKey rawValue _
        obtain ⟨e, he⟩ := grows_trans g0 (g1 { closeBlock f with current := none } n rawKey.trim)
        split
        · exact ⟨e, he⟩
        · split
          · exact ⟨e, he⟩
          · exact ⟨e, he⟩

theorem grows_fold (xs : List (Nat × String)) : ∀ f : Fields,
    Grows f (xs.foldl (fun fields pair => readLine fields (pair.1 + 2) pair.2) f) := by
  induction xs with
  | nil => intro f; exact grows_refl f
  | cons x rest ih =>
    intro f
    exact grows_trans (grows_readLine _ _ _) (ih _)

def resplit (lines : List String) : List String := (String.intercalate "\n" lines).splitOn "\n"

theorem parse_unsplit (lines : List String) (hs : splitFrontmatter (resplit lines) = none) :
    parseLines lines =
      if fenceUnclosed (resplit lines) then { problems := [(1, .unclosedFrontmatter)] } else {} := by
  simp only [resplit] at hs
  simp only [parseLines, hs, resplit]

theorem parse_split (lines : List String) (fm body : List String)
    (hs : splitFrontmatter (resplit lines) = some (fm, body)) :
    parseLines lines =
      match lookup? (readFrontmatter fm).scalars "id" with
      | none => { problems := (readFrontmatter fm).problems ++ unknownProblems (readFrontmatter fm) false }
      | some id =>
        if id.isEmpty then
          { problems := (readFrontmatter fm).problems ++ unknownProblems (readFrontmatter fm) false ++
              [(1, .emptyId)] }
        else
          let fields := readFrontmatter fm
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
            problems := fields.problems ++ unknownProblems fields true } := by
  simp only [resplit] at hs
  simp only [parseLines, hs]

theorem parse_requirement (lines : List String) (h : (parseLines lines).isRequirement = true) :
    ∃ fm body id, splitFrontmatter (resplit lines) = some (fm, body) ∧
      lookup? (readFrontmatter fm).scalars "id" = some id ∧ id.isEmpty = false ∧
      (parseLines lines).id = id ∧
      (parseLines lines).decomposition = decompositionOf (readFrontmatter fm) ∧
      (parseLines lines).clauses = mapEntry (readFrontmatter fm).maps "clauses" ∧
      (parseLines lines).narrowings = sortedNarrowings (readFrontmatter fm) ∧
      (parseLines lines).addressable =
        (if (mapEntry (readFrontmatter fm).maps "clauses").isEmpty then [none]
          else (mapEntry (readFrontmatter fm).maps "clauses").map (fun c => some c.1)) ∧
      (parseLines lines).problems = (readFrontmatter fm).problems ++
        unknownProblems (readFrontmatter fm) true := by
  cases hs : splitFrontmatter (resplit lines) with
  | none =>
    rw [parse_unsplit lines hs] at h
    split at h <;> simp at h
  | some pair =>
    obtain ⟨fm, body⟩ := pair
    rw [parse_split lines fm body hs] at h ⊢
    refine ⟨fm, body, ?_⟩
    cases hid : lookup? (readFrontmatter fm).scalars "id" with
    | none =>
      rw [hid] at h
      simp at h
    | some id =>
      rw [hid] at h
      simp only [hid]
      cases he : id.isEmpty
      · simp [he]
      · simp [he] at h

theorem parse_not_requirement (lines : List String) (h : (parseLines lines).isRequirement = false) :
    (parseLines lines).decomposition = .«open» ∧ (parseLines lines).clauses = [] ∧
      (parseLines lines).narrowings = [] ∧ (parseLines lines).addressable = [] := by
  cases hs : splitFrontmatter (resplit lines) with
  | none =>
    rw [parse_unsplit lines hs]
    split <;> simp
  | some pair =>
    obtain ⟨fm, body⟩ := pair
    rw [parse_split lines fm body hs] at h ⊢
    cases hid : lookup? (readFrontmatter fm).scalars "id" with
    | none => simp
    | some id =>
      rw [hid] at h
      simp only [hid]
      cases he : id.isEmpty
      · simp [he] at h
      · simp

/-- @proves REQ-REQDOC.id_is_identity -/
theorem a_requirement_is_its_declared_id (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    ∃ fm body, splitFrontmatter (resplit lines) = some (fm, body) ∧
      lookup? (readFrontmatter fm).scalars "id" = some (parseLines lines).id ∧
      (parseLines lines).id ≠ "" := by
  obtain ⟨fm, body, id, hs, hid, he, hpid, _⟩ := parse_requirement lines h
  refine ⟨fm, body, hs, by rw [hpid, hid], ?_⟩
  rw [hpid]
  intro e
  rw [e] at he
  simp [String.isEmpty] at he
  exact he rfl

/-- @proves REQ-REQDOC.clauseless_uniform -/
theorem a_clauseless_requirement_is_one_clause (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    ((parseLines lines).clauses = [] → (parseLines lines).addressable = [none]) ∧
    ((parseLines lines).clauses ≠ [] →
      (parseLines lines).addressable = (parseLines lines).clauses.map (fun c => some c.1)) := by
  obtain ⟨fm, body, id, _, _, _, _, _, hc, _, ha, _⟩ := parse_requirement lines h
  rw [ha, hc]
  constructor
  · intro e
    simp [e]
  · intro e
    have : (mapEntry (readFrontmatter fm).maps "clauses").isEmpty = false := by
      cases hm : mapEntry (readFrontmatter fm).maps "clauses"
      · exact absurd hm e
      · rfl
    simp [this]

/-- @proves REQ-REQDOC.decomposition_claimed -/
theorem completeness_is_only_ever_claimed (lines : List String) :
    (parseLines lines).decomposition = .complete →
      ∃ fm body d, splitFrontmatter (resplit lines) = some (fm, body) ∧
        lookup? (readFrontmatter fm).scalars "decomposition" = some d ∧
        d.trim.toLower = "complete" := by
  intro hc
  cases hr : (parseLines lines).isRequirement
  · rw [(parse_not_requirement lines hr).1] at hc
    cases hc
  · obtain ⟨fm, body, id, hs, _, _, _, hd, _⟩ := parse_requirement lines hr
    rw [hd] at hc
    unfold decompositionOf at hc
    split at hc
    · rename_i d hd'
      split at hc
      · rename_i ht
        exact ⟨fm, body, d, hs, hd', by simpa using ht⟩
      · cases hc
    · cases hc

/-- @proves REQ-REQDOC.malformed_reported -/
theorem a_malformed_document_is_reported (lines fm body : List String) :
    (splitFrontmatter (resplit lines) = none → fenceUnclosed (resplit lines) = true →
      (1, FrontmatterKind.unclosedFrontmatter) ∈ (parseLines lines).problems) ∧
    (splitFrontmatter (resplit lines) = some (fm, body) →
      ∀ x ∈ (readFrontmatter fm).problems, x ∈ (parseLines lines).problems) ∧
    ((parseLines lines).isRequirement = true → splitFrontmatter (resplit lines) = some (fm, body) →
      ∀ u ∈ (readFrontmatter fm).unknown, (u.1, FrontmatterKind.unknownKey) ∈ (parseLines lines).problems) ∧
    (∀ (f : Fields) (b : Block), f.block = some b →
      (lookup? (mapEntry f.maps "clauses") b.clause).getD "" = "" →
      (b.line, FrontmatterKind.missingText) ∈ (closeBlock f).problems) ∧
    (∀ (f : Fields) (n : Nat) (raw : String), Grows f (readLine f n raw)) := by
  refine ⟨?_, ?_, ?_, ?_, grows_readLine⟩
  · intro hs hu
    rw [parse_unsplit lines hs, if_pos hu]
    simp
  · intro hs x hx
    rw [parse_split lines fm body hs]
    cases lookup? (readFrontmatter fm).scalars "id" with
    | none => exact List.mem_append_left _ hx
    | some id =>
      dsimp only
      split
      · exact List.mem_append_left _ (List.mem_append_left _ hx)
      · exact List.mem_append_left _ hx
  · intro hr hs u hu
    obtain ⟨fm', body', _, hs', _, _, _, _, _, _, _, hp⟩ := parse_requirement lines hr
    rw [hs] at hs'
    cases hs'
    rw [hp]
    apply List.mem_append_right
    simp only [unknownProblems, List.mem_map, List.mem_filter, Bool.true_or, and_true]
    exact ⟨u, hu, rfl⟩
  · intro f b hb ht
    unfold closeBlock
    rw [hb]
    dsimp only
    rw [ht]
    have he : "".isEmpty = true := rfl
    rw [if_pos he]
    simp [problem]

/-- @proves REQ-REQDOC.clause_unique -/
theorem a_repeated_clause_or_narrowing_is_reported (f : Fields) (n : Nat) (raw k v : String)
    (hblank : ((trimEnd raw).trim.isEmpty || (trimStart (trimEnd raw)).startsWith "#") = false)
    (hind : ((trimEnd raw).startsWith " " || (trimEnd raw).startsWith "\t") = true)
    (hcur : f.current = some "clauses")
    (hsplit : splitOnFirst (trimEnd raw).trim ':' = some (k, v)) :
    ((∀ b, f.block = some b → ¬ indentOf (trimEnd raw) > b.indent) →
      (mapEntry f.maps "clauses").any (·.1 == k.trim) = true →
      (n, FrontmatterKind.duplicateClause) ∈ (readLine f n raw).problems) ∧
    (∀ b, f.block = some b → indentOf (trimEnd raw) > b.indent → k.trim ≠ "text" →
      (narrowingsOf f b.clause).any (·.1 == k.trim) = true →
      (n, FrontmatterKind.duplicateNarrowing) ∈ (readLine f n raw).problems) ∧
    (∀ b, f.block = some b → indentOf (trimEnd raw) > b.indent → k.trim = "text" →
      b.hasText = true →
      (n, FrontmatterKind.duplicateNarrowing) ∈ (readLine f n raw).problems) := by
  have base : readLine f n raw = readIndented f n (trimEnd raw) := by
    unfold readLine
    simp only [hblank, hind]
  rw [base]
  unfold readIndented
  rw [hcur]
  dsimp only
  refine ⟨?_, ?_, ?_⟩
  · intro shallow dup
    have hentry : ∀ g : Fields, g.maps = f.maps →
        (n, FrontmatterKind.duplicateClause) ∈ (readEntry g "clauses" n (trimEnd raw)).problems := by
      intro g hg
      unfold readEntry
      rw [hsplit]
      simp only [beq_self_eq_true, if_true]
      unfold readClause
      dsimp only
      split <;> simp [problem, hg, dup]
    unfold readNested
    cases hb : f.block with
    | none => exact hentry f rfl
    | some b =>
      simp only [shallow b hb, if_false]
      apply hentry
      unfold closeBlock
      rw [hb]
      dsimp only
      split <;> rfl
  · intro b hb deep notText dup
    unfold readNested
    rw [hb]
    simp only [deep, if_true]
    unfold readNarrowing
    rw [hsplit]
    dsimp only
    have hnt : (k.trim == "text") = false := by simpa using notText
    simp only [hnt, Bool.false_eq_true, if_false]
    unfold addNarrowing
    dsimp only
    split
    · rename_i hv
      have hd : ((narrowingsOf f b.clause).any fun x => x.fst == k.trim) = true := dup
      simp only [narrowingsOf] at hd ⊢
      simp [hd, problem]
    · have hd : ((narrowingsOf f b.clause).any fun x => x.fst == k.trim) = true := dup
      simp only [narrowingsOf, problem] at hd ⊢
      simp [hd]
  · intro b hb deep isText had
    unfold readNested
    rw [hb]
    simp only [deep, if_true]
    unfold readNarrowing
    rw [hsplit]
    dsimp only
    simp only [isText, beq_self_eq_true, if_true]
    unfold setText
    dsimp only
    simp [had, problem]

def clauseKeys (f : Fields) : List String := (mapEntry f.maps "clauses").map (·.1)

def Nested (f : Fields) : Prop :=
  (∀ x ∈ f.narrowings, x.1 ∈ clauseKeys f) ∧ (∀ b, f.block = some b → b.clause ∈ clauseKeys f)

theorem find_filter_other {β : Type} (m : List (String × β)) (k k' : String) (h : k ≠ k') :
    (m.filter (·.1 != k)).find? (·.1 == k') = m.find? (·.1 == k') := by
  induction m with
  | nil => rfl
  | cons x rest ih =>
    by_cases hx : x.1 = k
    · have h1 : (x.1 != k) = false := by simp [hx]
      have h2 : (x.1 == k') = false := by
        simp only [beq_eq_false_iff_ne, ne_eq]
        rw [hx]
        exact h
      simp [List.filter_cons, h1, List.find?_cons, h2, ih]
    · have h1 : (x.1 != k) = true := by simp [hx]
      simp only [List.filter_cons, h1, if_true, List.find?_cons]
      split <;> simp_all

theorem mapEntry_insert_same (m : List (String × List (String × String))) (k : String)
    (inner : List (String × String)) : mapEntry (mapInsert m k inner) k = inner := by
  have hn : (m.filter (·.1 != k)).find? (·.1 == k) = none := by
    rw [List.find?_eq_none]
    intro x hx
    simp only [List.mem_filter, bne_iff_ne, ne_eq] at hx
    simpa using hx.2
  simp [mapEntry, mapInsert, List.find?_append, hn]

theorem mapEntry_insert_other (m : List (String × List (String × String))) (k k' : String)
    (inner : List (String × String)) (h : k ≠ k') :
    mapEntry (mapInsert m k inner) k' = mapEntry m k' := by
  have h2 : ((k, inner).1 == k') = false := by simpa using h
  unfold mapEntry mapInsert
  rw [List.find?_append, find_filter_other m k k' h]
  cases m.find? (·.1 == k') with
  | none => simp [List.find?, h2]
  | some x => rfl

theorem keys_insertSorted (e : List (String × String)) (k v x : String) :
    x ∈ (insertSorted e k v).map (·.1) ↔ x = k ∨ x ∈ e.map (·.1) := by
  simp only [insertSorted, List.mem_map, List.mem_mergeSort, List.mem_append, List.mem_filter,
    List.mem_singleton, bne_iff_ne, ne_eq]
  constructor
  · intro ⟨y, hy, hyx⟩
    rcases hy with hy | hy
    · exact Or.inr ⟨y, hy.1, hyx⟩
    · subst hy
      exact Or.inl hyx.symm
  · intro h
    rcases h with h | ⟨y, hy, hyx⟩
    · exact ⟨(k, v), Or.inr rfl, h.symm⟩
    · by_cases hk : y.1 = k
      · exact ⟨(k, v), Or.inr rfl, hk ▸ hyx⟩
      · exact ⟨y, Or.inl ⟨hy, hk⟩, hyx⟩

theorem nested_closeBlock (f : Fields) (h : Nested f) : Nested (closeBlock f) := by
  unfold closeBlock
  split
  · exact h
  · dsimp only
    split
    · exact ⟨h.1, fun b hb => by simp [problem] at hb⟩
    · exact ⟨h.1, fun b hb => by simp at hb⟩

theorem nested_setText (f : Fields) (b : Block) (n : Nat) (v : String) (h : Nested f)
    (hb : b.clause ∈ clauseKeys f) : Nested (setText f b n v) := by
  have grow : ∀ x, x ∈ clauseKeys f → x ∈ clauseKeys (setText f b n v) := by
    intro x hx
    unfold setText
    dsimp only
    simp only [clauseKeys, mapEntry_insert_same]
    rw [keys_insertSorted]
    right
    split <;> exact hx
  refine ⟨fun x hx => grow _ (h.1 x ?_), fun b' hb' => ?_⟩
  · unfold setText at hx
    dsimp only at hx
    split at hx <;> exact hx
  · unfold setText at hb'
    dsimp only at hb'
    simp only [Option.some.injEq] at hb'
    subst hb'
    exact grow _ hb

theorem nested_addNarrowing (f : Fields) (b : Block) (n : Nat) (k v : String) (h : Nested f)
    (hb : b.clause ∈ clauseKeys f) : Nested (addNarrowing f b n k v) := by
  have same : clauseKeys (addNarrowing f b n k v) = clauseKeys f := by
    unfold addNarrowing
    dsimp only
    split <;> rfl
  have blk : (addNarrowing f b n k v).block = f.block := by
    unfold addNarrowing
    dsimp only
    split <;> rfl
  rw [Nested, same, blk]
  refine ⟨fun x hx => ?_, h.2⟩
  unfold addNarrowing at hx
  dsimp only at hx
  have hx' : x ∈ mapInsert f.narrowings b.clause (insertSorted (narrowingsOf f b.clause) k v) := by
    split at hx <;> exact hx
  simp only [mapInsert, List.mem_append, List.mem_filter, List.mem_singleton] at hx'
  rcases hx' with hx' | hx'
  · exact h.1 x hx'.1
  · rw [hx']
    exact hb

theorem nested_readNarrowing (f : Fields) (b : Block) (n : Nat) (line : String) (h : Nested f)
    (hb : b.clause ∈ clauseKeys f) : Nested (readNarrowing f b n line) := by
  unfold readNarrowing
  split
  · exact h
  · dsimp only
    have hc : ∀ c : Bool, Nested (if c = true then f else problem f n .invalidKey) ∧
        b.clause ∈ clauseKeys (if c = true then f else problem f n .invalidKey) := by
      intro c
      split
      · exact ⟨h, hb⟩
      · exact ⟨h, hb⟩
    split
    · exact nested_setText _ _ _ _ (hc _).1 (hc _).2
    · exact nested_addNarrowing _ _ _ _ _ (hc _).1 (hc _).2

theorem nested_readClause (f : Fields) (n i : Nat) (k v : String) (h : Nested f) :
    Nested (readClause f n i k v) := by
  have keys : ∀ x, x ∈ clauseKeys (readClause f n i k v) ↔ x = k ∨ x ∈ clauseKeys f := by
    intro x
    unfold readClause
    dsimp only
    simp only [clauseKeys, mapEntry_insert_same]
    rw [keys_insertSorted]
    have e2 : ∀ c : Bool, mapEntry (if c = true then f else problem f n .invalidKey).maps "clauses" =
        mapEntry f.maps "clauses" := by
      intro c
      split <;> rfl
    simp only [e2]
  refine ⟨fun x hx => ?_, fun b hb => ?_⟩
  · rw [keys]
    right
    unfold readClause at hx
    dsimp only at hx
    simp only [List.mem_filter] at hx
    have hx1 := hx.1
    have : x ∈ f.narrowings := by
      split at hx1 <;> split at hx1 <;> exact hx1
    exact h.1 x this
  · rw [keys]
    left
    unfold readClause at hb
    dsimp only at hb
    split at hb
    · simp only [Option.some.injEq] at hb
      rw [← hb]
    · simp at hb

theorem nested_readEntry (f : Fields) (m : String) (n : Nat) (line : String) (h : Nested f) :
    Nested (readEntry f m n line) := by
  unfold readEntry
  split
  · exact h
  · split
    · exact nested_readClause _ _ _ _ _ h
    · rename_i hm
      have hne : m ≠ "clauses" := by simpa using hm
      have same : ∀ inner, clauseKeys { f with maps := mapInsert f.maps m inner } = clauseKeys f := by
        intro inner
        simp only [clauseKeys, mapEntry_insert_other _ _ _ _ hne]
      refine ⟨fun x hx => ?_, fun b hb => ?_⟩
      · rw [same]
        exact h.1 x hx
      · rw [same]
        exact h.2 b hb

theorem nested_readLine (f : Fields) (n : Nat) (raw : String) (h : Nested f) :
    Nested (readLine f n raw) := by
  unfold readLine
  dsimp only
  split
  · exact h
  · split
    · unfold readIndented
      split
      · exact h
      · unfold readNested
        split
        · rename_i b hb
          split
          · exact nested_readNarrowing _ _ _ _ h (h.2 b hb)
          · exact nested_readEntry _ _ _ _ (nested_closeBlock _ h)
        · exact nested_readEntry _ _ _ _ h
    · unfold readKeyValue
      dsimp only
      have h0 := nested_closeBlock f h
      have g1 : ∀ (g : Fields) (n : Nat) (k : String), Nested g → Nested (noteUnknown g n k) := by
        intro g n k hg
        unfold noteUnknown
        split
        · exact hg
        · exact hg
      split
      · exact h0
      · rename_i rawKey rawValue _
        have h1 := g1 { closeBlock f with current := none } n rawKey.trim h0
        split
        · exact h1
        · split
          · exact h1
          · exact h1

theorem nested_fold (xs : List (Nat × String)) : ∀ f : Fields, Nested f →
    Nested (xs.foldl (fun fields pair => readLine fields (pair.1 + 2) pair.2) f) := by
  induction xs with
  | nil => intro f h; exact h
  | cons x _ ih =>
    intro f h
    exact ih _ (nested_readLine _ _ _ h)

theorem nested_readFrontmatter (fm : List String) : Nested (readFrontmatter fm) := by
  unfold readFrontmatter
  apply nested_closeBlock
  apply nested_fold
  exact ⟨fun x hx => by simp at hx, fun b hb => by simp at hb⟩

/-- @proves REQ-REQDOC.narrowings_nest -/
theorem a_narrowing_belongs_to_its_clause (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    (∀ x ∈ (parseLines lines).narrowings, x.1 ∈ (parseLines lines).clauses.map (·.1)) ∧
    (∀ y ∈ (parseLines lines).addressable,
      y = none ∨ ∃ k ∈ (parseLines lines).clauses.map (·.1), y = some k) := by
  obtain ⟨fm, body, id, _, _, _, _, _, hc, hn, ha, _⟩ := parse_requirement lines h
  rw [hc, hn, ha]
  refine ⟨fun x hx => ?_, fun y hy => ?_⟩
  · simp only [sortedNarrowings, List.mem_mergeSort, List.mem_filter] at hx
    exact (nested_readFrontmatter fm).1 x hx.1
  · split at hy
    · simp only [List.mem_singleton] at hy
      exact Or.inl hy
    · simp only [List.mem_map] at hy
      obtain ⟨c, hc', rfl⟩ := hy
      exact Or.inr ⟨c.1, List.mem_map.2 ⟨c, hc', rfl⟩, rfl⟩

end TraceLean.Requirement
