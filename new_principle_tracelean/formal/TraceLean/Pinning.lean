import TraceLean.Strength

/-!
# Pinning, per input

Models the per-input half of `REQ-STRENGTH`. A clause with a specification
predicate `P` (its one `specifies` declaration) and a model function `f` (its
one `models` declaration, ADR-0014) owes
`(∀ x, P x (f x)) ∧ (∀ x y1 y2, P x y1 → P x y2 → y1 = y2)`: the function meets
the specification, and no input has two answers it accepts. A person proves it
in a `@pins` theorem; the system states it, asks Lean, and believes only a
clean answer about that theorem, for the declarations as they now are.
-/

namespace TraceLean.Pinning

open TraceLean.Strength

open Lean (ToJson FromJson)

/-- A verdict as it is kept. -/
structure PinRecord where
  theoremName : String
  key : String
  pinned : Bool
  said : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- ` x1 x2 … xn`. -/
def argsOf (n : Nat) : String :=
  String.join ((List.range n).map (fun i => " x" ++ toString (i + 1)))

/-- The obligation for a specification and a model of `inputs` arguments.

@models REQ-STRENGTH.per_input -/
def statement (spec model : String) (inputs : Nat) : String :=
  let args := argsOf inputs
  let meets :=
    if inputs == 0 then spec ++ " " ++ model
    else "∀" ++ args ++ ", " ++ spec ++ args ++ " (" ++ model ++ args ++ ")"
  "(" ++ meets ++ ") ∧ (∀" ++ args ++ " y1 y2, " ++ spec ++ args ++ " y1 → " ++ spec ++ args
    ++ " y2 → y1 = y2)"

/-! ## How many inputs a model takes

The obligation quantifies over the model's explicit arguments, so it is only
as right as their count. An argument is named in a binder before the colon, or
is only an arrow of the type after it: a definition by pattern matching names
none and still takes one per arrow. -/

/-- Where a scan of a declaration's header stands. -/
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

/-- How many blank-separated words. -/
def wordCount (cs : List Char) : Nat :=
  (cs.foldl (fun acc c =>
    if isBlank c then (acc.1, false) else if acc.2 then acc else (acc.1 + 1, true)) (0, false)).1

/-- The names a binder group declares: the words before its colon. -/
def bindersIn (group : List Char) : Nat :=
  if group.contains ':' then wordCount (group.takeWhile (· != ':')) else 0

/-- One character of the binders, before the top-level colon. -/
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

/-- One character of the type, after the colon; `none` where the first
pattern starts. -/
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

/-- The text from the first occurrence of `pat` on. -/
def suffixFrom (pat : List Char) : List Char → Option (List Char)
  | [] => if pat.isEmpty then some [] else none
  | c :: rest => if pat.isPrefixOf (c :: rest) then some (c :: rest) else suffixFrom pat rest

/-- The text before the first occurrence of `pat`. -/
def before (pat : List Char) : List Char → List Char
  | [] => []
  | c :: rest => if pat.isPrefixOf (c :: rest) then [] else c :: before pat rest

def fresh : Scan :=
  { depth := 0, names := 0, group := [], explicit := false, typed := false, arrows := 0,
    result := [] }

/-- A declaration's shape: whether it is a proposition, and how many explicit
arguments it takes, binders and arrows together. `none` for what is not a `def`.

@models REQ-STRENGTH.inputs_counted -/
def shape (declaration : String) : Option (Bool × Nat) :=
  let cs := declaration.toList
  let start := (suffixFrom "def ".toList cs).orElse (fun _ => suffixFrom "abbrev ".toList cs)
  start.map (fun rest =>
    let s := scan fresh (before ":=".toList rest)
    (s.typed && (String.mk s.result).trim == "Prop", s.names + s.arrows))

/-- A definition by pattern matching takes the arrows of its type.

@proves REQ-STRENGTH.inputs_counted -/
theorem a_pattern_matched_definition_counts_its_arrows :
    shape "def f (a : Nat) : List Nat → Nat → Prop\n  | [], _ => True\n  | _, _ => False" =
      some (true, 3) := by
  native_decide

/-- An arrow inside an argument's type is not an argument.

@proves REQ-STRENGTH.inputs_counted -/
theorem an_inner_arrow_is_not_an_input :
    shape "def h : (Nat → Nat) → Nat := fun f => f 0" = some (false, 1) := by
  native_decide

/-- Whether `pat` occurs in `text`, character by character. -/
def occursIn (pat text : List Char) : Bool :=
  match text with
  | [] => pat.isEmpty
  | c :: rest => pat.isPrefixOf (c :: rest) || occursIn pat rest

def holds (text pat : String) : Bool := occursIn pat.toList text.toList

/-- Whether one line of Lean's answer clears the theorem of `sorry`. -/
def clears (named line : String) : Bool :=
  named.toList.isPrefixOf line.toList
    && (holds line "does not depend on any axioms"
      || (holds line "depends on axioms" && !(holds line "sorryAx")))

/-- Whether Lean accepted the check.

@models REQ-STRENGTH.kernel_decides -/
def accepted (theoremName output : String) (exitedOk : Bool) : Bool :=
  exitedOk && !(holds output ": error")
    && (output.splitOn "\n").any (clears ("'" ++ theoremName ++ "'"))

/-- Where a clause stands.

@models REQ-STRENGTH.verdict_kept -/
def standing (theoremName : Option String) (record : Option PinRecord) (key : String) : Strength :=
  match theoremName, record with
  | none, _ => Strength.«open»
  | some t, some r =>
    if r.pinned && r.theoremName == t && r.key == key then Strength.pinned t
    else Strength.attempted t
  | some t, none => Strength.attempted t

/-- With no theorem a clause is open, whatever verdict lies about.

@proves REQ-STRENGTH.open_is_the_default -/
theorem no_theorem_is_open (record : Option PinRecord) (key : String) :
    standing none record key = Strength.«open» := by
  cases record
  all_goals rfl

/-- A verdict about other declarations does not pin.

@proves REQ-STRENGTH.verdict_kept -/
theorem another_key_does_not_pin (t key : String) (r : PinRecord) (h : r.key ≠ key) :
    standing (some t) (some r) key = Strength.attempted t := by
  simp [standing, h]

/-- Lean exiting with an error never pins.

@proves REQ-STRENGTH.kernel_decides -/
theorem a_failed_run_is_not_accepted (t output : String) : accepted t output false = false := by
  simp [accepted]

/-- The obligation is that the model meets the specification on every input and
that no input has two answers the specification accepts.

@proves REQ-STRENGTH.per_input -/
theorem the_obligation_meets_and_determines :
    statement "Spec" "f" 2
      = "(∀ x1 x2, Spec x1 x2 (f x1 x2)) ∧ (∀ x1 x2 y1 y2, Spec x1 x2 y1 → Spec x1 x2 y2 → y1 = y2)" ∧
    statement "Spec" "c" 0 = "(Spec c) ∧ (∀ y1 y2, Spec y1 → Spec y2 → y1 = y2)" := by
  native_decide

end TraceLean.Pinning
