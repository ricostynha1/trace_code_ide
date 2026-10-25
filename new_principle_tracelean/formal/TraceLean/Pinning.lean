import TraceLean.Strength

namespace TraceLean.Pinning

open TraceLean.Strength

open Lean (ToJson FromJson)

structure PinRecord where
  theoremName : String
  key : String
  pinned : Bool
  said : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def argsOf (n : Nat) : String :=
  String.join ((List.range n).map (fun i => " x" ++ toString (i + 1)))

/-- @models REQ-STRENGTH.per_input -/
def statement (spec model : String) (inputs : Nat) : String :=
  let args := argsOf inputs
  let meets :=
    if inputs == 0 then spec ++ " " ++ model
    else "∀" ++ args ++ ", " ++ spec ++ args ++ " (" ++ model ++ args ++ ")"
  "(" ++ meets ++ ") ∧ (∀" ++ args ++ " y1 y2, " ++ spec ++ args ++ " y1 → " ++ spec ++ args
    ++ " y2 → y1 = y2)"

structure Scan where
  depth : Nat
  names : Nat
  group : List Char
  explicit : Bool
  typed : Bool
  arrows : Nat
  result : List Char
  deriving Repr, Inhabited

def isOpener (c : Char) : Bool := c == '(' || c == '{' || c == '['

def isCloser (c : Char) : Bool := c == ')' || c == '}' || c == ']'

def isBlank (c : Char) : Bool := c == ' ' || c == '\t' || c == '\n' || c == '\r'

def wordCount (cs : List Char) : Nat :=
  (cs.foldl (fun acc c =>
    if isBlank c then (acc.1, false) else if acc.2 then acc else (acc.1 + 1, true)) (0, false)).1

def bindersIn (group : List Char) : Nat :=
  if group.contains ':' then wordCount (group.takeWhile (· != ':')) else 0

def stepBinders (s : Scan) (c : Char) : Scan :=
  if isOpener c then
    if s.depth == 0 then { s with depth := 1, group := [], explicit := c == '(' }
    else { s with depth := s.depth + 1, group := s.group ++ [c] }
  else if isCloser c then
    if s.depth - 1 > 0 then { s with depth := s.depth - 1, group := s.group ++ [c] }
    else if s.explicit then { s with depth := 0, names := s.names + bindersIn s.group }
    else { s with depth := 0 }
  else if c == ':' && s.depth == 0 then { s with typed := true }
  else if s.depth ≥ 1 then { s with group := s.group ++ [c] }
  else s

def stepType (s : Scan) (c : Char) : Option Scan :=
  if isOpener c then some { s with depth := s.depth + 1, result := s.result ++ [c] }
  else if isCloser c then some { s with depth := s.depth - 1, result := s.result ++ [c] }
  else if s.depth == 0 && c == '|' then none
  else if s.depth == 0 && c == '→' then some { s with arrows := s.arrows + 1, result := [] }
  else if s.depth == 0 && c == '>' && s.result.getLast? == some '-' then
    some { s with arrows := s.arrows + 1, result := [] }
  else some { s with result := s.result ++ [c] }

def scan : Scan → List Char → Scan
  | s, [] => s
  | s, c :: rest =>
    if s.typed then
      match stepType s c with
      | none => s
      | some t => scan t rest
    else scan (stepBinders s c) rest

def suffixFrom (pat : List Char) : List Char → Option (List Char)
  | [] => if pat.isEmpty then some [] else none
  | c :: rest => if pat.isPrefixOf (c :: rest) then some (c :: rest) else suffixFrom pat rest

def before (pat : List Char) : List Char → List Char
  | [] => []
  | c :: rest => if pat.isPrefixOf (c :: rest) then [] else c :: before pat rest

def fresh : Scan :=
  { depth := 0, names := 0, group := [], explicit := false, typed := false, arrows := 0,
    result := [] }

/-- @models REQ-STRENGTH.inputs_counted -/
def shape (declaration : String) : Option (Bool × Nat) :=
  let cs := declaration.toList
  let start := (suffixFrom "def ".toList cs).orElse (fun _ => suffixFrom "abbrev ".toList cs)
  start.map (fun rest =>
    let s := scan fresh (before ":=".toList rest)
    (s.typed && (String.mk s.result).trim == "Prop", s.names + s.arrows))

def occursIn (pat text : List Char) : Bool :=
  match text with
  | [] => pat.isEmpty
  | c :: rest => pat.isPrefixOf (c :: rest) || occursIn pat rest

def holds (text pat : String) : Bool := occursIn pat.toList text.toList

def clears (named line : String) : Bool :=
  named.toList.isPrefixOf line.toList
    && (holds line "does not depend on any axioms"
      || (holds line "depends on axioms" && !(holds line "sorryAx")))

/-- @models REQ-STRENGTH.kernel_decides -/
def accepted (theoremName output : String) (exitedOk : Bool) : Bool :=
  exitedOk && !(holds output ": error")
    && (output.splitOn "\n").any (clears ("'" ++ theoremName ++ "'"))

/-- @models REQ-STRENGTH.verdict_kept -/
def standing (theoremName : Option String) (record : Option PinRecord) (key : String) : Strength :=
  match theoremName, record with
  | none, _ => Strength.«open»
  | some t, some r =>
    if r.pinned && r.theoremName == t && r.key == key then Strength.pinned t
    else Strength.attempted t
  | some t, none => Strength.attempted t

/-- @proves REQ-STRENGTH.open_is_the_default -/
theorem no_theorem_is_open (record : Option PinRecord) (key : String) :
    standing none record key = Strength.«open» := by
  cases record
  all_goals rfl

/-- @proves REQ-STRENGTH.verdict_kept -/
theorem another_key_does_not_pin (t key : String) (r : PinRecord) (h : r.key ≠ key) :
    standing (some t) (some r) key = Strength.attempted t := by
  simp [standing, h]

/-- @proves REQ-STRENGTH.kernel_decides -/
theorem a_failed_run_is_not_accepted (t output : String) : accepted t output false = false := by
  simp [accepted]

end TraceLean.Pinning
