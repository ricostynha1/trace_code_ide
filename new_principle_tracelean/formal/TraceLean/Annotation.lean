import Lean

namespace TraceLean.Annotation

open Lean (ToJson FromJson)

inductive Role where
  | models | specifies | implements | tests | drt | proves | pins
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Role.parse (word : String) : Option Role :=
  match word with
  | "models" => some .models
  | "specifies" => some .specifies
  | "implements" => some .implements
  | "tests" => some .tests
  | "drt" => some .drt
  | "proves" => some .proves
  | "pins" => some .pins
  | _ => none

inductive Qualifier where
  | «partial» (reason : Option String)
  | exempt (reason judgedBy expires : Option String)
  | nondeterministic (reason : Option String)
  | «structural» (reason : Option String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure RawAnnotation where
  role : Role
  reqId : String
  clause : Option String
  attrs : List (String × String)
  opensRegion : Bool
  line : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Directive where
  | annotation (annotation : RawAnnotation)
  | qualified (qualifier : Qualifier) (reqId : Option String) (clause : Option String) (line : Nat)
  | «end» (line : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive ProblemKind where
  | unknownRole
  | missingId
  | orphanQualifier
  | unclosedRegion
  | strayEnd
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Problem where
  kind : ProblemKind
  line : Nat
  message : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure ParsedComment where
  directives : List Directive
  problems : List Problem
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def isAsciiLower (c : Char) : Bool := 'a' ≤ c && c ≤ 'z'
def isAsciiUpper (c : Char) : Bool := 'A' ≤ c && c ≤ 'Z'
def isAsciiDigit (c : Char) : Bool := '0' ≤ c && c ≤ '9'
def isAsciiAlnum (c : Char) : Bool := isAsciiLower c || isAsciiUpper c || isAsciiDigit c
def isSpace (c : Char) : Bool :=
  c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == Char.ofNat 12

def takeWhileP (p : Char → Bool) : List Char → List Char × List Char
  | [] => ([], [])
  | c :: rest => if p c then let (w, r) := takeWhileP p rest; (c :: w, r) else ([], c :: rest)

def asString (cs : List Char) : String := String.mk cs

def insertAttr (attrs : List (String × String)) (key value : String) : List (String × String) :=
  match attrs with
  | [] => [(key, value)]
  | (k, v) :: rest =>
    if k == key then (key, value) :: rest
    else if decide (key < k) then (key, value) :: (k, v) :: rest
    else (k, v) :: insertAttr rest key value

def keyBefore : List Char → List Char
  | [] => []
  | c :: rest => if isAsciiLower c || c == '_' then c :: keyBefore rest else []

def splitAtChar (target : Char) : List Char → Option (List Char × List Char)
  | [] => none
  | c :: rest =>
    if c == target then some ([], rest)
    else (splitAtChar target rest).map (fun p => (c :: p.1, p.2))

def attrsFrom : Nat → List (String × String) → List Char → List Char → List (String × String)
  | 0, attrs, _, _ => attrs
  | _ + 1, attrs, _, [] => attrs
  | fuel + 1, attrs, before, c :: rest =>
    match c != '=' with
    | true => attrsFrom fuel attrs (c :: before) rest
    | false =>
      let key := (keyBefore before).reverse
      match key.isEmpty with
      | true => attrsFrom fuel attrs (c :: before) rest
      | false =>
        match rest with
        | [] => attrs
        | q :: afterQuote =>
          match q == '"' with
          | true =>
            match splitAtChar '"' afterQuote with
            | some (value, after) =>
              attrsFrom fuel (insertAttr attrs (asString key) (asString value)) [] after
            | none => insertAttr attrs (asString key) (asString afterQuote)
          | false =>
            let taken := takeWhileP (fun ch => !isSpace ch) (q :: afterQuote)
            attrsFrom fuel (insertAttr attrs (asString key) (asString taken.1)) [] taken.2

def parseAttrs (text : List Char) : List (String × String) :=
  attrsFrom (text.length + 1) [] [] text

def hasWord : Nat → List Char → List Char → Bool
  | 0, _, _ => false
  | _ + 1, _, [] => false
  | fuel + 1, target, cs =>
    match cs.dropWhile isSpace with
    | [] => false
    | rest =>
      let taken := takeWhileP (fun c => !isSpace c) rest
      taken.1 == target || hasWord fuel target taken.2

def splitIdentifier (tail : List Char) : Option String × Option String × List Char :=
  let trimmed := tail.dropWhile isSpace
  match trimmed.length == tail.length && !tail.isEmpty with
  | true => (none, none, tail)
  | false =>
    match trimmed with
    | [] => (none, none, tail)
    | first :: _ =>
      match !isAsciiUpper first with
      | true => (none, none, tail)
      | false =>
        let taken := takeWhileP (fun c => isAsciiAlnum c || c == '_' || c == '-') trimmed
        let id := asString taken.1
        match taken.2 with
        | '.' :: dotted =>
          let clause := takeWhileP (fun c => isAsciiAlnum c || c == '_') dotted
          match clause.1.isEmpty with
          | true => (some id, none, taken.2)
          | false => (some id, some (asString clause.1), clause.2)
        | _ => (some id, none, taken.2)

def atSigns : List Char → List Char → List (List Char)
  | _, [] => []
  | before, c :: rest =>
    let here :=
      if c == '@' && (before.isEmpty || isSpace (before.headD ' ')) then [rest] else []
    here ++ atSigns (c :: before) rest

private def qualifierOf (word : String) (attrs : List (String × String)) : Option Qualifier :=
  let get := fun k => (attrs.find? (·.1 == k)).map (·.2)
  match word with
  | "partial" => some (.partial (get "reason"))
  | "exempt" => some (.exempt (get "reason") (get "by") (get "until"))
  | "nondeterministic" => some (.nondeterministic (get "reason"))
  | "structural" => some (.structural (get "reason"))
  | _ => none

private def parseLineStep (line : Nat) (acc : ParsedComment) (rest : List Char) : ParsedComment :=
  let taken := takeWhileP (fun c => isAsciiLower c || c == '_') rest
  match taken.1.isEmpty with
  | true => acc
  | false =>
    let word := asString taken.1
    match word == "end" with
    | true => { acc with directives := acc.directives ++ [.end line] }
    | false =>
      let split := splitIdentifier taken.2
      let reqId := split.1
      let clause := split.2.1
      let afterId := split.2.2
      let attrs := parseAttrs afterId
      let opensRegion := hasWord (afterId.length + 1) "begin".toList afterId
      match Role.parse word with
      | some role =>
        match reqId with
        | some id =>
          { acc with directives := acc.directives ++
              [.annotation { role := role, reqId := id, clause := clause,
                             attrs := attrs, opensRegion := opensRegion, line := line }] }
        | none =>
          { acc with problems := acc.problems ++
              [{ kind := .missingId, line := line,
                 message := s!"`@{word}` names no requirement" }] }
      | none =>
        match qualifierOf word attrs with
        | some q => { acc with directives := acc.directives ++ [.qualified q reqId clause line] }
        | none =>
          { acc with problems := acc.problems ++
              [{ kind := .unknownRole, line := line,
                 message := s!"`@{word}` is not a role or a qualifier" }] }

def parseLine (line : Nat) (text : List Char) : ParsedComment :=
  (atSigns [] text).foldl (parseLineStep line) { directives := [], problems := [] }

def stripCarriageReturn (line : String) : String :=
  if line.endsWith "\r" then line.dropRight 1 else line

/-- @models REQ-ANNOT.totality
@models REQ-ANNOT.unknown_role_named
@models REQ-ANNOT.role_vocabulary
@models REQ-ANNOT.qualifiers -/
def parseComment (text : String) (firstLine : Nat) : ParsedComment :=
  let parts := text.splitOn "\n"
  let lines := if parts.getLast! == "" then parts.dropLast else parts
  lines.enum.foldl
    (fun acc pair =>
      let i := pair.1
      let line := pair.2
      let parsed := parseLine (firstLine + i) (stripCarriageReturn line).toList
      { directives := acc.directives ++ parsed.directives,
        problems := acc.problems ++ parsed.problems })
    { directives := [], problems := [] }

def parseCommentLines (lines : List String) (firstLine : Nat) : ParsedComment :=
  parseComment (String.intercalate "\n" lines) firstLine

/-- @models REQ-ANNOT.region_balanced -/
def regionBalance (directives : List Directive) : List Problem :=
  let step := directives.foldl
    (fun (acc : List Nat × List Problem) directive =>
      let (open_, problems) := acc
      match directive with
      | .annotation a => if a.opensRegion then (open_ ++ [a.line], problems) else acc
      | .«end» line =>
        match open_.reverse with
        | [] => (open_, problems ++ [{ kind := .strayEnd, line := line,
                                       message := "`@end` closes nothing" }])
        | _ :: rest => (rest.reverse, problems)
      | _ => acc)
    ([], [])
  step.2 ++ step.1.map (fun line =>
    { kind := .unclosedRegion, line := line,
      message := "a region opened with `begin` is never closed" })


def wordOf (r : List Char) : List Char := (takeWhileP (fun c => isAsciiLower c || c == '_') r).1

def tailOf (r : List Char) : List Char := (takeWhileP (fun c => isAsciiLower c || c == '_') r).2

def candidatesOf (text : String) (firstLine : Nat) : List (Nat × List Char) :=
  let parts := text.splitOn "\n"
  let lines := if parts.getLast! == "" then parts.dropLast else parts
  lines.enum.bind (fun p => (atSigns [] (stripCarriageReturn p.2).toList).map (fun r => (firstLine + p.1, r)))

def stepOf (c : Nat × List Char) : ParsedComment := parseLineStep c.1 ⟨[], []⟩ c.2

def roleWords : List String := ["models", "specifies", "implements", "tests", "drt", "proves", "pins"]

def qualifierWords : List String := ["partial", "exempt", "nondeterministic", "structural"]

theorem step_appends (line : Nat) (acc : ParsedComment) (r : List Char) :
    parseLineStep line acc r =
      ⟨acc.directives ++ (stepOf (line, r)).directives, acc.problems ++ (stepOf (line, r)).problems⟩ := by
  unfold stepOf parseLineStep
  simp only []
  split
  · simp
  · split
    · simp
    · split
      · split <;> simp
      · split <;> simp

theorem parseLine_fold (line : Nat) : ∀ (l : List (List Char)) (acc : ParsedComment),
    l.foldl (parseLineStep line) acc =
      ⟨acc.directives ++ l.bind (fun r => (stepOf (line, r)).directives),
       acc.problems ++ l.bind (fun r => (stepOf (line, r)).problems)⟩
  | [], acc => by simp
  | r :: rest, acc => by
    rw [List.foldl_cons, step_appends, parseLine_fold line rest]
    simp [List.append_assoc]

theorem parseComment_eq (text : String) (firstLine : Nat) :
    parseComment text firstLine =
      ⟨(candidatesOf text firstLine).bind (fun c => (stepOf c).directives),
       (candidatesOf text firstLine).bind (fun c => (stepOf c).problems)⟩ := by
  unfold parseComment candidatesOf
  simp only []
  generalize (if (text.splitOn "\n").getLast! == "" then (text.splitOn "\n").dropLast else text.splitOn "\n").enum = ps
  have key : ∀ (ps : List (Nat × String)) (acc : ParsedComment),
      ps.foldl (fun acc pair =>
        { directives := acc.directives ++ (parseLine (firstLine + pair.1) (stripCarriageReturn pair.2).toList).directives,
          problems := acc.problems ++ (parseLine (firstLine + pair.1) (stripCarriageReturn pair.2).toList).problems }) acc =
      ⟨acc.directives ++ (ps.bind (fun p => (atSigns [] (stripCarriageReturn p.2).toList).map
          (fun r => (firstLine + p.1, r)))).bind (fun c => (stepOf c).directives),
       acc.problems ++ (ps.bind (fun p => (atSigns [] (stripCarriageReturn p.2).toList).map
          (fun r => (firstLine + p.1, r)))).bind (fun c => (stepOf c).problems)⟩ := by
    intro ps
    induction ps with
    | nil => intro acc; simp
    | cons p rest ih =>
      intro acc
      rw [List.foldl_cons, ih]
      simp only [parseLine, parseLine_fold _, List.bind_cons, List.bind_append, List.bind_map,
        List.append_assoc, List.nil_append]
  rw [key]
  simp

theorem step_empty (line : Nat) (r : List Char) (h : wordOf r = []) :
    (stepOf (line, r)).directives = [] ∧ (stepOf (line, r)).problems = [] := by
  unfold stepOf parseLineStep
  unfold wordOf at h
  simp [h]

theorem step_one (line : Nat) (r : List Char) (h : wordOf r ≠ []) :
    (stepOf (line, r)).directives.length + (stepOf (line, r)).problems.length = 1 := by
  unfold stepOf parseLineStep
  unfold wordOf at h
  have : (takeWhileP (fun c => isAsciiLower c || c == '_') r).1.isEmpty = false := by
    cases e : (takeWhileP (fun c => isAsciiLower c || c == '_') r).1 with
    | nil => exact absurd e h
    | cons _ _ => rfl
  simp only [this]
  split
  · simp
  · split
    · split <;> simp
    · split <;> simp

theorem role_parse_words (w : String) (r : Role) (h : Role.parse w = some r) : w ∈ roleWords := by
  unfold Role.parse at h
  split at h <;> simp_all [roleWords]

theorem role_parse_complete (r : Role) : ∃ w ∈ roleWords, Role.parse w = some r := by
  cases r
  · exact ⟨"models", by simp [roleWords], rfl⟩
  · exact ⟨"specifies", by simp [roleWords], rfl⟩
  · exact ⟨"implements", by simp [roleWords], rfl⟩
  · exact ⟨"tests", by simp [roleWords], rfl⟩
  · exact ⟨"drt", by simp [roleWords], rfl⟩
  · exact ⟨"proves", by simp [roleWords], rfl⟩
  · exact ⟨"pins", by simp [roleWords], rfl⟩

theorem qualifierOf_words (w : String) (attrs : List (String × String)) (q : Qualifier)
    (h : qualifierOf w attrs = some q) : w ∈ qualifierWords := by
  unfold qualifierOf at h
  split at h <;> simp_all [qualifierWords]

theorem step_annotation (line : Nat) (r : List Char) (a : RawAnnotation)
    (h : Directive.annotation a ∈ (stepOf (line, r)).directives) :
    Role.parse (asString (wordOf r)) = some a.role ∧ a.line = line := by
  unfold stepOf parseLineStep at h
  unfold wordOf
  simp only [] at h
  split at h
  · simp at h
  · split at h
    · simp at h
    · split at h
      · rename_i role hr
        split at h
        · simp only [List.nil_append, List.mem_singleton, Directive.annotation.injEq] at h
          subst h
          exact ⟨hr, rfl⟩
        · simp at h
      · split at h
        · simp at h
        · simp at h

theorem word_nonempty_of_ne (r : List Char) (s : String) (hs : s ≠ "") (h : asString (wordOf r) = s) :
    (takeWhileP (fun c => isAsciiLower c || c == '_') r).1.isEmpty = false := by
  unfold wordOf at h
  cases e : (takeWhileP (fun c => isAsciiLower c || c == '_') r).1 with
  | nil => rw [e] at h; exact absurd h.symm hs
  | cons _ _ => rfl

theorem step_unknown (line : Nat) (r : List Char) (hw : wordOf r ≠ [])
    (hend : asString (wordOf r) ≠ "end") (hrole : asString (wordOf r) ∉ roleWords)
    (hq : asString (wordOf r) ∉ qualifierWords) :
    ∃ msg, (stepOf (line, r)).problems = [{ kind := .unknownRole, line := line, message := msg }] := by
  have hne : (takeWhileP (fun c => isAsciiLower c || c == '_') r).1.isEmpty = false := by
    unfold wordOf at hw
    cases e : (takeWhileP (fun c => isAsciiLower c || c == '_') r).1 with
    | nil => exact absurd e hw
    | cons _ _ => rfl
  have hparse : Role.parse (asString (wordOf r)) = none := by
    cases e : Role.parse (asString (wordOf r)) with
    | none => rfl
    | some rr => exact absurd (role_parse_words _ _ e) hrole
  unfold stepOf parseLineStep
  unfold wordOf at hend hparse hq
  simp only [hne]
  have hend' : (asString (takeWhileP (fun c => isAsciiLower c || c == '_') r).1 == "end") = false := by
    simpa using hend
  simp only [hend', hparse]
  split
  · rename_i q hqq
    exact absurd (qualifierOf_words _ _ _ hqq) hq
  · exact ⟨_, rfl⟩

theorem step_unknown_sound (line : Nat) (r : List Char) (p : Problem)
    (h : p ∈ (stepOf (line, r)).problems) (hk : p.kind = .unknownRole) :
    Role.parse (asString (wordOf r)) = none ∧ asString (wordOf r) ∉ qualifierWords ∧ p.line = line := by
  unfold stepOf parseLineStep at h
  unfold wordOf
  simp only [] at h
  split at h
  · simp at h
  · split at h
    · simp at h
    · split at h
      · split at h
        · simp at h
        · simp only [List.nil_append, List.mem_singleton] at h
          subst h
          cases hk
      · rename_i hr
        split at h
        · simp at h
        · rename_i hqn
          simp only [List.nil_append, List.mem_singleton] at h
          subst h
          refine ⟨hr, fun hm => ?_, rfl⟩
          simp only [qualifierWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hm
          rcases hm with hm | hm | hm | hm <;> simp [qualifierOf, hm] at hqn

theorem step_qualified_sound (line : Nat) (r : List Char) (q : Qualifier) (id cl : Option String) (ln : Nat)
    (h : Directive.qualified q id cl ln ∈ (stepOf (line, r)).directives) :
    asString (wordOf r) ∈ qualifierWords ∧ ln = line ∧ id = (splitIdentifier (tailOf r)).1 ∧
      cl = (splitIdentifier (tailOf r)).2.1 := by
  unfold stepOf parseLineStep at h
  unfold wordOf tailOf
  simp only [] at h
  split at h
  · simp at h
  · split at h
    · simp at h
    · split at h
      · split at h
        · simp at h
        · simp at h
      · split at h
        · rename_i q' hq'
          simp only [List.nil_append, List.mem_singleton, Directive.qualified.injEq] at h
          obtain ⟨_, h2, h3, h4⟩ := h
          exact ⟨qualifierOf_words _ _ _ hq', h4, h2, h3⟩
        · simp at h

theorem step_qualified_complete (line : Nat) (r : List Char) (hq : asString (wordOf r) ∈ qualifierWords) :
    ∃ q, (stepOf (line, r)).directives =
      [Directive.qualified q (splitIdentifier (tailOf r)).1 (splitIdentifier (tailOf r)).2.1 line] := by
  have hne := word_nonempty_of_ne r (asString (wordOf r)) (by
    simp only [qualifierWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hq
    rcases hq with h | h | h | h <;> rw [h] <;> simp) rfl
  have hparse : Role.parse (asString (wordOf r)) = none := by
    simp only [qualifierWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hq
    rcases hq with h | h | h | h <;> rw [h] <;> rfl
  have hend : (asString (wordOf r) == "end") = false := by
    simp only [qualifierWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hq
    rcases hq with h | h | h | h <;> rw [h] <;> rfl
  have hsome : ∀ attrs, ∃ q, qualifierOf (asString (wordOf r)) attrs = some q := by
    intro attrs
    simp only [qualifierWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hq
    rcases hq with h | h | h | h <;> rw [h] <;> exact ⟨_, rfl⟩
  unfold stepOf parseLineStep
  unfold wordOf at hparse hend hsome
  unfold tailOf
  simp only [hne, hend, hparse]
  split
  · exact ⟨_, rfl⟩
  · rename_i hn
    obtain ⟨q, hq⟩ := hsome (parseAttrs (splitIdentifier (takeWhileP (fun c => isAsciiLower c || c == '_') r).2).2.2)
    rw [hq] at hn
    cases hn

theorem bind_count {β γ : Type} (l : List (Nat × List Char)) (d : Nat × List Char → List β) (p : Nat × List Char → List γ)
    (hd : ∀ c, (d c).length + (p c).length = if (wordOf c.2).isEmpty then 0 else 1) :
    (l.bind d).length + (l.bind p).length = (l.filter (fun c => !(wordOf c.2).isEmpty)).length := by
  induction l with
  | nil => rfl
  | cons c rest ih =>
    simp only [List.bind_cons, List.length_append, List.filter_cons]
    have := hd c
    cases hb : (wordOf c.2).isEmpty
    · simp only [hb, Bool.not_false, ite_true, List.length_cons, Bool.false_eq_true, ite_false] at this ⊢
      omega
    · simp only [hb, Bool.not_true, ite_true, Bool.false_eq_true, ite_false] at this ⊢
      omega

/-- @proves REQ-ANNOT.totality -/
theorem every_candidate_parses_or_is_a_problem (text : String) (firstLine : Nat) :
    (parseComment text firstLine).directives.length + (parseComment text firstLine).problems.length =
      ((candidatesOf text firstLine).filter (fun c => !(wordOf c.2).isEmpty)).length := by
  rw [parseComment_eq]
  dsimp only
  refine bind_count _ (fun c => (stepOf c).directives) (fun c => (stepOf c).problems) (fun c => ?_)
  by_cases h : wordOf c.2 = []
  · obtain ⟨h1, h2⟩ := step_empty c.1 c.2 h
    show (stepOf (c.1, c.2)).directives.length + (stepOf (c.1, c.2)).problems.length = _
    rw [h1, h2, h]
    rfl
  · have := step_one c.1 c.2 h
    have hb : (wordOf c.2).isEmpty = false := by
      cases e : wordOf c.2 with
      | nil => exact absurd e h
      | cons _ _ => rfl
    simp only [hb]
    exact this

/-- @proves REQ-ANNOT.unknown_role_named -/
theorem unknown_roles_are_named (text : String) (firstLine : Nat) :
    (∀ c ∈ candidatesOf text firstLine, wordOf c.2 ≠ [] → asString (wordOf c.2) ≠ "end" →
      asString (wordOf c.2) ∉ roleWords → asString (wordOf c.2) ∉ qualifierWords →
      ∃ msg, { kind := .unknownRole, line := c.1, message := msg } ∈ (parseComment text firstLine).problems) ∧
    (∀ p ∈ (parseComment text firstLine).problems, p.kind = .unknownRole →
      ∃ c ∈ candidatesOf text firstLine, p.line = c.1 ∧
        asString (wordOf c.2) ∉ roleWords ∧ asString (wordOf c.2) ∉ qualifierWords) := by
  rw [parseComment_eq]
  constructor
  · intro c hc hw hend hrole hq
    obtain ⟨msg, hm⟩ := step_unknown c.1 c.2 hw hend hrole hq
    exact ⟨msg, List.mem_bind.mpr ⟨c, hc, by rw [hm]; exact List.mem_singleton_self _⟩⟩
  · intro p hp hk
    obtain ⟨c, hc, hpc⟩ := List.mem_bind.mp hp
    obtain ⟨hr, hq, hl⟩ := step_unknown_sound c.1 c.2 p hpc hk
    refine ⟨c, hc, hl, fun hm => ?_, hq⟩
    obtain ⟨r, hr'⟩ : ∃ r, Role.parse (asString (wordOf c.2)) = some r := by
      simp only [roleWords, List.mem_cons, List.mem_singleton, List.not_mem_nil, or_false] at hm
      rcases hm with h | h | h | h | h | h | h <;> rw [h] <;> exact ⟨_, rfl⟩
    rw [hr] at hr'
    cases hr'

/-- @proves REQ-ANNOT.role_vocabulary -/
theorem only_the_seven_roles_parse (text : String) (firstLine : Nat) :
    (∀ a, Directive.annotation a ∈ (parseComment text firstLine).directives →
      ∃ c ∈ candidatesOf text firstLine, asString (wordOf c.2) ∈ roleWords ∧
        Role.parse (asString (wordOf c.2)) = some a.role ∧ a.line = c.1) ∧
    (∀ w r, Role.parse w = some r → w ∈ roleWords) ∧
    (∀ r, ∃ w ∈ roleWords, Role.parse w = some r) := by
  refine ⟨?_, role_parse_words, role_parse_complete⟩
  rw [parseComment_eq]
  intro a ha
  obtain ⟨c, hc, hac⟩ := List.mem_bind.mp ha
  obtain ⟨h1, h2⟩ := step_annotation c.1 c.2 a hac
  exact ⟨c, hc, role_parse_words _ _ h1, h1, h2⟩

/-- @proves REQ-ANNOT.qualifiers -/
theorem qualifiers_parse_with_their_identifier (text : String) (firstLine : Nat) :
    (∀ c ∈ candidatesOf text firstLine, asString (wordOf c.2) ∈ qualifierWords →
      ∃ q, Directive.qualified q (splitIdentifier (tailOf c.2)).1 (splitIdentifier (tailOf c.2)).2.1 c.1 ∈
        (parseComment text firstLine).directives) ∧
    (∀ q id cl ln, Directive.qualified q id cl ln ∈ (parseComment text firstLine).directives →
      ∃ c ∈ candidatesOf text firstLine, asString (wordOf c.2) ∈ qualifierWords ∧ ln = c.1 ∧
        id = (splitIdentifier (tailOf c.2)).1 ∧ cl = (splitIdentifier (tailOf c.2)).2.1) := by
  rw [parseComment_eq]
  constructor
  · intro c hc hq
    obtain ⟨q, hd⟩ := step_qualified_complete c.1 c.2 hq
    exact ⟨q, List.mem_bind.mpr ⟨c, hc, by rw [hd]; exact List.mem_singleton_self _⟩⟩
  · intro q id cl ln h
    obtain ⟨c, hc, hd⟩ := List.mem_bind.mp h
    exact ⟨c, hc, step_qualified_sound c.1 c.2 q id cl ln hd⟩

def tally : Nat × Nat → Directive → Nat × Nat
  | (k, s), .annotation a => if a.opensRegion then (k + 1, s) else (k, s)
  | (k, s), .«end» _ => if k = 0 then (0, s + 1) else (k - 1, s)
  | ks, _ => ks

def balanceStep (acc : List Nat × List Problem) (directive : Directive) : List Nat × List Problem :=
  let (open_, problems) := acc
  match directive with
  | .annotation a => if a.opensRegion then (open_ ++ [a.line], problems) else acc
  | .«end» line =>
    match open_.reverse with
    | [] => (open_, problems ++ [{ kind := .strayEnd, line := line, message := "`@end` closes nothing" }])
    | _ :: rest => (rest.reverse, problems)
  | _ => acc

theorem regionBalance_eq (directives : List Directive) :
    regionBalance directives =
      (directives.foldl balanceStep ([], [])).2 ++ (directives.foldl balanceStep ([], [])).1.map (fun line =>
        { kind := .unclosedRegion, line := line, message := "a region opened with `begin` is never closed" }) := rfl

theorem balance_fold : ∀ (ds : List Directive) (st : List Nat) (ps : List Problem),
    (∀ p ∈ ps, p.kind = .strayEnd) →
    (ds.foldl balanceStep (st, ps)).1.length = (ds.foldl tally (st.length, ps.length)).1 ∧
    (ds.foldl balanceStep (st, ps)).2.length = (ds.foldl tally (st.length, ps.length)).2 ∧
    ∀ p ∈ (ds.foldl balanceStep (st, ps)).2, p.kind = .strayEnd
  | [], st, ps, h => ⟨rfl, rfl, h⟩
  | d :: rest, st, ps, h => by
    simp only [List.foldl_cons]
    cases d with
    | annotation a =>
      by_cases ho : a.opensRegion = true
      · have e1 : balanceStep (st, ps) (.annotation a) = (st ++ [a.line], ps) := by simp [balanceStep, ho]
        have e2 : tally (st.length, ps.length) (.annotation a) = ((st ++ [a.line]).length, ps.length) := by
          simp [tally, ho]
        rw [e1, e2]
        exact balance_fold rest _ _ h
      · have e1 : balanceStep (st, ps) (.annotation a) = (st, ps) := by simp [balanceStep, ho]
        have e2 : tally (st.length, ps.length) (.annotation a) = (st.length, ps.length) := by
          simp [tally, ho]
        rw [e1, e2]
        exact balance_fold rest _ _ h
    | qualified q i c l =>
      exact balance_fold rest st ps h
    | «end» line =>
      cases hs : st.reverse with
      | nil =>
        have hst : st = [] := List.reverse_eq_nil_iff.mp hs
        subst hst
        have e1 : balanceStep ([], ps) (.«end» line) =
            ([], ps ++ [{ kind := .strayEnd, line := line, message := "`@end` closes nothing" }]) := rfl
        have e2 : tally (([] : List Nat).length, ps.length) (.«end» line) =
            (([] : List Nat).length, (ps ++ [({ kind := .strayEnd, line := line, message := "`@end` closes nothing" } : Problem)]).length) := by simp [tally]
        rw [e1, e2]
        apply balance_fold rest _ _
        intro p hp
        rcases List.mem_append.mp hp with hp | hp
        · exact h p hp
        · rw [List.mem_singleton.mp hp]
      | cons x xs =>
        have e1 : balanceStep (st, ps) (.«end» line) = (xs.reverse, ps) := by
          simp only [balanceStep, hs]
        have hlen : st.length = xs.reverse.length + 1 := by
          rw [← List.length_reverse st, hs]
          simp
        have e2 : tally (st.length, ps.length) (.«end» line) = (xs.reverse.length, ps.length) := by
          simp [tally, hlen]
        rw [e1, e2]
        exact balance_fold rest _ _ h

/-- @proves REQ-ANNOT.region_balanced -/
theorem unbalanced_regions_are_reported (directives : List Directive) :
    ((regionBalance directives).filter (fun p => p.kind == .strayEnd)).length =
      (directives.foldl tally (0, 0)).2 ∧
    ((regionBalance directives).filter (fun p => p.kind == .unclosedRegion)).length =
      (directives.foldl tally (0, 0)).1 := by
  obtain ⟨h1, h2, h3⟩ := balance_fold directives [] [] (fun _ h => absurd h (List.not_mem_nil _))
  rw [regionBalance_eq, List.filter_append, List.filter_append]
  have fs : (directives.foldl balanceStep ([], [])).2.filter (fun p => p.kind == .strayEnd) =
      (directives.foldl balanceStep ([], [])).2 := List.filter_eq_self.mpr (fun p hp => by simp [h3 p hp])
  have fu : (directives.foldl balanceStep ([], [])).2.filter (fun p => p.kind == .unclosedRegion) = [] :=
    List.filter_eq_nil.mpr (fun p hp => by simp [h3 p hp])
  have ms : ((directives.foldl balanceStep ([], [])).1.map (fun line =>
        ({ kind := .unclosedRegion, line := line, message := "a region opened with `begin` is never closed" } : Problem))).filter
        (fun p => p.kind == .strayEnd) = [] :=
    List.filter_eq_nil.mpr (fun p hp => by
      obtain ⟨_, _, rfl⟩ := List.mem_map.mp hp
      simp)
  have mu : ((directives.foldl balanceStep ([], [])).1.map (fun line =>
        ({ kind := .unclosedRegion, line := line, message := "a region opened with `begin` is never closed" } : Problem))).filter
        (fun p => p.kind == .unclosedRegion) =
        (directives.foldl balanceStep ([], [])).1.map (fun line =>
        ({ kind := .unclosedRegion, line := line, message := "a region opened with `begin` is never closed" } : Problem)) :=
    List.filter_eq_self.mpr (fun p hp => by
      obtain ⟨_, _, rfl⟩ := List.mem_map.mp hp
      simp)
  rw [fs, fu, ms, mu]
  simp only [List.append_nil, List.nil_append, List.length_map]
  exact ⟨h2, h1⟩

end TraceLean.Annotation
