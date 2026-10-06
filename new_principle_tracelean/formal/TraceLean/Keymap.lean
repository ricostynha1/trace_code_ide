import Lean

/-!
# The modal keymap

Models `REQ-MYTH`. A mode machine is a total function from a mode and a key to
one of four outcomes. Stating that as a law is the difference between an
interface that occasionally swallows a key and one that cannot.
-/

namespace TraceLean.Keymap

open Lean (ToJson FromJson)

/-- What a key does. Exactly four possibilities, closed. -/
inductive Outcome where
  | enter (mode : String)
  | dispatch (action : String)
  | leave (mode : String)
  | passThrough
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Binding where
  | enter (mode : String) (description : String)
  | dispatch (action : String) (description : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Mode where
  parent : Option String := none
  bindings : List (String × Binding) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Keymap where
  root : String
  modes : List (String × Mode) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def leaveKey : String := "Escape"

def Keymap.mode (k : Keymap) (name : String) : Option Mode :=
  (k.modes.find? (·.1 == name)).map (·.2)

def Mode.binding (m : Mode) (key : String) : Option Binding :=
  (m.bindings.find? (·.1 == key)).map (·.2)

/--
What a key does in a mode.

Total: every key in every mode has an outcome, and no key is swallowed. An
unbound key in the root mode is ordinary typing; an unbound key anywhere else
leaves the mode, so a mistyped leader never strands the user.

@models REQ-MYTH.totality
@models REQ-MYTH.escape_pops_one
@models REQ-MYTH.outcomes_closed
-/
def step (keymap : Keymap) (mode : String) (key : String) : Outcome :=
  match keymap.mode mode with
  | none => .passThrough
  | some current =>
    match current.binding key with
    | some (.enter target _) => .enter target
    | some (.dispatch action _) => .dispatch action
    | none =>
      if key == leaveKey then
        match current.parent with
        | some parent => .leave parent
        | none => .passThrough
      else
        match current.parent with
        | none => .passThrough
        | some _ => .leave keymap.root

/-- The mode a key leaves the machine in.

@models REQ-MYTH.totality -/
def nextMode (keymap : Keymap) (mode : String) (key : String) : String :=
  match step keymap mode key with
  | .enter target => target
  | .leave target => target
  | .dispatch _ => keymap.root
  | .passThrough => mode

/-! ## Which keys are available

`REQ-MYTH.whichkey_is_a_query`. The bar is computed from the same data that
dispatches the key, so it cannot describe a binding that does not exist or omit
one that does. A separately maintained list would drift, and the drift would be
invisible until somebody pressed the key the bar promised.
-/

def Binding.description : Binding → String
  | .enter _ d => d
  | .dispatch _ d => d

/-- Keys available in a mode, with what they do.

@models REQ-MYTH.whichkey_is_a_query -/
def whichKey (keymap : Keymap) (mode : String) : List (String × String) :=
  match keymap.mode mode with
  | none => []
  | some current =>
    let bound := current.bindings.map (fun b => (b.1, b.2.description))
    bound ++ (if current.parent.isSome then [(leaveKey, "leave this mode")] else [])

/-! ## Validating a keymap

The keymap is data a user edits, so it can be wrong. Everything wrong with it is
reported **at load**: a binding that fails when pressed is a keymap that works
until the moment somebody needs it.

Two of these are about reachability rather than about any single binding, which
is why they cannot be checked on the key that triggers them. A mode no sequence
reaches is dead configuration; an action no sequence reaches is a feature the
user cannot get to, and neither shows up by pressing keys.
-/

/-- What is wrong with a keymap. -/
inductive Problem where
  /-- A binding enters a mode that does not exist. -/
  | undefinedMode (mode key target : String)
  /-- A binding names an action the registry does not have. -/
  | undefinedAction (mode key action : String)
  /-- Leaving would not terminate. -/
  | parentCycle (mode : String)
  /-- A mode no key sequence reaches from the root. -/
  | unreachableMode (mode : String)
  /-- An action no key sequence reaches from the root. -/
  | unreachableAction (action : String)
  /-- The declared root does not exist. -/
  | noRoot (root : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Variant first, then content: the order a derived comparison gives, written
out because the report is shown to a person and has to be the same every run. -/
def Problem.key : Problem → Nat × List String
  | .undefinedMode m k t => (0, [m, k, t])
  | .undefinedAction m k a => (1, [m, k, a])
  | .parentCycle m => (2, [m])
  | .unreachableMode m => (3, [m])
  | .unreachableAction a => (4, [a])
  | .noRoot r => (5, [r])

def listLe : List String → List String → Bool
  | [], _ => true
  | _ :: _, [] => false
  | a :: as, b :: bs => if a != b then decide (a ≤ b) else listLe as bs

def problemLe (a b : Problem) : Bool :=
  let (ia, fa) := a.key
  let (ib, fb) := b.key
  if ia != ib then decide (ia ≤ ib) else listLe fa fb

def Keymap.hasMode (k : Keymap) (name : String) : Bool := (k.mode name).isSome

/-- Whether following parents from `mode` revisits something, which would make
leaving loop forever.

@models REQ-MYTH.escape_terminates -/
def parentCycleFrom (keymap : Keymap) : Nat → List String → Option String → Bool
  | 0, _, _ => true
  | _ + 1, _, none => false
  | fuel + 1, seen, some parent =>
    if seen.contains parent then true
    else parentCycleFrom keymap fuel (seen ++ [parent])
      ((keymap.mode parent).bind (·.parent))

/-- The modes a mode's bindings enter. -/
def enterTargets (m : Mode) : List String :=
  m.bindings.filterMap (fun b => match b.2 with | .enter target _ => some target | _ => none)

/-- Modes reachable from a starting set by following `enter` bindings. -/
def reachableFrom (keymap : Keymap) : Nat → List String → List String → List String
  | 0, seen, _ => seen
  | _ + 1, seen, [] => seen
  | fuel + 1, seen, name :: frontier =>
    match keymap.mode name with
    | none => reachableFrom keymap fuel seen frontier
    | some m =>
      -- Deduplicated before pushing, so the same mode is never queued twice.
      let fresh := (enterTargets m).filter (fun t => !seen.contains t)
      let queue := fresh.foldl (fun acc t => if acc.contains t then acc else acc ++ [t]) []
      reachableFrom keymap fuel (seen ++ queue) (frontier ++ queue)

/-- What one binding of a mode is wrong about: a mode that does not exist, or an
action nobody has.

A definition rather than a lambda with a typed binder inside `validate`, which
the grammar that reads these annotations cannot read (ADR-0008). -/
private def bindingProblem (keymap : Keymap) (actions : List String) (name : String)
    (acc : List Problem) (b : String × Binding) : List Problem :=
  match b.2 with
  | .enter target _ =>
    if keymap.hasMode target then acc else acc ++ [Problem.undefinedMode name b.1 target]
  | .dispatch action _ =>
    if actions.contains action then acc
    else acc ++ [Problem.undefinedAction name b.1 action]

/-- Everything one mode is wrong about, its bindings included. -/
private def modeProblems (keymap : Keymap) (actions : List String) (acc : List Problem)
    (entry : String × Mode) : List Problem :=
  let name := entry.1
  let m := entry.2
  let bindingProblems := m.bindings.foldl (bindingProblem keymap actions name) acc
  let budget := keymap.modes.length + 2
  match parentCycleFrom keymap budget [name] m.parent with
  | true => bindingProblems ++ [Problem.parentCycle name]
  | false => bindingProblems

/-- The actions a mode dispatches, added to what is already reachable. -/
private def dispatchableIn (keymap : Keymap) (acc : List String) (name : String) : List String :=
  match keymap.mode name with
  | none => acc
  | some m =>
    m.bindings.foldl
      (fun seen b =>
        match b.2 with
        | .dispatch action _ => if seen.contains action then seen else seen ++ [action]
        | _ => seen)
      acc

/--
Check a keymap against the actions that exist.

@models REQ-MYTH.modes_defined
@models REQ-MYTH.actions_defined
@models REQ-MYTH.escape_terminates
@models REQ-MYTH.actions_reachable
@models REQ-MYTH.keymap_is_data
-/
def validate (keymap : Keymap) (actions : List String) : List Problem :=
  let actions := (actions.foldl (fun acc a => if acc.contains a then acc else acc ++ [a])
    []).mergeSort (fun a b => decide (a ≤ b))
  let noRoot := if keymap.hasMode keymap.root then [] else [Problem.noRoot keymap.root]
  let perMode := keymap.modes.foldl (modeProblems keymap actions) []
  let reachable := reachableFrom keymap (keymap.modes.length + 2) [keymap.root] [keymap.root]
  let unreachableModes := (keymap.modes.map (·.1)).filterMap
    (fun name => if reachable.contains name then none else some (Problem.unreachableMode name))
  let dispatchable := reachable.foldl (dispatchableIn keymap) []
  let unreachableActions := actions.filterMap
    (fun a => if dispatchable.contains a then none else some (Problem.unreachableAction a))
  (noRoot ++ perMode ++ unreachableModes ++ unreachableActions).mergeSort problemLe

/-- A keymap whose root does not exist is reported, rather than failing on the
first key pressed.

@proves REQ-MYTH.keymap_is_data -/
theorem a_missing_root_is_reported_at_load :
    validate { root := "Main", modes := [] } [] = [Problem.noRoot "Main"] := by
  native_decide

end TraceLean.Keymap
