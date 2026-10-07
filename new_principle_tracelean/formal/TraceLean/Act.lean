import Lean
import TraceLean.View
import TraceLean.Command
import TraceLean.Screen
import TraceLean.Context
import TraceLean.HistoryView

/-!
# From an action name to something the editor does

Models `REQ-ACT`. `REQ-MYTH` turns a key into an action *name*; `REQ-VIEW` puts
action names on spans, so a rendered affordance carries one too. Neither says
what an action does, and without that the editor has two ends of a wire and no
middle.

This is the middle, and it is one function. A key and a button carrying the same
name reach it with the same argument and get the same answer, which is why a
frontend cannot grow behaviour the other one lacks.

Resolution takes the *focus* as well as the action: the buffer, the offset, and
the text of the span under it. That is what makes `file.open` mean anything --
the path it opens is the path under the cursor, which the span already marks.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.Act

open TraceLean.Command
open TraceLean.Hash
open TraceLean.Screen (Arrangement Axis Direction)
open TraceLean.View

open Lean (ToJson FromJson)

/-- Where the cursor is and what is under it.

`under` is the text of the span the cursor is in, which is how an action learns
what it is about. A cursor in open space has none, and an action needing one
refuses rather than guessing. -/
structure Focus where
  /-- The buffer the cursor is in. -/
  kind : BufferKind
  offset : Nat
  under : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A move through the history tree. -/
inductive Move where
  | back
  | forward
  | branch
  /-- Straight to one node of the history, by its number. -/
  | to (node : Nat)
  /-- Back to the tree as it was opened, before any change. -/
  | base
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A change to what is being watched, or to what a watch produced. -/
inductive Watch where
  | start
  | accept
  | reject
  /-- Make a sandbox: a copy of the project for an agent the user starts. -/
  | create
  /-- Hand the user the command that enters the sandbox. -/
  | copy
  /-- Discard the sandbox. -/
  | finish
  /-- Put the focused buffer's whole text on the clipboard. -/
  | copyAll
  /-- Put the line the cursor is on on the clipboard. -/
  | copyLine
  /-- Put a file's path on the clipboard. -/
  | copyPath (path : String)
  /-- Take one file's observed change in, leaving the rest waiting. -/
  | acceptFile (path : String)
  /-- Take one file's observed change back out, leaving the rest waiting. -/
  | rejectFile (path : String)
  /-- Include a part of an agent's context, or leave it out. -/
  | contextToggle (part : TraceLean.Context.Part)
  /-- Put the chosen parts of an agent's context on the clipboard. -/
  | contextCopy
  /-- Show the history whole, for the open file, or at its saves. -/
  | historyFilter (filter : TraceLean.HistoryView.Filter)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Why nothing happened.

A key that appears to do nothing is the failure nobody reports, so the reason is
a value the editor can show.

@models REQ-ACT.unknown_is_refused -/
inductive Blocked where
  | unknownAction (action : String)
  | needsTarget (action : String) (what : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What the editor is to do next.

@models REQ-ACT.action_to_intent -/
inductive Intent where
  /-- Produce this buffer and show it. Which producer that is follows from the
  kind, and the shell knows that producing a report means computing one. -/
  | display (what : BufferKind)
  /-- Change the workspace, through the one path that has an inverse. -/
  | edit (command : Command)
  | travel (move : Move)
  | observe (watch : Watch)
  /-- Change the arrangement: split, close, move the focus, resize, show.
  Carried as a named change rather than performed here, so that a key and a
  pointer reach the one function that performs it (`REQ-SCREEN`). -/
  | arrange (how : Arrangement)
  /-- Write the state where it belongs. -/
  | persist
  | refuse (why : Blocked)
  -- No `DecidableEq`: `Command` is recursive and does not derive one, and this
  -- type is only ever compared through its JSON encoding.
  deriving Repr, Inhabited, ToJson, FromJson

/-- The buffer a report appears in. Reports are records: something computed and
shown, not something chosen. -/
private def report (title : String) : Intent :=
  Intent.display (BufferKind.record title)

/-- The path of the file the cursor is in, if it is in one. -/
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

/-- Deleting carries the content it removed, so the command has an inverse
without going back to the workspace for it. That is why this needs the
workspace: the witness is part of the command. -/
private def deleteFocus (focus : Focus) (w : Workspace) : Intent :=
  match focusPath focus with
  | none => Intent.refuse (Blocked.needsTarget "file.delete" "a file")
  | some path =>
    -- Through `canon`: a workspace is a map, and an encoding that lists a
    -- path twice means its later entry, as building the map does.
    match Workspace.get (Workspace.canon w) path with
    | none => Intent.refuse (Blocked.needsTarget "file.delete" "a file that exists")
    | some content => Intent.edit (Command.deleteFile path content)

private def diffUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "observe.diff" "a change")
  | some target => Intent.display (BufferKind.review target)

/-- The node a history row names, as `#12`: digits only, and few enough to be a
number on both sides of the model. -/
def nodeNumber (named : String) : Option Nat :=
  if named.startsWith "#" then
    let digits := named.drop 1
    if digits.isEmpty || digits.length > 18 || !digits.all Char.isDigit then none
    else some digits.toNat!
  else none

/-- A new requirement's document: the frontmatter every requirement has, a
first clause to rewrite, and a heading. A draft until someone approves it. -/
def requirementTemplate (id : String) : String :=
  "---\nid: " ++ id ++ "\ntitle: What this requirement is about\nstatus: draft\nclauses:\n  first: The system shall ...\n---\n\n# " ++ id ++ "\n\nWhy this requirement exists.\n"

/-- Where `pattern` first occurs in a text, counted in characters. Written out
rather than through `String.splitOn`, whose matching is not the obvious one
after a partial match. -/
private def findChars (pattern : List Char) : List Char → Nat → Option Nat
  | [], pos => if pattern.isEmpty then some pos else none
  | c :: rest, pos =>
    if pattern.isPrefixOf (c :: rest) then some pos else findChars pattern rest (pos + 1)

/-- Approving a draft requirement: the document the cursor is in, else the one
it is on, its first `status: draft` line made `status: approved` — an edit like
any other. -/
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
        let offset := pos + "\nstatus: ".length
        Intent.edit (Command.batch
          [Command.delete path offset "draft", Command.insert path offset "approved"])

/-- The file a change is about: the one a review shows, else the one under the
cursor. -/
private def changedFile (focus : Focus) : Option String :=
  match focus.kind with
  | BufferKind.review target => some target
  | _ => focus.under

/-- Open the requirement the cursor is on: its clauses and what claims each. -/
private def requirementUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "trace.requirement" "a requirement")
  | some id => Intent.display (BufferKind.record ("requirement " ++ id))

/-- Show the buffer the cursor is on, which is how a row of the strip works. -/
private def showUnder (focus : Focus) : Intent :=
  match focus.under with
  | none => Intent.refuse (Blocked.needsTarget "screen.show" "a buffer")
  | some buffer => Intent.arrange (Arrangement.showBuffer buffer)

/-! ### The stations

A station is an action of its own rather than one action taking the station's
name as a target. A station has to be reachable from a bare keyboard, and a
target comes from what the cursor is on -- so one `screen.station` would be
reachable only when a station row was already under the cursor, which is only
true once you have got to the stations, which is what the action was for.

What each one opens is `TraceLean.Screen.stationKind`'s answer and not a case
here: a window that knew `requirements` meant the requirement index and a
terminal that did not would be two editors, and a list of stations written twice
is a list that drifts. -/

private def stationIntent (station : String) : Intent :=
  match TraceLean.Screen.stationKind station with
  | some what => Intent.display what
  | none => Intent.refuse (Blocked.unknownAction ("screen.station." ++ station))

/--
What an action means, here, now.

Total: every name the keymap can dispatch has an answer, and every name it
cannot has a refusal. Nothing falls through.

@models REQ-ACT.action_to_intent
@models REQ-ACT.focus_is_carried
@models REQ-ACT.one_path
@models REQ-ACT.unknown_is_refused
@models REQ-ACT.missing_target_is_refused
@models REQ-ACT.edits_are_commands
@models REQ-ACT.dispatch_is_pure
-/
def dispatch (action : String) (focus : Focus) (w : Workspace) : Intent :=
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
    if focus.under == some "base" then Intent.travel Move.base
    else match focus.under.bind nodeNumber with
    | none => Intent.refuse (Blocked.needsTarget "history.jump" "a point in the history")
    | some node => Intent.travel (Move.to node)
  | "history.tree" => report "history"
  | "observe.start" => Intent.observe Watch.start
  | "observe.accept" => Intent.observe Watch.accept
  | "observe.reject" => Intent.observe Watch.reject
  | "observe.accept_file" =>
    match changedFile focus with
    | none => Intent.refuse (Blocked.needsTarget "observe.accept_file" "a changed file")
    | some path => Intent.observe (Watch.acceptFile path)
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
  | "trace.lock" => report "lock"
  | "trace.requirement" => requirementUnder focus
  | "trace.context" =>
    match focus.under with
    | none => Intent.refuse (Blocked.needsTarget "trace.context" "a requirement")
    | some id => report ("context " ++ id)
  | "context.toggle" =>
    match focus.under.bind TraceLean.Context.partNamed with
    | none => Intent.refuse (Blocked.needsTarget "context.toggle" "a part of the context")
    | some part => Intent.observe (Watch.contextToggle part)
  | "context.copy" => Intent.observe Watch.contextCopy
  | "history.filter" =>
    match focus.under.bind TraceLean.HistoryView.filterNamed with
    | none => Intent.refuse (Blocked.needsTarget "history.filter" "a view of the history")
    | some filter => Intent.observe (Watch.historyFilter filter)
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
  | "file.definition" => report "definition"
  | "file.references" => report "references"
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
  -- One weight a press. A pointer dragging a divider sends the amount it
  -- measured; the key means "a bit more", and a bit is one.
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

/-- Whether an intent is one the editor will act on. A refusal is an answer, not
an action.

@models REQ-ACT.unknown_is_refused -/
def acts : Intent → Bool
  | Intent.refuse _ => false
  | Intent.display _ => true
  | Intent.edit _ => true
  | Intent.travel _ => true
  | Intent.observe _ => true
  | Intent.arrange _ => true
  | Intent.persist => true

/-! ## What holds -/

private def cursorInFile : Focus :=
  { kind := BufferKind.file "a.rs", offset := 0, under := some "src/lib.rs" }

private def cursorInListing : Focus :=
  { kind := BufferKind.directory "src", offset := 0, under := some "src/lib.rs" }

private def cursorOverNothing : Focus :=
  { kind := BufferKind.directory "src", offset := 0, under := none }

private def noFiles : Workspace := {}

-- Intents are compared through their JSON encoding, because `Command` is
-- recursive and derives no `DecidableEq` -- the same reason `Mirror.Change`
-- gives. The encoding is what crosses the wire to a frontend anyway, so it is
-- the comparison that matters.
private def same (a b : Intent) : Bool :=
  Lean.toJson a == Lean.toJson b

/-- The same action in a listing and in a file opens the same thing, because
what it opens is what is under the cursor and nothing else.

@proves REQ-ACT.focus_is_carried -/
theorem opening_follows_the_cursor :
    same (dispatch "file.open" cursorInFile noFiles)
      (dispatch "file.open" cursorInListing noFiles) = true := by
  native_decide

/-- An action whose target is missing refuses, and says what was missing.

@proves REQ-ACT.missing_target_is_refused -/
theorem a_missing_target_is_named :
    same (dispatch "file.open" cursorOverNothing noFiles)
      (Intent.refuse (Blocked.needsTarget "file.open" "a path")) = true := by
  native_decide

/-- A name no keymap dispatches refuses rather than doing nothing.

@proves REQ-ACT.unknown_is_refused -/
theorem an_unknown_action_is_refused :
    acts (dispatch "file.explode" cursorInFile noFiles) = false := by
  native_decide

end TraceLean.Act
