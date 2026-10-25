import Lean

namespace TraceLean.Nav

open Lean (ToJson FromJson)

def introducers : List String :=
  ["fn", "struct", "enum", "trait", "type", "mod", "const", "static", "union", "macro_rules!",
   "def", "theorem", "lemma", "structure", "inductive", "class", "instance", "abbrev", "axiom",
   "function", "interface", "let", "var"]

def modifier (word : String) : Bool :=
  ["pub", "pub(crate)", "pub(super)", "crate", "async", "unsafe", "extern", "export",
   "default", "private", "protected", "noncomputable", "partial", "@[simp]"].contains word
    || word.startsWith "pub(" || word.startsWith "@["

def brace : Char := Char.ofNat 123

def pathMark : Char := Char.ofNat 8759

def separator (c : Char) : Bool :=
  c.isWhitespace || "()<>:=;,".toList.contains c || c == brace

def declaredAfter : List String → Option String
  | [] => none
  | word :: rest =>
    if introducers.contains word then
      match rest with
      | next :: _ => if next.toList.contains pathMark then none else some next
      | [] => none
    else if modifier word then declaredAfter rest else none

/-- @models REQ-NAV.declaration_line -/
def declaredOn (line : String) : Option String :=
  declaredAfter (((line.replace "::" (String.singleton pathMark)).split separator).filter (fun w => w ≠ ""))

structure Declared where
  path : String
  line : Nat
  text : String
  deriving DecidableEq, Repr, ToJson, FromJson

def placed (path : String) (p : Nat × String) : Declared :=
  { path := path, line := p.1 + 1, text := p.2.trim }

def linesKept (files : List (String × String)) (keep : String → Bool) : List Declared :=
  files.bind (fun file => (((file.2.splitOn "\n").enum.filter (fun p => keep p.2)).map (placed file.1)))

def byPlace (a b : Declared) : Bool :=
  decide (a.path < b.path) || (a.path == b.path && decide (a.line ≤ b.line))

def lastPart (name : String) : String :=
  ((name.splitOn ".").getLast?).getD name

def declares (name : String) (l : String) : Bool :=
  declaredOn l == some name || (lastPart name != name && declaredOn l == some (lastPart name))

/-- @models REQ-NAV.definitions_found -/
def declarations (files : List (String × String)) (name : String) : List Declared :=
  (linesKept files (declares name)).mergeSort byPlace

def wordChar (c : Char) : Bool := c.isAlphanum || c == '_'

def startsAt (line needle : List Char) (i : Nat) : Bool :=
  (line.drop i).take needle.length == needle

def wordAfter (line : List Char) (i : Nat) : Bool :=
  ((line.drop i).head?.map wordChar).getD false

def wholeIn (line needle : List Char) : Bool :=
  (List.range line.length).any (fun i =>
    startsAt line needle i && !(i > 0 && wordChar (line.getD (i - 1) ' '))
      && !(wordAfter line (i + needle.length)))

def heldIn (line needle : String) : Bool :=
  let l := line.toList.map Char.toLower
  let n := needle.toList.map Char.toLower
  (List.range (l.length + 1)).any (startsAt l n)

def holds (needle : String) (whole : Bool) (l : String) : Bool :=
  if whole then wholeIn l.toList needle.toList else heldIn l needle

def byPath (a b : String × String) : Bool := decide (a.1 ≤ b.1)

/-- @models REQ-NAV.uses_found -/
def occurrences (files : List (String × String)) (needle : String) (whole : Bool) (limit : Nat) : List Declared :=
  if needle.isEmpty then [] else (linesKept (files.mergeSort byPath) (holds needle whole)).take limit

def findFrom (lower : List Char) (letter : Char) (start : Nat) : Option Nat :=
  ((List.range lower.length).drop start).find? (fun i => lower.get? i == some letter)

def wordBreak (c : Char) : Bool := "/_-.".toList.contains c

def bonus (lower : List Char) (nameStarts found : Nat) (previous : Option Nat) : Int :=
  (if found ≥ nameStarts then 4 else 0)
    + (if previous.map (fun p => p + 1) == some found then 6 else 0)
    + (if found == nameStarts || (found > 0 && wordBreak (lower.getD (found - 1) ' ')) then 3 else 0)

def lettersFrom (lower : List Char) (nameStarts : Nat) :
    List Char → Nat → Option Nat → Int → Option Int
  | [], _, _, total => some total
  | letter :: rest, start, previous, total =>
    match findFrom lower letter start with
    | none => none
    | some found =>
      lettersFrom lower nameStarts rest (found + 1) (some found)
        (total + 1 + bonus lower nameStarts found previous)

def nameStart (lower : List Char) : Nat :=
  match (lower.enum.filter (fun p => p.2 == '/')).getLast? with
  | some p => p.1 + 1
  | none => 0

def best : List Int → Option Int
  | [] => none
  | t :: ts => some (ts.foldl max t)

def score (path query : String) : Option Int :=
  let lower := path.toList.map Char.toLower
  let wanted := (query.toList.map Char.toLower).filter (fun c => !c.isWhitespace)
  let n := nameStart lower
  if wanted.isEmpty then some (-(lower.length : Int)) else
    (best ([0, n].filterMap (fun start => lettersFrom lower n wanted start none 0))).map
      (fun b => b * 1000000 - ((lower.length - n : Nat) : Int) * 1000 - (lower.length : Int))

def better (a b : Int × String) : Bool :=
  decide (a.1 > b.1) || (a.1 == b.1 && decide (a.2 ≤ b.2))

/-- @models REQ-NAV.quick_open -/
def quickOpen (files : List String) (query : String) (limit : Nat) : List String :=
  (((files.filterMap (fun p => (score p query).map (fun s => (s, p)))).mergeSort better).take limit).map
    (fun p => p.2)

/-- @models REQ-NAV.session_recalled -/
def reopened (files : List String) (shown : Option String) (existing : List String) : List String :=
  (files ++ shown.toList).filter existing.contains

/-- @proves REQ-NAV.declaration_line -/
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

/-- @proves REQ-NAV.definitions_found -/
theorem a_definition_is_where_it_is_declared :
    (declarations sample "open").map (fun d => (d.path, d.line)) = [("src/a.rs", 2), ("web/app.ts", 1)] ∧
    (declarations sample "Workspace.canon").map (fun d => d.line) = [1] := by
  native_decide

/-- @proves REQ-NAV.uses_found -/
theorem uses_are_whole_words_at_most_the_number_asked :
    (occurrences sample "open" true 50).map (fun d => (d.path, d.line))
      = [("src/a.rs", 2), ("src/a.rs", 3), ("web/app.ts", 1), ("web/app.ts", 2)] ∧
    (occurrences sample "OPEN(a" false 50).length = 1 ∧
    (occurrences sample "open" true 0) = [] ∧
    (occurrences [("a", "x...")] ".." true 5).length = 1 := by
  native_decide

/-- @proves REQ-NAV.quick_open -/
theorem the_name_counts_most :
    quickOpen ["src/surface/screen.rs", "src/surface/view.rs", "docs/screenshots.md", "README.md"] "screen" 2
      = ["src/surface/screen.rs", "docs/screenshots.md"] ∧
    quickOpen ["src/surface/view.rs", "README.md"] "vw" 5 = ["src/surface/view.rs"] ∧
    quickOpen ["a/long.rs", "README.md"] "" 1 = ["README.md"] ∧
    quickOpen ["README.md"] "zzz" 5 = [] := by
  native_decide

/-- @proves REQ-NAV.session_recalled -/
theorem what_no_longer_exists_stays_closed :
    reopened ["a", "gone", "b"] (some "a") ["a", "b"] = ["a", "b", "a"] := by
  native_decide

/-- @proves REQ-NAV.session_recalled -/
theorem everything_reopened_exists (files existing : List String) (shown : Option String) (p : String)
    (h : List.Mem p (reopened files shown existing)) : List.Mem p existing := by
  have kept := (List.mem_filter.mp h).2
  simpa using kept

end TraceLean.Nav
