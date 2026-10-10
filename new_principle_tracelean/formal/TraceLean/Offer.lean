import TraceLean.View
import TraceLean.Keymap
import TraceLean.Act

/-!
# What a pointer is offered

Models `REQ-ACT.everything_is_offered`: the context menu at a position is every
action a span declares there, the actions of the buffer it is in and the places
always reachable, each with the keys that reach it from the root mode, less any
the dispatcher does not know. What an
entry reads as is wording and not modelled; what it does, what it is about and
how to reach it by keys are.
-/

namespace TraceLean.Offer

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Keymap

/-- One entry, as far as the clause is concerned: its part of the menu, the
action, what it is about, and the keys that reach it. -/
structure Offered where
  group : String
  action : String
  target : Option String
  keys : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One mode's bindings in order: the keys of the first that dispatches
`action`, or else the modes it enters that were not seen yet. -/
def scanBindings (action : String) (route : List String) :
    List (String × Binding) → List String × List (String × List String) →
      String ⊕ (List String × List (String × List String))
  | [], state => .inr state
  | (key, binding) :: rest, (seen, next) =>
    match binding with
    | .dispatch bound _ =>
      if bound == action then .inl (" ".intercalate (route ++ [key]))
      else scanBindings action route rest (seen, next)
    | .enter entered _ =>
      if seen.contains entered then scanBindings action route rest (seen, next)
      else scanBindings action route rest (seen ++ [entered], next ++ [(entered, route ++ [key])])

/-- One level of the search: every mode in the frontier, in order. -/
def scanLevel (keymap : Keymap) (action : String) :
    List (String × List String) → List String × List (String × List String) →
      String ⊕ (List String × List (String × List String))
  | [], state => .inr state
  | (mode, route) :: rest, state =>
    match keymap.mode mode with
    | none => scanLevel keymap action rest state
    | some found =>
      match scanBindings action route found.bindings state with
      | .inl keys => .inl keys
      | .inr state => scanLevel keymap action rest state

def search (keymap : Keymap) (action : String) :
    Nat → List (String × List String) → List String → Option String
  | 0, _, _ => none
  | fuel + 1, frontier, seen =>
    if frontier.isEmpty then none
    else
      match scanLevel keymap action frontier (seen, []) with
      | .inl keys => some keys
      | .inr (seen, next) => search keymap action fuel next seen

/-- The keys that dispatch `action` from the root mode: breadth first, so the
shortest sequence wins, and among equals the first in the keymap's order.
Each level enters at least one mode not seen before, so there are no more
levels than modes. -/
def keysFor (keymap : Keymap) (action : String) : Option String :=
  search keymap action (keymap.modes.length + 1) [(keymap.root, [])] [keymap.root]

/-- The text of the first span under `offset`. -/
def under (buffer : Buffer) (offset : Nat) : Option String :=
  let cs := buffer.text.toList
  (buffer.spans.find? (fun span => span.start ≤ offset && offset < span.stop)).map fun span =>
    let start := min span.start cs.length
    String.mk ((cs.drop start).take (min span.stop cs.length - start))

def entry (keymap : Keymap) (group action : String) (target : Option String) : Offered :=
  { group := group, action := action, target := target, keys := keysFor keymap action }

/-- What a span declares at the position, with an accept and a reject for one
file beside a diff; a button's text is not what it is about. -/
def pointedAt (keymap : Keymap) (here : Option String) : List String → List Offered
  | [] => []
  | action :: rest =>
    let target := if action == "observe.accept_file" || action == "observe.reject_file" then none else here
    let extra :=
      if action == "observe.diff" then
        [entry keymap "here" "observe.accept_file" here, entry keymap "here" "observe.reject_file" here]
      else []
    entry keymap "here" action target :: extra ++ pointedAt keymap here rest

/-- What the buffer as a whole offers. -/
def ofBuffer (keymap : Keymap) (kind : BufferKind) : List Offered :=
  let e := entry keymap "file"
  [e "file.copy_line" none, e "file.copy" none] ++
  match kind with
  | .file path =>
    [e "file.definition" none, e "file.references" none, e "file.save" none,
     e "history.undo" none, e "history.redo" none, e "trace.evidence" none,
     e "file.rename" (some path), e "file.delete" (some path), e "file.copy_path" (some path)]
  | .directory path => [e "file.new" none, e "file.open" (some path)]
  | .review target =>
    [e "observe.accept" none, e "observe.reject" none,
     e "observe.accept_file" (some target), e "observe.reject_file" (some target)]
  | .record title =>
    (if title == "sandbox" then [e "sandbox.new" none, e "sandbox.copy" none, e "observe.start" none] else []) ++
    (if title == "observed" || title == "sandbox" then [e "observe.accept" none, e "observe.reject" none] else []) ++
    (if title == "sandbox" then [e "sandbox.end" none] else [])
  | .menu title =>
    (if title == "design" then [e "design.expand_everything" none, e "design.fold_all" none] else []) ++
    (if title == "requirements" || title == "design" then [e "trace.new_requirement" none] else [])

/-- The places always reachable, in the order the menu lists them. -/
def places : List String :=
  ["screen.station.project", "screen.station.trace", "screen.station.design", "screen.station.sandbox", "observe.start", "trace.check",
   "trace.findings", "history.tree"]

/-- Whether the dispatcher knows `action`: asked with nothing focused, it does
not refuse it as unknown -- wanting a target is still knowing it. -/
def known (action : String) : Bool :=
  match TraceLean.Act.dispatch action { kind := BufferKind.menu "", offset := 0, under := none } {} [] with
  | .refuse (.unknownAction _) => false
  | _ => true

/-- Everything offered at `offset` in `buffer`: what was pointed at, a listing
row's file operations, the buffer's own actions, the panes, and the places.

@models REQ-ACT.everything_is_offered -/
def offers (buffer : Buffer) (offset : Nat) (keymap : Keymap) : List Offered :=
  -- A span may declare anything; what the dispatcher would refuse as unknown
  -- is a button that does nothing, so it is not offered.
  let here := under buffer offset
  let declared := actionsAt buffer offset
  let row :=
    match buffer.kind, here with
    | .directory _, some path =>
      (if declared.contains "file.open" then ["file.rename", "file.delete", "file.copy_path"] else []).map
        (fun a => entry keymap "here" a (some path))
    | _, _ => []
  (pointedAt keymap here declared ++ row ++ ofBuffer keymap buffer.kind ++
    ["screen.split.across", "screen.split.down", "screen.close"].map (fun a => entry keymap "panes" a none) ++
    places.map (fun a => entry keymap "go" a none)).filter (fun o => known o.action)

end TraceLean.Offer
