import TraceLean.View
import TraceLean.Keymap
import TraceLean.Act

namespace TraceLean.Offer

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Keymap

structure Offered where
  group : String
  action : String
  target : Option String
  keys : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

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

def keysFor (keymap : Keymap) (action : String) : Option String :=
  search keymap action (keymap.modes.length + 1) [(keymap.root, [])] [keymap.root]

def under (buffer : Buffer) (offset : Nat) : Option String :=
  let cs := buffer.text.toList
  (buffer.spans.find? (fun span => span.start ≤ offset && offset < span.stop)).map fun span =>
    let start := min span.start cs.length
    String.mk ((cs.drop start).take (min span.stop cs.length - start))

def entry (keymap : Keymap) (group action : String) (target : Option String) : Offered :=
  { group := group, action := action, target := target, keys := keysFor keymap action }

def pointedAt (keymap : Keymap) (here : Option String) : List String → List Offered
  | [] => []
  | action :: rest =>
    let target := if action == "observe.accept_file" || action == "observe.reject_file" then none else here
    let extra :=
      if action == "observe.diff" then
        [entry keymap "here" "observe.accept_file" here, entry keymap "here" "observe.reject_file" here]
      else []
    entry keymap "here" action target :: extra ++ pointedAt keymap here rest

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
    (if title == "requirements" || title == "design" then
      [e "design.open_only" none, e "trace.new_requirement" none] else [])

def places : List String :=
  ["screen.station.project", "screen.station.trace", "screen.station.design", "screen.station.sandbox", "observe.start", "trace.check",
   "trace.findings", "history.tree"]

def known (action : String) : Bool :=
  match TraceLean.Act.dispatch action { kind := BufferKind.menu "", offset := 0, under := none } {} [] with
  | .refuse (.unknownAction _) => false
  | _ => true

/-- @models REQ-ACT.everything_is_offered -/
def offers (buffer : Buffer) (offset : Nat) (keymap : Keymap) : List Offered :=
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

def leader : Keymap :=
  ⟨"Normal", [("Normal", ⟨none, [("Space", .enter "Leader" "leader")]⟩),
              ("Leader", ⟨some "Normal", [("g", .enter "Design" "design")]⟩),
              ("Design", ⟨some "Leader", [("e", .dispatch "design.expand_everything" "all")]⟩)]⟩

def marked : Buffer :=
  { id := "d", kind := .menu "design", text := "▸ REQ-A",
    spans := [⟨0, 1, .entry, ["design.toggle REQ-A", "file.explode"]⟩] }

/-- @proves REQ-ACT.everything_is_offered -/
theorem the_menu_holds_everything_that_applies :
    (offers marked 0 leader).map (·.action)
      = ["design.toggle REQ-A", "file.copy_line", "file.copy", "design.expand_everything",
         "design.fold_all", "design.open_only", "trace.new_requirement", "screen.split.across", "screen.split.down",
         "screen.close"] ++ places ∧
    ((offers marked 0 leader).find? (·.action == "design.expand_everything")).bind (·.keys)
      = some "Space g e" := by
  native_decide

end TraceLean.Offer
