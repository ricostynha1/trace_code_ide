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

private def cursorInFile : Focus :=
  { kind := BufferKind.file "a.rs", offset := 0, under := some "src/lib.rs" }

private def cursorInListing : Focus :=
  { kind := BufferKind.directory "src", offset := 0, under := some "src/lib.rs" }

private def cursorOverNothing : Focus :=
  { kind := BufferKind.directory "src", offset := 0, under := none }

private def noFiles : Workspace := {}

private def same (a b : Intent) : Bool :=
  Lean.toJson a == Lean.toJson b

/-- @proves REQ-ACT.focus_is_carried -/
theorem opening_follows_the_cursor :
    same (dispatch "file.open" cursorInFile noFiles [])
      (dispatch "file.open" cursorInListing noFiles []) = true := by
  native_decide

/-- @proves REQ-ACT.missing_target_is_refused -/
theorem a_missing_target_is_named :
    same (dispatch "file.open" cursorOverNothing noFiles [])
      (Intent.refuse (Blocked.needsTarget "file.open" "a path")) = true := by
  native_decide

/-- @proves REQ-ACT.unknown_is_refused -/
theorem an_unknown_action_is_refused :
    acts (dispatch "file.explode" cursorInFile noFiles []) = false := by
  native_decide

/-- @proves REQ-ACT.action_to_intent -/
theorem an_action_resolves_to_an_intent :
    same (dispatch "history.undo" cursorInFile noFiles []) (Intent.travel Move.back) = true ∧
    same (dispatch "file.save" cursorInFile noFiles []) Intent.persist = true ∧
    same (dispatch "design.fold_all" cursorInFile noFiles [])
      (Intent.observe Watch.designFold) = true := by
  native_decide

/-- @proves REQ-ACT.one_path -/
theorem a_key_and_an_affordance_agree :
    same (dispatch "design.toggle REQ-A" cursorOverNothing noFiles [])
      (dispatch "design.toggle" { cursorOverNothing with under := some "REQ-A" } noFiles []) = true ∧
    same (dispatch "file.open src/lib.rs" cursorOverNothing noFiles [])
      (dispatch "file.open" cursorInListing noFiles []) = true := by
  native_decide

def isEdit : Intent → Bool
  | Intent.edit _ => true
  | _ => false

/-- @proves REQ-ACT.edits_are_commands -/
theorem changes_are_commands :
    isEdit (dispatch "file.new" cursorInFile noFiles []) = true ∧
    isEdit (dispatch "file.rename" cursorInFile noFiles []) = true ∧
    isEdit (dispatch "trace.new_requirement REQ-NEW" cursorInFile noFiles []) = true ∧
    same (dispatch "file.delete" cursorInFile ⟨[("a.rs", "fn f() -> u8 0")]⟩ [])
      (Intent.edit (Command.deleteFile "a.rs" "fn f() -> u8 0")) = true := by
  native_decide

/-- @proves REQ-ACT.dispatch_is_pure -/
theorem dispatch_reads_only_its_arguments (a a' : String) (f f' : Focus) (w w' : Workspace)
    (ws ws' : List Command) (ha : a = a') (hf : f = f') (hw : w = w') (hws : ws = ws') :
    dispatch a f w ws = dispatch a' f' w' ws' := by
  subst ha hf hw hws
  rfl

/-- @proves REQ-ACT.dispatch_is_pure -/
theorem accepting_takes_the_changes_given :
    same (dispatch "observe.accept" cursorInFile noFiles [Command.createFile "n.rs"])
      (Intent.observe (Watch.accept (Command.batch [Command.createFile "n.rs"]))) = true ∧
    acts (dispatch "observe.accept" cursorInFile noFiles []) = false := by
  native_decide

end TraceLean.Act
