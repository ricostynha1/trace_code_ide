import TraceLean.Annotation

namespace TraceLean.Strength

open TraceLean.Annotation

open Lean (ToJson FromJson)

inductive Strength where
  | pinned (theoremName : String)
  | attempted (theoremName : String)
  | «open»
  | nondeterministic (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Strength.isSettled : Strength → Bool
  | .pinned _ => true
  | .nondeterministic _ => true
  | _ => false

structure StrengthLink where
  role : Role
  reqId : String
  clause : Option String
  anchor : String
  nondeterministic : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Obligation where
  symbol : String
  clauses : List (String × Option String)
  theorems : List String
  strength : Strength
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

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

def assoc? (xs : List (String × β)) (key : String) : Option β :=
  (xs.find? (·.1 == key)).map (·.2)

def assocSet (xs : List (String × β)) (key : String) (value : β) : List (String × β) :=
  if (xs.any (·.1 == key)) then xs.map (fun p => if p.1 == key then (key, value) else p)
  else xs ++ [(key, value)]

def assocPush (xs : List (String × List β)) (key : String) (value : β) : List (String × List β) :=
  match assoc? xs key with
  | some current => assocSet xs key (current ++ [value])
  | none => xs ++ [(key, [value])]

/-- @models REQ-STRENGTH.qualifies_proof
@models REQ-STRENGTH.open_is_the_default
@models REQ-STRENGTH.attempted_distinguished
@models REQ-STRENGTH.nondeterministic_declared -/
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

/-- @proves REQ-STRENGTH.open_is_the_default -/
theorem an_unpinned_model_is_open_and_not_a_pass :
    obligationsFrom [⟨.models, "REQ-A", some "one", "M::f", none⟩] =
      [⟨"M::f", [("REQ-A", some "one")], [], .«open»⟩]
    ∧ Strength.«open».isSettled = false := by
  constructor
  · native_decide
  · rfl

def leanName (symbol : String) : String :=
  String.mk (((symbol.replace "::" "_").replace "." "_").toList)

def clauseName (clause : String × Option String) : String :=
  match clause.2 with
  | some c => clause.1 ++ "." ++ c
  | none => clause.1

/-- @models REQ-STRENGTH.obligation_generated
@models REQ-STRENGTH.not_proved_by_us -/
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

/-- @proves REQ-STRENGTH.not_proved_by_us -/
theorem an_obligation_is_never_discharged :
    ((obligationSource { symbol := "TraceLean.assurance", clauses := [("REQ-EVID", some "weakest_link")],
                         theorems := ["t1"], strength := .pinned "t1" }).splitOn "sorry").length = 2
    ∧ ((obligationSource { symbol := "TraceLean.f", clauses := [], theorems := [],
                           strength := .open }).splitOn "sorry").length = 2 := by
  native_decide

def pinnedAt : List StrengthLink :=
  [⟨.models, "REQ-A", some "one", "M::f", none⟩, ⟨.proves, "REQ-A", some "one", "T::p", none⟩,
   ⟨.pins, "REQ-A", some "one", "T::pin", none⟩]

/-- @proves REQ-STRENGTH.obligation_generated -/
theorem each_model_owes_the_determining_question :
    (obligationsFrom pinnedAt).map (·.symbol) = ["M::f"] ∧
    ((obligationSource ((obligationsFrom pinnedAt).headD default)).splitOn
      "∀ f g, Spec_M_f f → Spec_M_f g → f = g").length = 2 := by
  native_decide

/-- @proves REQ-STRENGTH.attempted_distinguished -/
theorem an_attempt_is_its_own_state :
    ((obligationsFrom pinnedAt).map (·.strength)) = [.attempted "T::pin"] ∧
    Strength.attempted "T::pin" ≠ Strength.«open» ∧
    Strength.attempted "T::pin" ≠ Strength.pinned "T::pin" ∧
    (Strength.attempted "T::pin").isSettled = false := by
  native_decide

/-- @proves REQ-STRENGTH.nondeterministic_declared -/
theorem a_declared_nondeterminism_is_settled :
    ((obligationsFrom [⟨.models, "REQ-A", none, "M::clock", some "reads the time"⟩]).map (·.strength))
      = [.nondeterministic "reads the time"] ∧
    (Strength.nondeterministic "reads the time").isSettled = true := by
  native_decide

/-- @proves REQ-STRENGTH.qualifies_proof -/
theorem a_proof_comes_with_its_strength :
    (obligationsFrom pinnedAt).map (fun o => (o.theorems, o.strength))
      = [(["T::p"], .attempted "T::pin")] := by
  native_decide

end TraceLean.Strength
