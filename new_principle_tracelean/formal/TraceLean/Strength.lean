import TraceLean.Annotation

/-!
# Spec strength

Models `REQ-STRENGTH`. `@proves` says the model has a property. It says nothing
about how much that property rules out, and the gap is enormous: `discount s ≤ s`
is a genuine machine-checked theorem that the constant-zero function also
satisfies. Unqualified, a proof level is what a testing level would be without a
coverage floor.

The relational specification is the *set* of proved theorems, and the obligation
is whether everything satisfying them is this model. Four states, and the point
of the requirement is that they stay four: **open** is the default and is not a
pass, an obligation stated but unfinished is not the same as one never
attempted, and a model genuinely not determined by its inputs can be declared so
once, with a reason, rather than sitting open forever teaching everyone to
ignore the column.
-/

namespace TraceLean.Strength

open TraceLean.Annotation

open Lean (ToJson FromJson)

/-- What is known about one modelled declaration's strength. -/
inductive Strength where
  /-- A pinning theorem exists and is finished. -/
  | pinned (theoremName : String)
  /-- A pinning theorem exists and is not finished. -/
  | attempted (theoremName : String)
  /-- Nobody has said anything. The default, and not a failure -- but not a pass
  either, which is the whole point of naming the state. -/
  | «open»
  /-- Declared unpinnable, with a reason. -/
  | nondeterministic (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether this may be presented as a pass. `open` may not. -/
def Strength.isSettled : Strength → Bool
  | .pinned _ => true
  | .nondeterministic _ => true
  | _ => false

/-- A link reduced to what strength depends on. -/
structure StrengthLink where
  role : Role
  reqId : String
  clause : Option String
  anchor : String
  nondeterministic : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The obligation for one modelled declaration.

The question is about the *function*, so one declaration owes one obligation
however many clauses it serves. -/
structure Obligation where
  symbol : String
  clauses : List (String × Option String)
  theorems : List String
  strength : Strength
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-! ## Sorted, deduplicated collections

Written out rather than taken from a library, because the order obligations come
back in is part of what the two sides have to agree on. -/

def insertSorted [BEq α] (le : α → α → Bool) (xs : List α) (x : α) : List α :=
  match xs with
  | [] => [x]
  | y :: rest =>
    if x == y then y :: rest
    else if le x y then x :: y :: rest
    else y :: insertSorted le rest x

def sortedUnique [BEq α] (le : α → α → Bool) (xs : List α) : List α :=
  xs.foldl (insertSorted le) []

def clauseLe (a b : String × Option String) : Bool :=
  if a.1 != b.1 then decide (a.1 ≤ b.1)
  else match a.2, b.2 with
    | none, _ => true
    | some _, none => false
    | some x, some y => decide (x ≤ y)

/-- The value for a key, as an association list holds it. -/
def assoc? (xs : List (String × β)) (key : String) : Option β :=
  (xs.find? (·.1 == key)).map (·.2)

/-- Replace or append. -/
def assocSet (xs : List (String × β)) (key : String) (value : β) : List (String × β) :=
  if (xs.any (·.1 == key)) then xs.map (fun p => if p.1 == key then (key, value) else p)
  else xs ++ [(key, value)]

/-- Append to the list held for a key. -/
def assocPush (xs : List (String × List β)) (key : String) (value : β) : List (String × List β) :=
  match assoc? xs key with
  | some current => assocSet xs key (current ++ [value])
  | none => xs ++ [(key, [value])]

/--
Every modelled declaration's obligation, and where it stands.

A theorem or a pin is attributed to the model it is about, which is the model
serving the same clause. A pinning theorem that *exists* is not yet one that is
*finished*: only the kernel can say which, and that is recorded as evidence
rather than guessed here -- so `pinned` is never produced from links alone.

@models REQ-STRENGTH.qualifies_proof
@models REQ-STRENGTH.open_is_the_default
@models REQ-STRENGTH.attempted_distinguished
@models REQ-STRENGTH.nondeterministic_declared
-/
def obligationsFrom (links : List StrengthLink) : List Obligation :=
  let step := fun (acc : List (String × List (String × Option String))
                        × List (String × List String)
                        × List (String × String)
                        × List (String × String))
                  (link : StrengthLink) =>
    let (bySymbol, theorems, pins, nondet) := acc
    let key := (link.reqId, link.clause)
    match link.role with
    | .models =>
      let bySymbol := assocPush bySymbol link.anchor key
      let nondet := match link.nondeterministic with
        | some reason => assocSet nondet link.anchor reason
        | none => nondet
      (bySymbol, theorems, pins, nondet)
    | .proves =>
      let theorems := links.foldl
        (fun acc other =>
          if other.role == Role.models && other.reqId == link.reqId && other.clause == link.clause
          then assocPush acc other.anchor link.anchor else acc)
        theorems
      (bySymbol, theorems, pins, nondet)
    | .pins =>
      let pins := links.foldl
        (fun acc other =>
          if other.role == Role.models && other.reqId == link.reqId && other.clause == link.clause
          then assocSet acc other.anchor link.anchor else acc)
        pins
      (bySymbol, theorems, pins, nondet)
    | _ => acc
  let (bySymbol, theorems, pins, nondet) := links.foldl step ([], [], [], [])
  let symbols := sortedUnique (fun a b => decide (a ≤ b)) (bySymbol.map (·.1))
  symbols.map (fun symbol =>
    let clauses := sortedUnique clauseLe ((assoc? bySymbol symbol).getD [])
    let proved := sortedUnique (fun a b => decide (a ≤ b)) ((assoc? theorems symbol).getD [])
    let strength :=
      match assoc? nondet symbol with
      | some reason => Strength.nondeterministic reason
      | none => match assoc? pins symbol with
        | some theoremName => Strength.attempted theoremName
        | none => Strength.«open»
    { symbol := symbol, clauses := clauses, theorems := proved, strength := strength })

/-- A model with no pin and no declaration is open, and open is not settled.

@proves REQ-STRENGTH.open_is_the_default -/
theorem an_unpinned_model_is_open_and_not_a_pass :
    obligationsFrom [⟨.models, "REQ-A", some "one", "M::f", none⟩] =
      [⟨"M::f", [("REQ-A", some "one")], [], .«open»⟩]
    ∧ Strength.«open».isSettled = false := by
  constructor
  · native_decide
  · rfl

/-! ## The obligation itself

`obligation_generated` and `not_proved_by_us`. The system writes the question
and leaves it open. The generated text ends in `sorry`, so the kernel itself
agrees the obligation is unproved until somebody does the work — which is the
difference between a tool that helps you find out and one that tells you what
you wanted to hear.

An obligation with no theorems is deliberately unprovable as it stands: the
specification is `True`, everything satisfies it, and no two functions
satisfying it are equal. That is the honest rendering of "nothing is proved
about this model", and a generator that quietly emitted something provable
instead would hide exactly the case worth seeing.
-/

/-- A Lean identifier made from a symbol path. -/
def leanName (symbol : String) : String :=
  String.mk (((symbol.replace "::" "_").replace "." "_").toList)

/-- How a clause is written in the generated `@pins` annotation. -/
def clauseName (clause : String × Option String) : String :=
  match clause.2 with
  | some c => clause.1 ++ "." ++ c
  | none => clause.1

/--
The Lean source of an obligation.

@models REQ-STRENGTH.obligation_generated
@models REQ-STRENGTH.not_proved_by_us
-/
def obligationSource (obligation : Obligation) : String :=
  let name := leanName obligation.symbol
  let spec :=
    if obligation.theorems.isEmpty then
      "  -- No theorems are proved about this model, so the specification is empty\n" ++
      "  -- and everything satisfies it: this obligation is unprovable as it stands.\n" ++
      "  True"
    else
      String.intercalate " ∧\n"
        (obligation.theorems.map (fun t => "  -- from " ++ t ++ "\n  True"))
  let pins := String.intercalate " " (obligation.clauses.map clauseName)
  "-- Generated by TraceLean. The proof is yours to write.\n" ++
  "-- Does the specification determine the model, or merely constrain it?\n" ++
  "def Spec_" ++ name ++ " (f : _) : Prop :=\n" ++ spec ++ "\n\n" ++
  "/-- @pins " ++ pins ++ " -/\n" ++
  "theorem pins_" ++ name ++ " :\n" ++
  "    ∀ f g, Spec_" ++ name ++ " f → Spec_" ++ name ++ " g → f = g := by\n" ++
  "  sorry\n"

/-- Whatever the obligation, the generated proof is left open.

@proves REQ-STRENGTH.not_proved_by_us -/
theorem an_obligation_is_never_discharged :
    ((obligationSource { symbol := "TraceLean.assurance", clauses := [("REQ-EVID", some "weakest_link")],
                         theorems := ["t1"], strength := .pinned "t1" }).splitOn "sorry").length = 2
    ∧ ((obligationSource { symbol := "TraceLean.f", clauses := [], theorems := [],
                           strength := .open }).splitOn "sorry").length = 2 := by
  native_decide

end TraceLean.Strength
