import Lean

/-!
# The annotation grammar

Models `REQ-ANNOT`. Annotations are the only mechanism linking anything to
anything in this system, which makes **totality** the load-bearing property:
every `@word` yields either a directive or a named problem, and nothing is
dropped. A scanner that can silently discard a link reports less coverage than
the project has, and teaches people to distrust what it does report.

The grammar is deliberately narrow in two places. An identifier must start with
an upper-case letter, and a clause is word characters only -- because the
alternative to refusing an odd identifier is resolving it to something the
author did not mean. Both narrowings are visible here rather than buried in a
scanner.

Modelled over `List Char`. The implementation walks bytes, but every character
the grammar distinguishes is ASCII, so the two agree: a `@` preceded by a
multi-byte character is preceded by a byte that is not ASCII whitespace, and by
a character that is not whitespace either.
-/

namespace TraceLean.Annotation

open Lean (ToJson FromJson)

/-- What a link claims. `models` is the function computing what the clause
talks about; `specifies` is the `Prop` saying which answers are right
(ADR-0014).

@models REQ-ANNOT.role_vocabulary -/
inductive Role where
  | models | specifies | implements | tests | drt | proves | pins
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Nothing outside the vocabulary parses as a role. -/
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

/-- Modifies the claim made by the nearest annotation.

@models REQ-ANNOT.qualifiers -/
inductive Qualifier where
  /-- Caps a clause's contribution below full, and suppresses nothing. -/
  | «partial» (reason : Option String)
  /-- Removes a clause from the coverage denominator entirely. -/
  | exempt (reason judgedBy expires : Option String)
  /-- This model cannot be pinned, and here is why. -/
  | nondeterministic (reason : Option String)
  /-- This clause is a property of the tree rather than of a value, so there is
  no data-to-data function to model and no pair of functions to compare. It is
  still implemented and still tested; it stays in the denominator, which an
  exemption would not. -/
  | «structural» (reason : Option String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One annotation, before anything is resolved. -/
structure RawAnnotation where
  role : Role
  reqId : String
  clause : Option String
  attrs : List (String × String)
  opensRegion : Bool
  line : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One directive found in a comment. -/
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

/-- Everything one comment said. -/
structure ParsedComment where
  directives : List Directive
  problems : List Problem
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-! ## Character classes

Spelled out rather than taken from the standard library, so the grammar does not
change underneath the model when a library definition widens. -/

def isAsciiLower (c : Char) : Bool := 'a' ≤ c && c ≤ 'z'
def isAsciiUpper (c : Char) : Bool := 'A' ≤ c && c ≤ 'Z'
def isAsciiDigit (c : Char) : Bool := '0' ≤ c && c ≤ '9'
def isAsciiAlnum (c : Char) : Bool := isAsciiLower c || isAsciiUpper c || isAsciiDigit c
-- Form feed by code point: the grammar that reads these annotations cannot read
-- a `\xNN` character literal (ADR-0008).
def isSpace (c : Char) : Bool :=
  c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == Char.ofNat 12

/-- Characters of a word, and what follows. -/
def takeWhileP (p : Char → Bool) : List Char → List Char × List Char
  | [] => ([], [])
  | c :: rest => if p c then let (w, r) := takeWhileP p rest; (c :: w, r) else ([], c :: rest)

def asString (cs : List Char) : String := String.mk cs

/-! ## Attributes -/

/-- Insert into a sorted association list, later wins.

The implementation uses an ordered map; this is that map's behaviour written
out, because the order attributes come back in is part of what the two sides
have to agree on. -/
def insertAttr (attrs : List (String × String)) (key value : String) : List (String × String) :=
  match attrs with
  | [] => [(key, value)]
  | (k, v) :: rest =>
    if k == key then (key, value) :: rest
    else if decide (key < k) then (key, value) :: (k, v) :: rest
    else (k, v) :: insertAttr rest key value

/-- The key ending at this `=`, scanning backwards over the characters already
passed. Returned in reverse, which is the order they were passed in. -/
def keyBefore : List Char → List Char
  | [] => []
  | c :: rest => if isAsciiLower c || c == '_' then c :: keyBefore rest else []

/-- Everything before the first occurrence of `target`, and everything after. -/
def splitAtChar (target : Char) : List Char → Option (List Char × List Char)
  | [] => none
  | c :: rest =>
    if c == target then some ([], rest)
    else (splitAtChar target rest).map (fun p => (c :: p.1, p.2))

/--
`key="quoted value"` or `key=bare`, scanned left to right.

`before` holds the characters already passed, most recent first, which is how a
key is recovered without a second pass -- the same trick the implementation
plays with a byte index walking backwards.

`fuel` exists only to make the recursion structural: every step consumes at
least one character, so the length of the input is always enough. Lean cannot
see that through the quoted-value case, and an explicit bound is honest about
where the difficulty is.
-/
-- Written with `match` where an `if` would bind a `let` in a branch: the same
-- conditions in the same order, in the subset of Lean the annotation grammar
-- reads (ADR-0008).
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
            -- An unterminated quote takes the rest of the line rather than
            -- discarding what the author wrote.
            | none => insertAttr attrs (asString key) (asString afterQuote)
          | false =>
            let taken := takeWhileP (fun ch => !isSpace ch) (q :: afterQuote)
            attrsFrom fuel (insertAttr attrs (asString key) (asString taken.1)) [] taken.2

def parseAttrs (text : List Char) : List (String × String) :=
  attrsFrom (text.length + 1) [] [] text

/-- Whether some whitespace-delimited word of the input is exactly `target`. -/
def hasWord : Nat → List Char → List Char → Bool
  | 0, _, _ => false
  | _ + 1, _, [] => false
  | fuel + 1, target, cs =>
    match cs.dropWhile isSpace with
    | [] => false
    | rest =>
      let taken := takeWhileP (fun c => !isSpace c) rest
      taken.1 == target || hasWord fuel target taken.2

/-! ## Identifiers -/

/-- Split a leading ` ID` or ` ID.clause` off, returning what follows. -/
def splitIdentifier (tail : List Char) : Option String × Option String × List Char :=
  let trimmed := tail.dropWhile isSpace
  match trimmed.length == tail.length && !tail.isEmpty with
  -- `@modelsREQ-X`: the word did not end where an identifier begins.
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

/-! ## The scan -/

/-- Positions of every `@` that starts a word, with what follows each. -/
def atSigns : List Char → List Char → List (List Char)
  | _, [] => []
  | before, c :: rest =>
    let here :=
      if c == '@' && (before.isEmpty || isSpace (before.headD ' ')) then [rest] else []
    here ++ atSigns (c :: before) rest

/-- The qualifier a word names, if it names one. -/
private def qualifierOf (word : String) (attrs : List (String × String)) : Option Qualifier :=
  let get := fun k => (attrs.find? (·.1 == k)).map (·.2)
  match word with
  | "partial" => some (.partial (get "reason"))
  | "exempt" => some (.exempt (get "reason") (get "by") (get "until"))
  | "nondeterministic" => some (.nondeterministic (get "reason"))
  | "structural" => some (.structural (get "reason"))
  | _ => none

/-- One `@word` of a comment, read into what has been read so far.

A definition rather than a lambda with a typed binder, and `match` where an `if`
would bind a `let` in a branch: the same reading, in the subset of Lean the
annotation grammar reads (ADR-0008). -/
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

/-- Parse the directives on one line. -/
def parseLine (line : Nat) (text : List Char) : ParsedComment :=
  (atSigns [] text).foldl (parseLineStep line) { directives := [], problems := [] }

/-- A line as a line-oriented reader sees it. -/
def stripCarriageReturn (line : String) : String :=
  if line.endsWith "\r" then line.dropRight 1 else line

/--
Parse the directives in one comment's text.

`firstLine` is the 0-indexed line the comment starts on, so reported lines are
file lines.

@models REQ-ANNOT.totality
@models REQ-ANNOT.unknown_role_named
@models REQ-ANNOT.role_vocabulary
@models REQ-ANNOT.qualifiers
-/
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

/-- `parseComment`, over the lines a comment is made of.

Exists so a generator can compose comment bodies out of fragments: a flat string
schema would never produce a newline, and multi-line comments are where line
numbering can go wrong.

@models REQ-ANNOT.totality -/
def parseCommentLines (lines : List String) (firstLine : Nat) : ParsedComment :=
  parseComment (String.intercalate "\n" lines) firstLine

/-- Nothing is dropped: a comment with no at-signs yields nothing, and a comment
with one yields exactly one directive or one problem.

@proves REQ-ANNOT.totality -/
theorem a_comment_without_at_signs_says_nothing :
    parseComment "just prose" 0 = { directives := [], problems := [] } := by
  native_decide

/-- A role-shaped token outside the vocabulary is named, not ignored.

@proves REQ-ANNOT.unknown_role_named -/
theorem an_unknown_role_is_reported :
    (parseComment "@banana REQ-X" 0).problems.length = 1 := by
  native_decide

/-! ## Regions

`region_balanced`. A region is opened by an annotation carrying `begin` and
closed by `@end`. Both halves of the balance are reported: an opened region
nothing closes, and an `@end` that closes nothing.

Reporting both matters because they are different mistakes. An unclosed region
silently extends a claim over code nobody meant to claim; a stray `@end` means
somebody thought they were closing something and were not.
-/

/-- Region and `@end` balance across a sequence of directives.

The question is about the sequence and nothing else -- not the file, not the
parse, not what any directive names.

@models REQ-ANNOT.region_balanced -/
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

/-- An `@end` with nothing open is reported rather than ignored.

@proves REQ-ANNOT.region_balanced -/
theorem a_stray_end_is_reported :
    regionBalance [Directive.«end» 3]
      = [{ kind := .strayEnd, line := 3, message := "`@end` closes nothing" }] := by
  native_decide

/-- And a balanced pair is not.

@proves REQ-ANNOT.region_balanced -/
theorem a_balanced_region_is_quiet :
    regionBalance
      [ .annotation { role := .implements, reqId := "REQ-A", clause := none, attrs := [],
                      opensRegion := true, line := 1 },
        Directive.«end» 5 ] = [] := by
  native_decide

end TraceLean.Annotation
