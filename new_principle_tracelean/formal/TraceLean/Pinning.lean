import TraceLean.Strength

/-!
# Pinning, per input

Models the per-input half of `REQ-STRENGTH`. A clause modelled by a
specification predicate `P` and a function `f` owes
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

end TraceLean.Pinning
