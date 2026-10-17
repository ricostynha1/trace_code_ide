import Lean

/-!
# Getting around the workspace

Models `REQ-NAV`: which name a line declares, where a name is declared and
used, which files a few typed letters reach, and which files a project opens
again. All are read off text the editor already holds; no language server is
asked.

Written in the subset the grammar reads (ADR-0008): no brace in a string, no
`where`, no `·` shorthand.
-/

namespace TraceLean.Nav

open Lean (ToJson FromJson)

/-- Words that introduce a declaration, in the languages the editor reads. -/
def introducers : List String :=
  ["fn", "struct", "enum", "trait", "type", "mod", "const", "static", "union", "macro_rules!",
   "def", "theorem", "lemma", "structure", "inductive", "class", "instance", "abbrev", "axiom",
   "function", "interface", "let", "var"]

/-- A word allowed before the declaring word. -/
def modifier (word : String) : Bool :=
  ["pub", "pub(crate)", "pub(super)", "crate", "async", "unsafe", "extern", "export",
   "default", "private", "protected", "noncomputable", "partial", "@[simp]"].contains word
    || word.startsWith "pub(" || word.startsWith "@["

/-- An opening brace, built from its code point: the grammar reads a brace in a
string as the start of an interpolation. -/
def brace : Char := Char.ofNat 123

/-- What a path's `::` becomes, so a path stays one word: `∷`. -/
def pathMark : Char := Char.ofNat 8759

def separator (c : Char) : Bool :=
  c.isWhitespace || "()<>:=;,".toList.contains c || c == brace

/-- The word after the first declaring word, when only modifiers come first. -/
def declaredAfter : List String → Option String
  | [] => none
  | word :: rest =>
    if introducers.contains word then
      match rest with
      | next :: _ => if next.toList.contains pathMark then none else some next
      | [] => none
    else if modifier word then declaredAfter rest else none

/-- The name a line declares: the word after its declaring word, with only
modifiers before that word; a path (`Kind::File`) is declared by no line.

@models REQ-NAV.declaration_line -/
def declaredOn (line : String) : Option String :=
  declaredAfter (((line.replace "::" (String.singleton pathMark)).split separator).filter (fun w => w ≠ ""))

/-- One place a name is found: a path, a one-based line, the line trimmed. -/
structure Declared where
  path : String
  line : Nat
  text : String
  deriving DecidableEq, Repr, ToJson, FromJson

def placed (path : String) (p : Nat × String) : Declared :=
  { path := path, line := p.1 + 1, text := p.2.trim }

/-- Each line of each file, numbered from one, that `keep` keeps. -/
def linesKept (files : List (String × String)) (keep : String → Bool) : List Declared :=
  files.bind (fun file => (((file.2.splitOn "\n").enum.filter (fun p => keep p.2)).map (placed file.1)))

def byPlace (a b : Declared) : Bool :=
  decide (a.path < b.path) || (a.path == b.path && decide (a.line ≤ b.line))

def lastPart (name : String) : String :=
  ((name.splitOn ".").getLast?).getD name

def declares (name : String) (l : String) : Bool :=
  declaredOn l == some name || (lastPart name != name && declaredOn l == some (lastPart name))

/-- Every line declaring `name`, or its last part when it is dotted, by path
then line.

@models REQ-NAV.definitions_found -/
def declarations (files : List (String × String)) (name : String) : List Declared :=
  (linesKept files (declares name)).mergeSort byPlace

def wordChar (c : Char) : Bool := c.isAlphanum || c == '_'

/-- Whether `needle` starts at `i` of `line`. -/
def startsAt (line needle : List Char) (i : Nat) : Bool :=
  (line.drop i).take needle.length == needle

def wordAfter (line : List Char) (i : Nat) : Bool :=
  ((line.drop i).head?.map wordChar).getD false

/-- Whether `needle` stands in `line` as a whole word somewhere. -/
def wholeIn (line needle : List Char) : Bool :=
  (List.range line.length).any (fun i =>
    startsAt line needle i && !(i > 0 && wordChar (line.getD (i - 1) ' '))
      && !(wordAfter line (i + needle.length)))

/-- Whether `needle` stands in `line` anywhere, ignoring case. -/
def heldIn (line needle : String) : Bool :=
  let l := line.toList.map Char.toLower
  let n := needle.toList.map Char.toLower
  (List.range (l.length + 1)).any (startsAt l n)

def holds (needle : String) (whole : Bool) (l : String) : Bool :=
  if whole then wholeIn l.toList needle.toList else heldIn l needle

def byPath (a b : String × String) : Bool := decide (a.1 ≤ b.1)

/-- Every line holding `needle` — as a whole word when `whole`, in any case
otherwise — by path then line, at most `limit`.

@models REQ-NAV.uses_found -/
def occurrences (files : List (String × String)) (needle : String) (whole : Bool) (limit : Nat) : List Declared :=
  if needle.isEmpty then [] else (linesKept (files.mergeSort byPath) (holds needle whole)).take limit

/-- The first place at or after `start` holding `letter`. -/
def findFrom (lower : List Char) (letter : Char) (start : Nat) : Option Nat :=
  ((List.range lower.length).drop start).find? (fun i => lower.get? i == some letter)

def wordBreak (c : Char) : Bool := "/_-.".toList.contains c

/-- What one letter found at `found` adds: four in the file's name, six next to
the letter before, three at the start of a word. -/
def bonus (lower : List Char) (nameStarts found : Nat) (previous : Option Nat) : Int :=
  (if found ≥ nameStarts then 4 else 0)
    + (if previous.map (fun p => p + 1) == some found then 6 else 0)
    + (if found == nameStarts || (found > 0 && wordBreak (lower.getD (found - 1) ' ')) then 3 else 0)

/-- The score of matching `wanted` in order, leftmost, from `start`: a point a
letter and its bonus. -/
def lettersFrom (lower : List Char) (nameStarts : Nat) :
    List Char → Nat → Option Nat → Int → Option Int
  | [], _, _, total => some total
  | letter :: rest, start, previous, total =>
    match findFrom lower letter start with
    | none => none
    | some found =>
      lettersFrom lower nameStarts rest (found + 1) (some found)
        (total + 1 + bonus lower nameStarts found previous)

/-- Where the file's name starts: after the last `/`. -/
def nameStart (lower : List Char) : Nat :=
  match (lower.enum.filter (fun p => p.2 == '/')).getLast? with
  | some p => p.1 + 1
  | none => 0

def best : List Int → Option Int
  | [] => none
  | t :: ts => some (ts.foldl max t)

/-- How well `query` matches `path`, if it does: matched from the start of the
path and within the file's name alone, the better kept; a tie goes to the
shorter name, then the shorter path. -/
def score (path query : String) : Option Int :=
  let lower := path.toList.map Char.toLower
  let wanted := (query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)
  let n := nameStart lower
  if wanted.isEmpty then some (-(lower.length : Int)) else
    (best ([0, n].filterMap (fun start => lettersFrom lower n wanted start none 0))).map
      (fun b => b * 1000000 - ((lower.length - n : Nat) : Int) * 1000 - (lower.length : Int))

def better (a b : Int × String) : Bool :=
  decide (a.1 > b.1) || (a.1 == b.1 && decide (a.2 ≤ b.2))

/-- The best `limit` paths for `query`, best first, a tie by path.

@models REQ-NAV.quick_open -/
def quickOpen (files : List String) (query : String) (limit : Nat) : List String :=
  (((files.filterMap (fun p => (score p query).map (fun s => (s, p)))).mergeSort better).take limit).map
    (fun p => p.2)

/-- The files a project opens again: its tabs in order, then the one in front
so it is in front again, each only if it still exists.

@models REQ-NAV.session_recalled -/
def reopened (files : List String) (shown : Option String) (existing : List String) : List String :=
  (files ++ shown.toList).filter existing.contains

/-- A declaring word with modifiers before it declares the next word; a use,
or a path, declares nothing.

@proves REQ-NAV.declaration_line -/
theorem the_word_after_the_declaring_word_is_declared :
    declaredOn "    pub(crate) fn go(a: u8)" = some "go" ∧
    declaredOn "theorem open_ok : True := trivial" = some "open_ok" ∧
    declaredOn "let Kind::File p = kind" = none ∧
    declaredOn "x = open(1)" = none := by
  native_decide

def sample : List (String × String) :=
  [("web/app.ts", "export function open() = 1\nconst x = open();\n"),
   ("src/a.rs", "use x;\npub fn open(a: u8) = 1\nfn other() = open(1)\n"),
   ("formal/A.lean", "def canon (w : W) : W := w\n")]

/-- A name's definitions are the lines declaring it, in every file, by path
then line, and a dotted name is found by its last part.

@proves REQ-NAV.definitions_found -/
theorem a_definition_is_where_it_is_declared :
    (declarations sample "open").map (fun d => (d.path, d.line)) = [("src/a.rs", 2), ("web/app.ts", 1)] ∧
    (declarations sample "Workspace.canon").map (fun d => d.line) = [1] := by
  native_decide

/-- Uses are whole words, overlapping ones too; a search ignores case; at most
the number asked.

@proves REQ-NAV.uses_found -/
theorem uses_are_whole_words_at_most_the_number_asked :
    (occurrences sample "open" true 50).map (fun d => (d.path, d.line))
      = [("src/a.rs", 2), ("src/a.rs", 3), ("web/app.ts", 1), ("web/app.ts", 2)] ∧
    (occurrences sample "OPEN(a" false 50).length = 1 ∧
    (occurrences sample "open" true 0) = [] ∧
    (occurrences [("a", "x...")] ".." true 5).length = 1 := by
  native_decide

/-- Letters in the file's name win over letters spread over its folders, and
no letters lists every file, shortest first.

@proves REQ-NAV.quick_open -/
theorem the_name_counts_most :
    quickOpen ["src/surface/screen.rs", "src/surface/view.rs", "docs/screenshots.md", "README.md"] "screen" 2
      = ["src/surface/screen.rs", "docs/screenshots.md"] ∧
    quickOpen ["src/surface/view.rs", "README.md"] "vw" 5 = ["src/surface/view.rs"] ∧
    quickOpen ["a/long.rs", "README.md"] "" 1 = ["README.md"] ∧
    quickOpen ["README.md"] "zzz" 5 = [] := by
  native_decide

/-- What was removed since stays closed, and the file in front comes last.

@proves REQ-NAV.session_recalled -/
theorem what_no_longer_exists_stays_closed :
    reopened ["a", "gone", "b"] (some "a") ["a", "b"] = ["a", "b", "a"] := by
  native_decide

/-- Whatever comes back exists, for every session kept.

@proves REQ-NAV.session_recalled -/
theorem everything_reopened_exists (files existing : List String) (shown : Option String) (p : String)
    (h : List.Mem p (reopened files shown existing)) : List.Mem p existing := by
  have kept := (List.mem_filter.mp h).2
  simpa using kept

end TraceLean.Nav
