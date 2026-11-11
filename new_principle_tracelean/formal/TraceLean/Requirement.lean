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
  | notAKeyValue
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

/-! A requirement is plain markdown: `# ID: title`, then `- key: value` bullets, prose, and
a `## Clauses` list of `- **key**: text` with `- key: text` narrowings under each. -/

def trimEnd (s : String) : String :=
  String.mk (s.toList.reverse.dropWhile Char.isWhitespace).reverse

def trimStart (s : String) : String :=
  String.mk (s.toList.dropWhile Char.isWhitespace)

def insertSorted (entries : List (String × String)) (key value : String) :
    List (String × String) :=
  ((entries.filter (·.1 != key)) ++ [(key, value)]).mergeSort (fun a b => a.1 ≤ b.1)

def lookup? (entries : List (String × String)) (key : String) : Option String :=
  (entries.find? (·.1 == key)).map (·.2)

def mapEntry (entries : List (String × List (String × String))) (key : String) :
    List (String × String) :=
  ((entries.find? (·.1 == key)).map (·.2)).getD []

def mapInsert (entries : List (String × List (String × String))) (key : String)
    (inner : List (String × String)) : List (String × List (String × String)) :=
  (entries.filter (·.1 != key)) ++ [(key, inner)]

def validKey (key : String) : Bool :=
  !key.isEmpty && key.all (fun c => c.isAlphanum || c == '_')

/-- The first occurrence of `sep` in a list of characters, and what lies either side of it. -/
def splitOnceL (sep : List Char) : List Char → Option (List Char × List Char)
  | [] => none
  | c :: rest =>
    if sep.isPrefixOf (c :: rest) then some ([], (c :: rest).drop sep.length)
    else (splitOnceL sep rest).map (fun p => (c :: p.1, p.2))

def splitOnce (s sep : String) : Option (String × String) :=
  (splitOnceL sep.toList s.toList).map (fun p => (String.mk p.1, String.mk p.2))

def stripPrefix (s p : String) : Option String :=
  if s.startsWith p then some (s.drop p.length) else none

def isIdQuote (c : Char) : Bool := c == '`' || c == '"' || c == '\''

def trimQuotes (s : String) : String :=
  String.mk ((s.toList.dropWhile isIdQuote).reverse.dropWhile isIdQuote).reverse

/-- Is this heading text the name of a requirement? -/
def named (id : String) : Bool :=
  !id.isEmpty && !(id.toList.any Char.isWhitespace) && id.contains '-' &&
    (match id.toList with | c :: _ => c.isUpper | [] => false) &&
    id.all (fun c => c.isAlphanum || c == '-' || c == '_')

structure Meta where
  refines : List String := []
  status : Option String := none
  decomposition : Option String := none
  deriving Inhabited

def refinesOf (value : String) : List String :=
  let inner := ((value.toList.dropWhile (· == '[')).reverse.dropWhile (· == ']')).reverse
  ((String.mk inner).splitOn ",").map (fun item => trimQuotes item.trim) |>.filter (fun i => !i.isEmpty)

def metaEntry (line : String) : Option (String × String) :=
  match stripPrefix line "- " with
  | none => none
  | some b =>
    match splitOnce b ":" with
    | none => none
    | some (key, value) =>
      let key := key.trim.toLower
      if key == "refines" || key == "status" || key == "decomposition" then some (key, value.trim)
      else none

def addMeta (m : Meta) (key value : String) : Meta :=
  if key == "refines" then { m with refines := refinesOf value }
  else if key == "status" then { m with status := some value }
  else { m with decomposition := some value }

/-- The bullets straight under the heading, and the lines from where they end. -/
def readMeta : List String → Meta → List String → Meta × List String
  | [], m, rest => (m, rest)
  | l :: ls, m, rest =>
    if l.trim.isEmpty then readMeta ls m rest
    else
      match metaEntry (trimEnd l) with
      | none => (m, rest)
      | some (k, v) => readMeta ls (addMeta m k v) ls

structure St where
  problems : List (Nat × FrontmatterKind) := []
  clauses : List (String × String) := []
  narrowings : List (String × List (String × String)) := []
  started : List (String × Nat) := []
  last : Option String := none
  inClauses : Bool := false
  fenced : Bool := false

def addProblem (s : St) (n : Nat) (k : FrontmatterKind) : St :=
  { s with problems := s.problems ++ [(n, k)] }

def flag (c : Bool) (s : St) (n : Nat) (k : FrontmatterKind) : St :=
  if c then addProblem s n k else s

def record (s : St) (n : Nat) (key text : String) : St :=
  { s with
    started := if (s.started.find? (·.1 == key)).isSome then s.started else s.started ++ [(key, n)]
    clauses := insertSorted s.clauses key text.trim
    last := some key }

def readClause (s : St) (n : Nat) (entry : String) : St :=
  match (stripPrefix entry "**").bind (fun e => splitOnce e "**:") with
  | none => addProblem s n .notAKeyValue
  | some (k, t) =>
    let key := k.trim
    record (flag (lookup? s.clauses key).isSome (flag (!validKey key) s n .invalidKey) n .duplicateClause)
      n key t

def narrow (s : St) (clause key text : String) : St :=
  { s with narrowings := mapInsert s.narrowings clause (insertSorted (mapEntry s.narrowings clause) key text.trim) }

def readNarrowing (s : St) (n : Nat) (entry : String) : St :=
  match s.last with
  | none => s
  | some clause =>
    match splitOnce entry ":" with
    | none => addProblem s n .notAKeyValue
    | some (k, t) =>
      let key := k.trim
      narrow (flag (lookup? (mapEntry s.narrowings clause) key).isSome (flag (!validKey key) s n .invalidKey) n .duplicateNarrowing)
        clause key t

def continueClause (s : St) (text : String) : St :=
  match s.last with
  | none => s
  | some k =>
    { s with clauses := s.clauses.map (fun c => if c.1 == k then (k, c.2 ++ " " ++ text) else c) }

def stepLine (s : St) (n : Nat) (line : String) : St :=
  if line.trim.isEmpty then s
  else
    let flush := line.length == (trimStart line).length
    match stripPrefix (trimStart line) "- " with
    | some entry => if flush then readClause s n entry else readNarrowing s n entry
    | none => continueClause s line.trim

def fence (s : St) (b : Bool) : St := { s with fenced := b }

def setFenced (s : St) (raw : String) : St :=
  let line := trimEnd raw
  fence s (if (trimStart line).startsWith "```" || (trimStart line).startsWith "~~~"
    then !s.fenced else s.fenced)

def enter (s : St) (b : Bool) : St := { s with inClauses := b }

def setSection (s : St) (raw : String) : St :=
  let line := trimEnd raw
  if !s.fenced && line.trim == "## Clauses" then enter s true
  else if !s.fenced && s.inClauses && line.startsWith "## " then enter s false
  else s

def step (s : St) (n : Nat) (raw : String) : St :=
  let s := setFenced s raw
  let t := setSection s raw
  if !s.fenced && (trimEnd raw).trim == "## Clauses" then t
  else if !t.inClauses then t
  else stepLine t n (trimEnd raw)

def enumFrom : Nat → List String → List (Nat × String)
  | _, [] => []
  | n, l :: ls => (n, l) :: enumFrom (n + 1) ls

def decompositionOf : Option String → Decomposition
  | some d => if d.trim.toLower == "complete" then .complete else .«open»
  | none => .«open»

def statusOf : Option String → Status
  | some raw =>
    let s := raw.trim.toLower
    if s == "approved" then .approved else if s == "linked" then .linked else .draft
  | none => .draft

def sortedNarrowings (narrowings : List (String × List (String × String))) :
    List (String × List (String × String)) :=
  (narrowings.filter (fun n => !n.2.isEmpty)).mergeSort (fun a b => a.1 ≤ b.1)

def missingText (s : St) (firstLine : Nat) : List (Nat × FrontmatterKind) :=
  (s.clauses.filter (fun c => c.2.isEmpty)).map
    (fun c => (((s.started.find? (·.1 == c.1)).map (·.2)).getD firstLine, .missingText))

def headingParts (heading : String) : String × String :=
  match splitOnce heading ":" with
  | some (i, t) => (i.trim, t.trim)
  | none => (heading.trim, "")

/-- The heading: the line number of the first line, the id and the title, then the lines after it. -/
def header (lines : List String) : Option (Nat × String × String × List String) :=
  match lines.dropWhile (fun l => l.trim.isEmpty) with
  | [] => none
  | h :: rest =>
    (stripPrefix h "# ").bind fun heading =>
      if named (headingParts heading).1 then
        some ((lines.length - rest.length), (headingParts heading).1, (headingParts heading).2, rest)
      else none

def runClauses (start : Nat) (body : List String) : St :=
  (enumFrom start body).foldl (fun s p => step s p.1 p.2) {}

def finish (at1 : Nat) (id title : String) (m : Meta) (s : St) : Parsed :=
  { isRequirement := true
    id := id
    title := if title.isEmpty then id else title
    refines := m.refines
    decomposition := decompositionOf m.decomposition
    status := statusOf m.status
    clauses := s.clauses
    narrowings := sortedNarrowings s.narrowings
    addressable := if s.clauses.isEmpty then [none] else s.clauses.map (fun c => some c.1)
    problems := s.problems ++ missingText s at1 }

def split (lines : List String) : List String := (String.intercalate "\n" lines).splitOn "\n"

/-- @models REQ-REQDOC.id_is_identity
@models REQ-REQDOC.clauseless_uniform
@models REQ-REQDOC.decomposition_claimed
@models REQ-REQDOC.malformed_reported
@models REQ-REQDOC.narrowings_nest
@models REQ-REQDOC.clause_unique -/
def parseLines (lines : List String) : Parsed :=
  match header (split lines) with
  | none => {}
  | some (at1, id, title, rest) =>
    let (m, body) := readMeta rest {} rest
    finish at1 id title m (runClauses ((split lines).length - body.length + 1) body)

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


theorem header_named (lines : List String) (a : Nat) (id title : String) (rest : List String)
    (h : header lines = some (a, id, title, rest)) : named id = true := by
  unfold header at h
  split at h
  · cases h
  · simp only [Option.bind_eq_some] at h
    obtain ⟨heading, _, h⟩ := h
    split at h
    · rename_i hn
      simp only [Option.some.injEq, Prod.mk.injEq] at h
      rw [← h.2.1]
      exact hn
    · cases h

theorem named_not_empty (id : String) (h : named id = true) : id ≠ "" := by
  intro e
  subst e
  simp [named] at h

theorem parse_some (lines : List String) (a : Nat) (id title : String) (rest : List String)
    (h : header (split lines) = some (a, id, title, rest)) :
    parseLines lines = finish a id title (readMeta rest {} rest).1
      (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1) (readMeta rest {} rest).2) := by
  unfold parseLines
  rw [h]

theorem parse_none (lines : List String) (h : header (split lines) = none) :
    parseLines lines = {} := by
  unfold parseLines
  rw [h]

theorem parse_requirement (lines : List String) (h : (parseLines lines).isRequirement = true) :
    ∃ a id title rest, header (split lines) = some (a, id, title, rest) ∧
      parseLines lines = finish a id title (readMeta rest {} rest).1
        (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1) (readMeta rest {} rest).2) := by
  cases hh : header (split lines) with
  | none => rw [parse_none lines hh] at h; simp at h
  | some t =>
    obtain ⟨a, id, title, rest⟩ := t
    exact ⟨a, id, title, rest, rfl, parse_some lines a id title rest hh⟩

/-- @proves REQ-REQDOC.id_is_identity -/
theorem a_requirement_is_its_declared_id (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    ∃ a title rest, header (split lines) = some (a, (parseLines lines).id, title, rest) ∧
      named (parseLines lines).id = true ∧ (parseLines lines).id ≠ "" := by
  obtain ⟨a, id, title, rest, hh, hp⟩ := parse_requirement lines h
  have hid : (parseLines lines).id = id := by rw [hp]; rfl
  rw [hid]
  exact ⟨a, title, rest, hh, header_named _ _ _ _ _ hh, named_not_empty _ (header_named _ _ _ _ _ hh)⟩

/-- @proves REQ-REQDOC.clauseless_uniform -/
theorem a_clauseless_requirement_is_one_clause (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    ((parseLines lines).clauses = [] → (parseLines lines).addressable = [none]) ∧
    ((parseLines lines).clauses ≠ [] →
      (parseLines lines).addressable = (parseLines lines).clauses.map (fun c => some c.1)) := by
  obtain ⟨a, id, title, rest, _, hp⟩ := parse_requirement lines h
  rw [hp]
  constructor
  · intro e
    simp only [finish] at e ⊢
    simp [e]
  · intro e
    simp only [finish] at e ⊢
    have : (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1) (readMeta rest {} rest).2).clauses.isEmpty = false := by
      cases hm : (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1) (readMeta rest {} rest).2).clauses
      · exact absurd hm e
      · rfl
    simp [this]

/-- @proves REQ-REQDOC.decomposition_claimed -/
theorem completeness_is_only_ever_claimed (lines : List String) :
    (parseLines lines).decomposition = .complete →
      ∃ a id title rest d, header (split lines) = some (a, id, title, rest) ∧
        (readMeta rest {} rest).1.decomposition = some d ∧ d.trim.toLower = "complete" := by
  intro hc
  cases hr : (parseLines lines).isRequirement
  · cases hh : header (split lines) with
    | none => rw [parse_none lines hh] at hc; cases hc
    | some t =>
      obtain ⟨a, id, title, rest⟩ := t
      have := parse_some lines a id title rest hh
      rw [this] at hr; simp [finish] at hr
  · obtain ⟨a, id, title, rest, hh, hp⟩ := parse_requirement lines hr
    rw [hp] at hc
    simp only [finish] at hc
    unfold decompositionOf at hc
    split at hc
    · rename_i d hd
      split at hc
      · rename_i ht
        exact ⟨a, id, title, rest, d, hh, hd, by simpa using ht⟩
      · cases hc
    · cases hc


/-! The clause reader only ever adds problems, and only ever declares a narrowing under a clause. -/

def Keeps (s t : St) : Prop := ∀ x ∈ s.problems, x ∈ t.problems

theorem keeps_refl (s : St) : Keeps s s := fun _ h => h

theorem keeps_trans {a b c : St} (h : Keeps a b) (g : Keeps b c) : Keeps a c := fun x hx => g x (h x hx)

theorem keeps_addProblem (s : St) (n : Nat) (k : FrontmatterKind) : Keeps s (addProblem s n k) := by
  intro x hx
  simp [addProblem, hx]

theorem keeps_flag (c : Bool) (s : St) (n : Nat) (k : FrontmatterKind) : Keeps s (flag c s n k) := by
  unfold flag
  split
  · exact keeps_addProblem _ _ _
  · exact keeps_refl _

theorem keeps_record (s : St) (n : Nat) (key text : String) : Keeps s (record s n key text) :=
  fun _ h => h

theorem keeps_narrow (s : St) (c k t : String) : Keeps s (narrow s c k t) := fun _ h => h

theorem keeps_enter (s : St) (b : Bool) : Keeps s (enter s b) := fun _ h => h

theorem keeps_readClause (s : St) (n : Nat) (entry : String) : Keeps s (readClause s n entry) := by
  unfold readClause
  split
  · exact keeps_addProblem _ _ _
  · exact keeps_trans (keeps_flag _ _ _ _) (keeps_trans (keeps_flag _ _ _ _) (keeps_record _ _ _ _))

theorem keeps_readNarrowing (s : St) (n : Nat) (entry : String) : Keeps s (readNarrowing s n entry) := by
  unfold readNarrowing
  split
  · exact keeps_refl _
  · split
    · exact keeps_addProblem _ _ _
    · exact keeps_trans (keeps_flag _ _ _ _) (keeps_trans (keeps_flag _ _ _ _) (keeps_narrow _ _ _ _))

theorem keeps_continueClause (s : St) (t : String) : Keeps s (continueClause s t) := by
  unfold continueClause
  split
  · exact keeps_refl _
  · exact fun _ h => h

theorem keeps_stepLine (s : St) (n : Nat) (line : String) : Keeps s (stepLine s n line) := by
  unfold stepLine
  split
  · exact keeps_refl _
  · dsimp only
    split
    · split
      · exact keeps_readClause _ _ _
      · exact keeps_readNarrowing _ _ _
    · exact keeps_continueClause _ _

theorem keeps_fence (s : St) (b : Bool) : Keeps s (fence s b) := fun _ h => h

theorem keeps_setFenced (s : St) (raw : String) : Keeps s (setFenced s raw) := keeps_fence _ _

theorem keeps_setSection (s : St) (raw : String) : Keeps s (setSection s raw) := by
  unfold setSection
  dsimp only
  split
  · exact keeps_enter _ _
  · split
    · exact keeps_enter _ _
    · exact keeps_refl _

theorem keeps_step (s : St) (n : Nat) (raw : String) : Keeps s (step s n raw) := by
  unfold step
  dsimp only
  have h1 := keeps_setFenced s raw
  have h2 := keeps_setSection (setFenced s raw) raw
  split
  · exact keeps_trans h1 h2
  · split
    · exact keeps_trans h1 h2
    · exact keeps_trans h1 (keeps_trans h2 (keeps_stepLine _ _ _))

theorem keeps_fold (xs : List (Nat × String)) : ∀ s : St,
    Keeps s (xs.foldl (fun s p => step s p.1 p.2) s) := by
  induction xs with
  | nil => intro s; exact keeps_refl _
  | cons p rest ih => intro s; exact keeps_trans (keeps_step s p.1 p.2) (ih _)


theorem mem_flag_true (s : St) (n : Nat) (k : FrontmatterKind) : (n, k) ∈ (flag true s n k).problems := by
  simp [flag, addProblem]

theorem mem_flag_keep (c : Bool) (s : St) (n : Nat) (k : FrontmatterKind) (x : Nat × FrontmatterKind)
    (h : x ∈ s.problems) : x ∈ (flag c s n k).problems := keeps_flag c s n k x h

/-- @proves REQ-REQDOC.malformed_reported -/
theorem a_malformed_document_is_reported (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    (∀ (s : St) (n : Nat) (e : String),
      (stripPrefix e "**").bind (fun e => splitOnce e "**:") = none →
        (n, FrontmatterKind.notAKeyValue) ∈ (readClause s n e).problems) ∧
    (∀ (xs : List (Nat × String)) (s : St), Keeps s (xs.foldl (fun s p => step s p.1 p.2) s)) ∧
    (∀ a id title rest, header (split lines) = some (a, id, title, rest) →
      ∀ x ∈ (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1)
              (readMeta rest {} rest).2).problems, x ∈ (parseLines lines).problems) ∧
    (∀ c ∈ (parseLines lines).clauses, c.2.isEmpty = true →
      ∃ n, (n, FrontmatterKind.missingText) ∈ (parseLines lines).problems) := by
  obtain ⟨a, id, title, rest, hh, hp⟩ := parse_requirement lines h
  refine ⟨?_, keeps_fold, ?_, ?_⟩
  · intro s n e he
    unfold readClause
    rw [he]
    simp [addProblem]
  · intro a' id' title' rest' hh' x hx
    rw [hh] at hh'
    cases hh'
    rw [hp]
    simp [finish, hx]
  · intro c hc he
    rw [hp] at hc ⊢
    simp only [finish, missingText] at hc ⊢
    refine ⟨(((runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1)
              (readMeta rest {} rest).2).started.find? (·.1 == c.1)).map (·.2)).getD a, ?_⟩
    apply List.mem_append_right
    exact List.mem_map.2 ⟨c, List.mem_filter.2 ⟨hc, he⟩, rfl⟩

/-- @proves REQ-REQDOC.clause_unique -/
theorem a_repeated_clause_or_narrowing_is_reported (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    (∀ a id title rest, header (split lines) = some (a, id, title, rest) →
      ∀ x ∈ (runClauses ((split lines).length - (readMeta rest {} rest).2.length + 1)
              (readMeta rest {} rest).2).problems, x ∈ (parseLines lines).problems) ∧
    (∀ (s : St) (n : Nat) (e k t : String),
      (stripPrefix e "**").bind (fun e => splitOnce e "**:") = some (k, t) →
      (lookup? s.clauses k.trim).isSome = true →
        (n, FrontmatterKind.duplicateClause) ∈ (readClause s n e).problems) ∧
    (∀ (s : St) (n : Nat) (e clause k t : String),
      s.last = some clause → splitOnce e ":" = some (k, t) →
      (lookup? (mapEntry s.narrowings clause) k.trim).isSome = true →
        (n, FrontmatterKind.duplicateNarrowing) ∈ (readNarrowing s n e).problems) := by
  obtain ⟨a, id, title, rest, hh, hp⟩ := parse_requirement lines h
  refine ⟨?_, ?_, ?_⟩
  · intro a' id' title' rest' hh' x hx
    rw [hh] at hh'
    cases hh'
    rw [hp]
    simp [finish, hx]
  · intro s n e k t hs hc
    unfold readClause
    rw [hs]
    simp only [record]
    simp [flag, addProblem, hc]
  · intro s n e clause k t hl hs hc
    unfold readNarrowing
    rw [hl]
    simp only [hs, narrow]
    simp [flag, addProblem, hc]


/-! A narrowing is only ever filed under a clause that has been declared. -/

def keys (xs : List (String × String)) : List String := xs.map (·.1)

def Nested (s : St) : Prop :=
  (∀ x ∈ s.narrowings, x.1 ∈ keys s.clauses) ∧ (∀ k, s.last = some k → k ∈ keys s.clauses)

theorem mem_keys_insertSorted (e : List (String × String)) (k v x : String) :
    x ∈ keys (insertSorted e k v) ↔ x = k ∨ x ∈ keys e := by
  unfold insertSorted keys
  simp only [List.mem_map, List.mem_mergeSort, List.mem_append, List.mem_filter, List.mem_singleton]
  constructor
  · rintro ⟨c, hc | hc, rfl⟩
    · by_cases h : c.1 = k
      · exact Or.inl h
      · exact Or.inr ⟨c, hc.1, rfl⟩
    · subst hc; exact Or.inl rfl
  · rintro (rfl | ⟨c, hc, rfl⟩)
    · exact ⟨(x, v), Or.inr rfl, rfl⟩
    · by_cases h : c.1 = k
      · exact ⟨(k, v), Or.inr rfl, h.symm⟩
      · exact ⟨c, Or.inl ⟨hc, by simpa using h⟩, rfl⟩

theorem nested_empty : Nested {} := ⟨fun x hx => by simp at hx, fun k hk => by simp at hk⟩

theorem flag_clauses (c : Bool) (s : St) (n : Nat) (k : FrontmatterKind) :
    (flag c s n k).clauses = s.clauses := by
  unfold flag; split <;> rfl

theorem nested_flag (c : Bool) (s : St) (n : Nat) (k : FrontmatterKind) (h : Nested s) :
    Nested (flag c s n k) := by
  unfold flag; split
  · exact h
  · exact h

theorem nested_addProblem (s : St) (n : Nat) (k : FrontmatterKind) (h : Nested s) :
    Nested (addProblem s n k) := h

theorem nested_record (s : St) (n : Nat) (key text : String) (h : Nested s) :
    Nested (record s n key text) := by
  refine ⟨fun x hx => ?_, fun k hk => ?_⟩
  · have := h.1 x hx
    exact (mem_keys_insertSorted _ _ _ _).2 (Or.inr this)
  · simp only [record, Option.some.injEq] at hk
    subst hk
    exact (mem_keys_insertSorted _ _ _ _).2 (Or.inl rfl)

theorem nested_narrow (s : St) (clause key text : String) (h : Nested s)
    (hc : clause ∈ keys s.clauses) : Nested (narrow s clause key text) := by
  refine ⟨fun x hx => ?_, h.2⟩
  simp only [narrow, mapInsert, List.mem_append, List.mem_filter, List.mem_singleton] at hx
  rcases hx with hx | rfl
  · exact h.1 x hx.1
  · exact hc

theorem nested_readClause (s : St) (n : Nat) (entry : String) (h : Nested s) :
    Nested (readClause s n entry) := by
  unfold readClause
  split
  · exact h
  · exact nested_record _ _ _ _ (nested_flag _ _ _ _ (nested_flag _ _ _ _ h))

theorem nested_readNarrowing (s : St) (n : Nat) (entry : String) (h : Nested s) :
    Nested (readNarrowing s n entry) := by
  unfold readNarrowing
  split
  · exact h
  · rename_i clause hl
    split
    · exact h
    · exact nested_narrow _ _ _ _ (nested_flag _ _ _ _ (nested_flag _ _ _ _ h))
        (by rw [flag_clauses, flag_clauses]; exact h.2 clause hl)

theorem nested_continueClause (s : St) (t : String) (h : Nested s) : Nested (continueClause s t) := by
  unfold continueClause
  split
  · exact h
  · rename_i k _hk
    have keep : ∀ cs : List (String × String),
        keys (cs.map (fun c => if c.1 == k then (k, c.2 ++ " " ++ t) else c)) = keys cs := by
      intro cs
      induction cs with
      | nil => rfl
      | cons c rest ih =>
        simp only [keys, List.map_cons] at ih ⊢
        rw [ih]
        by_cases hc : c.1 == k
        · simp [hc]; exact ((beq_iff_eq _ _).1 hc).symm
        · simp [hc]
    refine ⟨fun x hx => ?_, fun k' hk' => ?_⟩
    · show x.1 ∈ keys (s.clauses.map _)
      rw [keep]; exact h.1 x hx
    · show k' ∈ keys (s.clauses.map _)
      rw [keep]; exact h.2 k' hk'

theorem nested_stepLine (s : St) (n : Nat) (line : String) (h : Nested s) : Nested (stepLine s n line) := by
  unfold stepLine
  split
  · exact h
  · dsimp only
    split
    · split
      · exact nested_readClause _ _ _ h
      · exact nested_readNarrowing _ _ _ h
    · exact nested_continueClause _ _ h

theorem nested_fence' (s : St) (b : Bool) (h : Nested s) : Nested (fence s b) := h

theorem nested_enter (s : St) (b : Bool) (h : Nested s) : Nested (enter s b) := h

theorem nested_fence (s : St) (raw : String) (h : Nested s) : Nested (setFenced s raw) := by
  unfold setFenced
  exact nested_fence' _ _ h

theorem nested_setSection (s : St) (raw : String) (h : Nested s) : Nested (setSection s raw) := by
  unfold setSection
  dsimp only
  split
  · exact nested_enter _ _ h
  · split
    · exact nested_enter _ _ h
    · exact h

theorem nested_step (s : St) (n : Nat) (raw : String) (h : Nested s) : Nested (step s n raw) := by
  unfold step
  dsimp only
  have h1 : Nested (setFenced s raw) := nested_fence s raw h
  have h2 : Nested (setSection (setFenced s raw) raw) := nested_setSection _ _ h1
  split
  · exact h2
  · split
    · exact h2
    · exact nested_stepLine _ _ _ h2

theorem nested_fold (xs : List (Nat × String)) : ∀ s : St, Nested s →
    Nested (xs.foldl (fun s p => step s p.1 p.2) s) := by
  induction xs with
  | nil => intro s h; exact h
  | cons p _rest ih => intro s h; exact ih _ (nested_step s p.1 p.2 h)

/-- @proves REQ-REQDOC.narrowings_nest -/
theorem a_narrowing_belongs_to_its_clause (lines : List String)
    (h : (parseLines lines).isRequirement = true) :
    (∀ x ∈ (parseLines lines).narrowings, x.1 ∈ (parseLines lines).clauses.map (·.1)) ∧
    (∀ y ∈ (parseLines lines).addressable,
      y = none ∨ ∃ k ∈ (parseLines lines).clauses.map (·.1), y = some k) := by
  obtain ⟨_, _, _, rest, _, hp⟩ := parse_requirement lines h
  have hn := nested_fold (enumFrom ((split lines).length - (readMeta rest {} rest).2.length + 1)
    (readMeta rest {} rest).2) {} nested_empty
  rw [hp]
  refine ⟨fun x hx => ?_, fun y hy => ?_⟩
  · simp only [finish, sortedNarrowings, List.mem_mergeSort, List.mem_filter] at hx ⊢
    exact hn.1 x hx.1
  · simp only [finish] at hy ⊢
    split at hy
    · simp only [List.mem_singleton] at hy
      exact Or.inl hy
    · simp only [List.mem_map] at hy
      obtain ⟨c, hc', rfl⟩ := hy
      exact Or.inr ⟨c.1, List.mem_map.2 ⟨c, hc', rfl⟩, rfl⟩

end TraceLean.Requirement
