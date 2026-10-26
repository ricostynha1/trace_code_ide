import Lean
import TraceLean.View
import TraceLean.Command
import TraceLean.Screen
import TraceLean.Context
import TraceLean.HistoryView

namespace TraceLean.Act

open TraceLean.Command
open TraceLean.Hash
open TraceLean.Screen (Arrangement Axis Direction)
open TraceLean.View

open Lean (ToJson FromJson)

structure Focus where
  kind : BufferKind
  offset : Nat
  under : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Move where
  | back
  | forward
  | branch
  | to (node : Nat)
  | base
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Watch where
  | start
  | accept (command : Command)
  | reject
  | create
  | copy
  | finish
  | copyAll
  | copyLine
  | copyPath (path : String)
  | acceptFile (path : String) (command : Command)
  | rejectFile (path : String)
  | contextToggle (part : TraceLean.Context.Part)
  | contextCopy
  | historyFilter (filter : TraceLean.HistoryView.Filter)
  | designToggle (id : String)
  | designExpand (id : Option String)
  | designFold
  | designOpenOnly
  deriving Repr, Inhabited, ToJson, FromJson

inductive Blocked where
  | unknownAction (action : String)
  | needsTarget (action : String) (what : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Intent where
  | display (what : BufferKind)
  | edit (command : Command)
  | travel (move : Move)
  | observe (watch : Watch)
  | arrange (how : Arrangement)
  | persist
  | refuse (why : Blocked)
  deriving Repr, Inhabited, ToJson, FromJson

private def report (title : String) : Intent :=
  Intent.display (BufferKind.record title)

private def focusPath (focus : Focus) : Option String :=
  match focus.kind with
  | BufferKind.file path => some path
  | BufferKind.directory _ => none
  | BufferKind.review _ => none
  | BufferKind.menu _ => none
  | BufferKind.record _ => none

private def openUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "file.open" "a path")
  | some path => Intent.display (BufferKind.file path)

private def newUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "file.new" "a name")
  | some path => Intent.edit (Command.createFile path)

private def renameUnder (focus : Focus) : Intent :=
  match focusPath focus with
  | none => Intent.refuse (Blocked.needsTarget "file.rename" "a file")
  | some from_ =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "file.rename" "a new name")
    | some to => Intent.edit (Command.renameFile from_ to)

private def deleteFocus (focus : Focus) (w : Workspace) : Intent :=
  match focusPath focus with
  | none => Intent.refuse (Blocked.needsTarget "file.delete" "a file")
  | some path =>
    match Workspace.get (Workspace.canon w) path with
    | none => Intent.refuse (Blocked.needsTarget "file.delete" "a file that exists")
    | some content => Intent.edit (Command.deleteFile path content)

private def diffUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "observe.diff" "a change")
  | some target => Intent.display (BufferKind.review target)

def nodeNumber (named : String) : Option Nat :=
  if named.startsWith "#" then
    let digits := named.drop 1
    if digits.isEmpty || digits.length > 18 || !digits.all Char.isDigit then none
    else some digits.toNat!
  else none

def requirementTemplate (id : String) : String :=
  "---\nid: " ++ id ++ "\ntitle: What this requirement is about\nstatus: draft\nclauses:\n  first: The system shall ...\n---\n\n# " ++ id ++ "\n\nWhy this requirement exists.\n"

private def findChars (pattern : List Char) : List Char → Nat → Option Nat
  | [], pos => if pattern.isEmpty then some pos else none
  | c :: rest, pos =>
    if pattern.isPrefixOf (c :: rest) then some pos else findChars pattern rest (pos + 1)

private def approveUnder (focus : Focus) (w : Workspace) : Intent :=
  let refuse := Intent.refuse (Blocked.needsTarget "trace.approve" "a draft requirement")
  match (focusPath focus).orElse (fun _ => focus.under) with
  | none => refuse
  | some path =>
    match Workspace.get (Workspace.canon w) path with
    | none => refuse
    | some text =>
      match findChars "\nstatus: draft\n".toList text.toList 0 with
      | none => refuse
      | some pos =>
        let offset := (String.mk (text.toList.take pos)).utf8ByteSize + "\nstatus: ".utf8ByteSize
        Intent.edit (Command.batch
          [Command.delete path offset "draft", Command.insert path offset "approved"])

private def changedFile (focus : Focus) : Option String :=
  match focus.kind with
  | BufferKind.review target => some target
  | _ => focus.under

private def identChar (c : Char) : Bool :=
  c.isAlphanum || c == '_' || c == '.' || c == '!'

def identifierAt (text : String) (offset : Nat) : Option String :=
  let chars := text.toList
  let here := min offset chars.length
  let before := ((chars.take here).reverse.takeWhile identChar).reverse
  let after := (chars.drop here).takeWhile identChar
  let word := before ++ after
  let unfronted := word.dropWhile (fun c => c == '.')
  let unended := (unfronted.reverse.dropWhile (fun c => c == '.')).reverse
  let unbanged := (unended.reverse.dropWhile (fun c => c == '!')).reverse
  if unbanged.isEmpty || unbanged.all Char.isDigit then none else some (String.mk unbanged)

private def nameAt (focus : Focus) (w : Workspace) : Option String :=
  match (focusPath focus).bind (fun path => Workspace.get (Workspace.canon w) path) with
  | some text => identifierAt text focus.offset
  | none => focus.under.filter (fun name => !name.isEmpty)

private def requirementUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "trace.requirement" "a requirement")
  | some id => Intent.display (BufferKind.record ("requirement " ++ id))

private def showUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "screen.show" "a buffer")
  | some buffer => Intent.arrange (Arrangement.showBuffer buffer)

private def stationIntent (station : String) : Intent :=
  match TraceLean.Screen.stationKind station with
  | some what => Intent.display what
  | none => Intent.refuse (Blocked.unknownAction ("screen.station." ++ station))

def dispatchNamed (action : String) (focus : Focus) (w : Workspace) (waiting : List Command) : Intent :=
  match action with
  | "file.open" => openUnder focus
  | "file.new" => newUnder focus
  | "file.rename" => renameUnder focus
  | "file.delete" => deleteFocus focus w
  | "trace.approve" => approveUnder focus w
  | "file.save" => Intent.persist
  | "history.undo" => Intent.travel Move.back
  | "history.redo" => Intent.travel Move.forward
  | "history.branch" => Intent.travel Move.branch
  | "history.jump" =>
    match focus.under.bind nodeNumber with
    | none => Intent.refuse (Blocked.needsTarget "history.jump" "a point in the history")
    | some 0 => Intent.travel Move.base
    | some (k + 1) => Intent.travel (Move.to k)
  | "history.tree" => report "history"
  | "observe.start" => Intent.observe Watch.start
  | "observe.accept" =>
    match waiting with
    | [] => Intent.refuse (Blocked.needsTarget "observe.accept" "a change waiting")
    | _ => Intent.observe (Watch.accept (Command.batch waiting))
  | "observe.reject" => Intent.observe Watch.reject
  | "observe.accept_file" =>
    match changedFile focus with
    | none => Intent.refuse (Blocked.needsTarget "observe.accept_file" "a changed file")
    | some path =>
      match waiting.filter (fun command => touched command == path) with
      | [] => Intent.refuse (Blocked.needsTarget "observe.accept_file" "a change waiting to that file")
      | taken => Intent.observe (Watch.acceptFile path (Command.batch taken))
  | "observe.reject_file" =>
    match changedFile focus with
    | none => Intent.refuse (Blocked.needsTarget "observe.reject_file" "a changed file")
    | some path => Intent.observe (Watch.rejectFile path)
  | "sandbox.new" => Intent.observe Watch.create
  | "sandbox.copy" => Intent.observe Watch.copy
  | "sandbox.end" => Intent.observe Watch.finish
  | "observe.diff" => diffUnder focus
  | "trace.check" => report "check"
  | "trace.evidence" => report "evidence"
  | "trace.findings" => report "findings"
  | "definitions.edit" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "definitions.edit" "a setting")
    | some setting => report ("edit " ++ setting)
  | "file.find" => report "find"
  | "file.find_lines" => report "find lines"
  | "trace.lock" => report "lock"
  | "trace.requirement" => requirementUnder focus
  | "trace.context" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "trace.context" "a requirement")
    | some id => report ("context " ++ id)
  | "trace.coverage" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "trace.coverage" "a requirement")
    | some id => report ("coverage " ++ id)
  | "context.toggle" =>
    match focus.under.bind TraceLean.Context.partNamed with
    | none => Intent.refuse (Blocked.needsTarget "context.toggle" "a part of the context")
    | some part => Intent.observe (Watch.contextToggle part)
  | "context.copy" => Intent.observe Watch.contextCopy
  | "history.filter" =>
    match focus.under.bind TraceLean.HistoryView.filterNamed with
    | none => Intent.refuse (Blocked.needsTarget "history.filter" "a view of the history")
    | some filter => Intent.observe (Watch.historyFilter filter)
  | "design.toggle" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "design.toggle" "a requirement")
    | some id => Intent.observe (Watch.designToggle id)
  | "design.expand_all" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "design.expand_all" "a requirement")
    | some id => Intent.observe (Watch.designExpand (some id))
  | "design.expand_everything" => Intent.observe (Watch.designExpand none)
  | "design.fold_all" => Intent.observe Watch.designFold
  | "design.open_only" => Intent.observe Watch.designOpenOnly
  | "trace.judge" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "trace.judge" "a clause")
    | some clause => report ("judge " ++ clause)
  | "file.copy" => Intent.observe Watch.copyAll
  | "file.copy_line" => Intent.observe Watch.copyLine
  | "file.copy_path" =>
    match focusPath focus with
    | none => Intent.refuse (Blocked.needsTarget "file.copy_path" "a file")
    | some path => Intent.observe (Watch.copyPath path)
  | "file.definition" =>
    match nameAt focus w with
    | none => Intent.refuse (Blocked.needsTarget "file.definition" "a name")
    | some name => report ("definition " ++ name)
  | "file.references" =>
    match nameAt focus w with
    | none => Intent.refuse (Blocked.needsTarget "file.references" "a name")
    | some name => report ("references " ++ name)
  | "trace.new_requirement" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "trace.new_requirement" "an identifier")
    | some id =>
      Intent.edit (Command.batch
        [Command.createFile ("reqs/" ++ id ++ ".md"),
         Command.insert ("reqs/" ++ id ++ ".md") 0 (requirementTemplate id)])
  | "trace.rollup" => report "rollup"
  | "trace.stale" => report "stale"
  | "drt.bindings" => report "drt bindings"
  | "drt.coverage" => report "drt coverage"
  | "drt.judge" => report "drt judge"
  | "drt.run" => report "drt run"
  | "drt.shrink" => report "drt shrink"
  | "screen.split.across" => Intent.arrange (Arrangement.split Axis.across)
  | "screen.split.down" => Intent.arrange (Arrangement.split Axis.down)
  | "screen.close" => Intent.arrange Arrangement.close
  | "screen.focus.left" => Intent.arrange (Arrangement.focus Direction.left)
  | "screen.focus.right" => Intent.arrange (Arrangement.focus Direction.right)
  | "screen.focus.up" => Intent.arrange (Arrangement.focus Direction.up)
  | "screen.focus.down" => Intent.arrange (Arrangement.focus Direction.down)
  | "screen.grow" => Intent.arrange (Arrangement.resize 1)
  | "screen.shrink" => Intent.arrange (Arrangement.resize (-1))
  | "screen.show" => showUnder focus
  | "screen.strip" => Intent.display (BufferKind.menu "opened")
  | "screen.offers" => Intent.display (BufferKind.menu "offers")
  | other =>
    if other.startsWith "screen.station." then
      stationIntent (other.drop "screen.station.".length)
    else
      Intent.refuse (Blocked.unknownAction other)

/-- @models REQ-ACT.action_to_intent
@models REQ-ACT.focus_is_carried
@models REQ-ACT.one_path
@models REQ-ACT.unknown_is_refused
@models REQ-ACT.missing_target_is_refused
@models REQ-ACT.edits_are_commands
@models REQ-ACT.dispatch_is_pure -/
def dispatch (action : String) (focus : Focus) (w : Workspace) (waiting : List Command) : Intent :=
  let focus := match actionTarget action with
    | some target => { focus with under := some target }
    | none => focus
  dispatchNamed (actionName action) focus w waiting

def acts : Intent → Bool
  | Intent.refuse _ => false
  | Intent.display _ => true
  | Intent.edit _ => true
  | Intent.travel _ => true
  | Intent.observe _ => true
  | Intent.arrange _ => true
  | Intent.persist => true

private def same (a b : Intent) : Bool :=
  Lean.toJson a == Lean.toJson b

/-- @proves REQ-ACT.dispatch_is_pure -/
theorem dispatch_reads_only_its_arguments (a a' : String) (f f' : Focus) (w w' : Workspace)
    (ws ws' : List Command) (ha : a = a') (hf : f = f') (hw : w = w') (hws : ws = ws') :
    dispatch a f w ws = dispatch a' f' w' ws' := by
  subst ha hf hw hws
  rfl

def namedActions : List String :=
  ["file.open",
   "file.new",
   "file.rename",
   "file.delete",
   "trace.approve",
   "file.save",
   "history.undo",
   "history.redo",
   "history.branch",
   "history.jump",
   "history.tree",
   "observe.start",
   "observe.accept",
   "observe.reject",
   "observe.accept_file",
   "observe.reject_file",
   "sandbox.new",
   "sandbox.copy",
   "sandbox.end",
   "observe.diff",
   "trace.check",
   "trace.evidence",
   "trace.findings",
   "definitions.edit",
   "file.find",
   "file.find_lines",
   "trace.lock",
   "trace.requirement",
   "trace.context",
   "trace.coverage",
   "context.toggle",
   "context.copy",
   "history.filter",
   "design.toggle",
   "design.expand_all",
   "design.expand_everything",
   "design.fold_all",
   "design.open_only",
   "trace.judge",
   "file.copy",
   "file.copy_line",
   "file.copy_path",
   "file.definition",
   "file.references",
   "trace.new_requirement",
   "trace.rollup",
   "trace.stale",
   "drt.bindings",
   "drt.coverage",
   "drt.judge",
   "drt.run",
   "drt.shrink",
   "screen.split.across",
   "screen.split.down",
   "screen.close",
   "screen.focus.left",
   "screen.focus.right",
   "screen.focus.up",
   "screen.focus.down",
   "screen.grow",
   "screen.shrink",
   "screen.show",
   "screen.strip",
   "screen.offers"]

theorem dispatch_factors (a b : String) (f : Focus) (w : Workspace) (ws : List Command)
    (hn : actionName a = actionName b) (ht : actionTarget a = actionTarget b) :
    dispatch a f w ws = dispatch b f w ws := by
  simp only [dispatch, hn, ht]

/-- @proves REQ-ACT.unknown_is_refused -/
theorem an_unnamed_action_is_refused (a : String) (f : Focus) (w : Workspace) (ws : List Command)
    (unnamed : actionName a ∉ namedActions)
    (notStation : (actionName a).startsWith "screen.station." = false) :
    dispatch a f w ws = .refuse (.unknownAction (actionName a)) := by
  unfold dispatch
  generalize actionName a = n at *
  unfold dispatchNamed
  split <;> simp_all [namedActions]

/-- @proves REQ-ACT.action_to_intent -/
theorem a_named_action_resolves_to_an_intent (a : String) (f : Focus) (w : Workspace)
    (ws : List Command) (named : actionName a ∈ namedActions) (x : String) :
    dispatch a f w ws ≠ .refuse (.unknownAction x) := by
  unfold dispatch
  generalize (match actionTarget a with
    | some target => { f with under := some target }
    | none => f) = g
  generalize actionName a = n at *
  unfold dispatchNamed
  split <;> simp_all [namedActions, openUnder, newUnder, renameUnder, deleteFocus, approveUnder,
    diffUnder, requirementUnder, showUnder, report] <;> (repeat' split) <;> simp

def underActions : List String :=
  ["file.open", "file.new", "observe.diff", "history.jump", "definitions.edit",
   "trace.requirement", "trace.context", "trace.coverage", "context.toggle", "history.filter",
   "design.toggle", "design.expand_all", "trace.judge", "trace.new_requirement", "screen.show"]

def fileActions : List String := ["file.rename", "file.delete", "file.copy_path"]

def mutatingActions : List String :=
  ["file.new", "file.rename", "file.delete", "trace.approve", "trace.new_requirement"]

/-- @proves REQ-ACT.missing_target_is_refused -/
theorem a_missing_target_is_refused_by_name (a : String) (f : Focus) (w : Workspace)
    (ws : List Command) (noTarget : actionTarget a = none) :
    (actionName a ∈ underActions → f.under = none →
      ∃ what, dispatch a f w ws = .refuse (.needsTarget (actionName a) what)) ∧
    (actionName a ∈ fileActions → (∀ p, f.kind ≠ .file p) →
      ∃ what, dispatch a f w ws = .refuse (.needsTarget (actionName a) what)) ∧
    (actionName a = "observe.accept" → ws = [] →
      ∃ what, dispatch a f w ws = .refuse (.needsTarget (actionName a) what)) := by
  simp only [dispatch, noTarget]
  generalize actionName a = n
  refine ⟨?_, ?_, ?_⟩
  · intro needs nothing
    unfold dispatchNamed
    split <;> simp_all [underActions, openUnder, newUnder, diffUnder, requirementUnder, showUnder]
  · intro needs notFile
    have none_path : focusPath f = none := by
      unfold focusPath
      cases h : f.kind <;> simp_all
    unfold dispatchNamed
    split <;> simp_all [fileActions, renameUnder, deleteFocus]
  · intro named empty
    unfold dispatchNamed
    split <;> simp_all

/-- @proves REQ-ACT.focus_is_carried -/
theorem the_named_target_is_the_focus (a t : String) (f : Focus) (w : Workspace)
    (ws : List Command) (named : actionTarget a = some t) (u : Option String) :
    dispatch a f w ws = dispatchNamed (actionName a) { f with under := some t } w ws ∧
    dispatch a f w ws = dispatch a { f with under := u } w ws := by
  simp [dispatch, named]

/-- @proves REQ-ACT.one_path -/
theorem a_key_and_an_affordance_agree (key affordance t : String) (f : Focus) (w : Workspace)
    (ws : List Command) (same : actionName key = actionName affordance)
    (bare : actionTarget key = none) (carried : actionTarget affordance = some t)
    (cursor : f.under = some t) :
    dispatch key f w ws = dispatch affordance f w ws := by
  have hf : { f with under := some t } = f := by
    cases f
    simp_all
  simp [dispatch, bare, carried, same, hf]

/-- @proves REQ-ACT.edits_are_commands -/
theorem a_change_carries_a_command (a : String) (f : Focus) (w : Workspace) (ws : List Command) :
    (actionName a ∈ mutatingActions →
      (∃ c, dispatch a f w ws = .edit c) ∨ ∃ why, dispatch a f w ws = .refuse why) ∧
    (actionName a = "observe.accept" →
      (∃ c, dispatch a f w ws = .observe (.accept c)) ∨ ∃ why, dispatch a f w ws = .refuse why) ∧
    (actionName a = "observe.accept_file" →
      (∃ p c, dispatch a f w ws = .observe (.acceptFile p c)) ∨
        ∃ why, dispatch a f w ws = .refuse why) := by
  unfold dispatch
  generalize (match actionTarget a with
    | some target => { f with under := some target }
    | none => f) = g
  dsimp only
  generalize actionName a = n
  refine ⟨?_, ?_, ?_⟩
  · intro m
    unfold dispatchNamed
    split <;> simp_all [mutatingActions, newUnder, renameUnder, deleteFocus, approveUnder] <;>
      (repeat' split) <;> simp
  · intro m
    unfold dispatchNamed
    split <;> simp_all <;> split <;> simp
  · intro m
    unfold dispatchNamed
    split <;> simp_all <;> (repeat' split) <;> simp

end TraceLean.Act
