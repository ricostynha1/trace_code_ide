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

/-- @proves REQ-ANNOT.totality -/
theorem a_comment_without_at_signs_says_nothing :
    parseComment "just prose" 0 = { directives := [], problems := [] } := by
  native_decide

/-- @proves REQ-ANNOT.unknown_role_named -/
theorem an_unknown_role_is_reported :
    (parseComment "@banana REQ-X" 0).problems.length = 1 := by
  native_decide

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

/-- @proves REQ-ANNOT.region_balanced -/
theorem a_stray_end_is_reported :
    regionBalance [Directive.«end» 3]
      = [{ kind := .strayEnd, line := 3, message := "`@end` closes nothing" }] := by
  native_decide

/-- @proves REQ-ANNOT.region_balanced -/
theorem a_balanced_region_is_quiet :
    regionBalance
      [ .annotation { role := .implements, reqId := "REQ-A", clause := none, attrs := [],
                      opensRegion := true, line := 1 },
        Directive.«end» 5 ] = [] := by
  native_decide

/-- @proves REQ-ANNOT.role_vocabulary -/
theorem the_seven_roles_and_nothing_else :
    ["models", "specifies", "implements", "tests", "drt", "proves", "pins"].map Role.parse
      = [some .models, some .specifies, some .implements, some .tests, some .drt, some .proves,
         some .pins] ∧
    ["model", "test", "implement", "requires", "", "Models"].map Role.parse
      = [none, none, none, none, none, none] := by
  native_decide

/-- @proves REQ-ANNOT.qualifiers -/
theorem qualifiers_attach_or_name_their_own :
    (parseComment "@implements REQ-A.x\n@partial" 0).directives.getLast?
      = some (Directive.qualified (.partial none) none none 1) ∧
    (parseComment "@exempt REQ-B.y" 0).directives
      = [Directive.qualified (.exempt none none none) (some "REQ-B") (some "y") 0] ∧
    (parseComment "@nondeterministic" 0).directives
      = [Directive.qualified (.nondeterministic none) none none 0] ∧
    (parseComment "@structural" 0).directives
      = [Directive.qualified (.structural none) none none 0] := by
  native_decide

end TraceLean.Annotation
