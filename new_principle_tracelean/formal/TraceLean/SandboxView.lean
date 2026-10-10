import TraceLean.Layout

/-!
# The sandbox station

Models `REQ-SHOW.sandbox_session_shown`: whether a session exists, the command
that enters it, each change waiting with a way to review it, ways to accept and
reject them, and the agent's conversation newest first, wrapped to the width.
-/

namespace TraceLean.SandboxView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce
open TraceLean.Transcript
open TraceLean.Layout

structure Pending where
  kind : String
  path : String
  added : Nat
  removed : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Shown where
  id : String
  containment : String
  command : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure View where
  session : Option Shown
  pending : List Pending
  said : List Event
  spend : TraceLean.Cost.Spend
  width : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The role a row of the conversation is drawn in. -/
def roleOf (kind : String) : Role :=
  match kind with
  | "you" => Role.requirement
  | "tool" => Role.path
  | "failed" => Role.removed
  | "cost" => Role.heading
  | "thinking" => Role.entry
  | "result" => Role.entry
  | _ => Role.plain

def costLines (out : Lines) (estimate : String) : Lines :=
  wrapped (line out [("Agent cost (price table's unit)", roleOf "cost", [])]) "  " estimate Role.plain []

def changeLine (out : Lines) (change : Pending) : Lines :=
  let role :=
    match change.kind with
    | "created" => Role.added
    | "deleted" => Role.removed
    | _ => Role.requirement
  line out [("  ", Role.plain, []), ("[✓]", Role.added, ["observe.accept_file"]), (" ", Role.plain, []),
            ("[✗]", Role.removed, ["observe.reject_file"]), (" " ++ padRight change.kind 9, role, []),
            (change.path, Role.path, ["observe.diff"]), (" +" ++ toString change.added, Role.added, []),
            (" −" ++ toString change.removed, Role.removed, [])]

def noSession (view : View) : Buffer :=
  let out := start view.width
  let out := line out [("Sandbox", Role.heading, [])]
  let out := wrapped (blank out) ""
    "A sandbox is a copy of this project. You run your agent in it, from your own terminal; what it changes appears here for you to accept or reject, and what it says appears below."
    Role.plain []
  let out := line (blank out) [("[ New sandbox ]", Role.entry, ["sandbox.new"])]
  let out :=
    match (sandboxEvents [] view.said view.spend).getLast? with
    | some cost => costLines (blank out) cost.text
    | none => out
  finish out "sandbox"

/-- The sandbox panel.

@models REQ-SHOW.sandbox_session_shown -/
def sandboxView (view : View) : Buffer :=
  match view.session with
  | none => noSession view
  | some session =>
    let out := start view.width
    let out := line out [("Sandbox ", Role.heading, []), (session.id, Role.heading, [])]
    let out := blank (wrapped out "" session.containment Role.entry [])
    let out := wrapped out "" "Run this in your own terminal, then start your agent (for example `claude`):" Role.plain []
    let out := blank (wrapped out "  " session.command Role.path ["sandbox.copy"])
    let out := line out [("[ Copy command ]", Role.entry, ["sandbox.copy"]), (" ", Role.plain, []),
                         ("[ Look for changes ]", Role.entry, ["observe.start"])]
    let out := blank (line out [("[ End sandbox ]", Role.entry, ["sandbox.end"])])
    let out := line out [("Changes waiting (", Role.heading, []), (toString view.pending.length, Role.heading, []),
                         (")", Role.heading, [])]
    let out := if view.pending.isEmpty then line out [("  none yet", Role.plain, [])] else out
    let out := view.pending.foldl changeLine out
    let out :=
      if view.pending.isEmpty then out
      else line out [("[ Accept all ]", Role.added, ["observe.accept"]), (" ", Role.plain, []),
                     ("[ Reject all ]", Role.removed, ["observe.reject"])]
    let out := line (blank out) [("Conversation, newest first", Role.heading, [])]
    let rows := sandboxEvents [] view.said view.spend
    let out :=
      if rows.length ≤ 1 then wrapped out "  " "Nothing yet. It appears here as the agent works." Role.plain []
      else out
    let out := rows.dropLast.reverse.foldl
      (fun (o : Lines) (event : Event) => wrapped o "" (event.kind ++ ": " ++ event.text) (roleOf event.kind) []) out
    let out :=
      match rows.getLast? with
      | some cost => costLines (blank out) cost.text
      | none => out
    finish out "sandbox"

end TraceLean.SandboxView
