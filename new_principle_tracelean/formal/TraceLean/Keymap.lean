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

/-- The mode a key leaves the machine in. A helper over `step`, which is the
model of `totality`. -/
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
  /-- A mode's parent does not exist: leaving would land nowhere. -/
  | undefinedParent (mode parent : String)
  /-- Repeated Escape from this mode stops short of the root, in a mode that
  has no parent and does not bind Escape, or in one that does not exist. -/
  | stranded (mode : String)
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
  | .undefinedParent m p => (6, [m, p])
  | .stranded m => (7, [m])

def listLe : List String → List String → Bool
  | [], _ => true
  | _ :: _, [] => false
  | a :: as, b :: bs => if a != b then decide (a ≤ b) else listLe as bs

def problemLe (a b : Problem) : Bool :=
  let (ia, fa) := a.key
  let (ib, fb) := b.key
  if ia != ib then decide (ia ≤ ib) else listLe fa fb

def Keymap.hasMode (k : Keymap) (name : String) : Bool := (k.mode name).isSome

/-- Where pressing Escape over and over ends. -/
inductive Leaving where
  /-- At the root, which Escape does not move away from. -/
  | reachesRoot
  /-- Back in a mode already passed: it would go round for ever. -/
  | loops
  /-- In a mode other than the root where Escape does nothing: the mode has no
  parent and does not bind Escape, or it does not exist. -/
  | stops
  deriving Repr, DecidableEq

/-- Press Escape from `mode` until something settles, following the machine
itself (`step`), so a mode that binds Escape (as an insert mode does) is
followed through its binding rather than its parent. Each step visits a name
not in `seen`, and only the last can be a name that is no mode, so
`modes.length + 2` fuel always suffices. A helper of `validate`, which models
`escape_terminates`. -/
def leaveFrom (keymap : Keymap) : Nat → List String → String → Leaving
  | 0, _, _ => .loops
  | fuel + 1, seen, mode =>
    match step keymap mode leaveKey with
    | .dispatch _ => .reachesRoot
    | .passThrough => if mode == keymap.root then .reachesRoot else .stops
    | .enter target =>
      if seen.contains target then .loops else leaveFrom keymap fuel (seen ++ [target]) target
    | .leave target =>
      if seen.contains target then .loops else leaveFrom keymap fuel (seen ++ [target]) target

/-- The modes a mode's bindings enter. -/
def enterTargets (m : Mode) : List String :=
  m.bindings.filterMap (fun b => match b.2 with | .enter target _ => some target | _ => none)

/-- Modes reachable from a starting set by following `enter` bindings.

Only modes that exist are queued (a dangling target is `undefinedMode`, not a
place to explore), and none twice, so every unit of fuel after the first is
spent on a distinct mode of the keymap: `modes.length + 1` always suffices. A
queue that also held dangling names could run out first and report real
modes unreachable. -/
def reachableFrom (keymap : Keymap) : Nat → List String → List String → List String
  | 0, seen, _ => seen
  | _ + 1, seen, [] => seen
  | fuel + 1, seen, name :: frontier =>
    match keymap.mode name with
    | none => reachableFrom keymap fuel seen frontier
    | some m =>
      -- Deduplicated before pushing, so the same mode is never queued twice.
      let fresh := (enterTargets m).filter (fun t => !seen.contains t && keymap.hasMode t)
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

/-- A mode's parent is a transition target (`step` leaves to it), so it must
exist. -/
private def parentProblem (keymap : Keymap) (name : String) : Option String → List Problem
  | none => []
  | some parent => if keymap.hasMode parent then [] else [Problem.undefinedParent name parent]

/-- What repeated Escape from a mode is wrong about. -/
private def leavingProblem (name : String) : Leaving → List Problem
  | .reachesRoot => []
  | .loops => [Problem.parentCycle name]
  | .stops => [Problem.stranded name]

/-- Everything one mode is wrong about, its bindings included. -/
private def modeProblems (keymap : Keymap) (actions : List String) (acc : List Problem)
    (entry : String × Mode) : List Problem :=
  let name := entry.1
  let m := entry.2
  let bindingProblems := m.bindings.foldl (bindingProblem keymap actions name) acc
  let parentProblems := parentProblem keymap name m.parent
  let budget := keymap.modes.length + 2
  bindingProblems ++ parentProblems ++ leavingProblem name (leaveFrom keymap budget [name] name)

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
  let reachable := reachableFrom keymap (keymap.modes.length + 1) [keymap.root] [keymap.root]
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

/-- The two keymaps the first review found `validate` passing although Escape
never reaches the root: a mode with no parent, and a mode whose parent does
not exist. A parentless mode that binds Escape back to the root (an insert
mode) is fine.

@proves REQ-MYTH.escape_terminates -/
theorem a_mode_leaving_nowhere_is_reported :
    validate { root := "A", modes := [("A", { bindings := [("b", .enter "B" "")] }),
                                      ("B", {})] } []
      = [Problem.stranded "B"]
    ∧ validate { root := "A", modes := [("A", { bindings := [("b", .enter "B" "")] }),
                                        ("B", { parent := some "Ghost" })] } []
      = [Problem.undefinedParent "B" "Ghost", Problem.stranded "B"]
    ∧ validate { root := "A", modes := [("A", { bindings := [("i", .enter "I" "")] }),
                                        ("I", { bindings := [("Escape", .enter "A" "")] })] } []
      = [] := by
  native_decide

/-- Dangling `enter` targets queued ahead of a real mode used to spend the
search's fuel, so a mode the root does reach was reported unreachable. -/
theorem dangling_targets_do_not_hide_a_reachable_mode :
    validate { root := "A", modes := [
        ("A", { bindings := [("1", .enter "G1" ""), ("2", .enter "G2" ""),
                             ("3", .enter "G3" ""), ("4", .enter "B" "")] }),
        ("B", { parent := some "A" })] } []
      = [Problem.undefinedMode "A" "1" "G1", Problem.undefinedMode "A" "2" "G2",
         Problem.undefinedMode "A" "3" "G3"] := by
  native_decide

end TraceLean.Keymap
