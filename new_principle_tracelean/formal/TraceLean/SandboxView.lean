import TraceLean.LinesProof

namespace TraceLean.SandboxView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce
open TraceLean.Transcript
open TraceLean.Layout
open TraceLean.LinesProof

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

/-- @models REQ-SHOW.sandbox_session_shown -/
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

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

def live : View :=
  ⟨some ⟨"s0001", "contained (bwrap)", "cd /tmp/x"⟩, [⟨"modified", "a.rs", 2, 1⟩],
   [⟨"you", "first"⟩, ⟨"agent", "second"⟩], ⟨0, []⟩, 40⟩


theorem costLines_grows (out : Lines) (estimate : String) : LGrows out (costLines out estimate) :=
  lgrows_trans (line_grows _ _) (wrapped_grows _ _ _ _ _)

theorem changeLine_grows (out : Lines) (change : Pending) : LGrows out (changeLine out change) :=
  line_grows _ _

theorem good_costLines {o : Lines} (estimate : String) (h : Good o) : Good (costLines o estimate) :=
  (costLines_grows o estimate).2.2.2 h

theorem good_changeLine {o : Lines} (change : Pending) (h : Good o) : Good (changeLine o change) :=
  (changeLine_grows o change).2.2.2 h

macro_rules | `(tactic| grow_step) => `(tactic| first
  | with_reducible apply good_costLines
  | with_reducible apply good_changeLine)

macro_rules | `(tactic| say_step) => `(tactic| first
  | with_reducible refine says_grows ?_ (costLines_grows _ _)
  | with_reducible exact changeLine_grows _ _)

theorem change_says_path (o : Lines) (c : Pending) : Says (changeLine o c) Role.path ["observe.diff"] c.path :=
  line_says _ _ _ _ _ (by simp) rfl

macro "sayc" c:term "," hc:term : tactic => `(tactic| repeat (first
  | with_reducible exact foldl_says changeLine (fun o x => changeLine_grows o x) $c (fun o => change_says_path o $c) _ _ $hc
  | with_reducible refine says_grows ?_ (line_grows _ _)
  | with_reducible refine says_blank ?_
  | with_reducible refine says_grows ?_ (wrapped_grows _ _ _ _ _)
  | with_reducible refine says_grows ?_ (costLines_grows _ _)
  | with_reducible refine says_grows ?_ (foldl_lgrows _ ?_ _ _)
  | intro _ _
  | with_reducible exact wrapped_grows _ _ _ _ _
  | split))

set_option maxRecDepth 100000 in
set_option maxHeartbeats 4000000 in
/-- @proves REQ-SHOW.sandbox_session_shown -/
theorem a_sandbox_station_shows_its_session (view : View) :
    faults (sandboxView view) = [] ∧
    (view.session = none → Offers (sandboxView view) Role.entry ["sandbox.new"] "[ New sandbox ]") ∧
    (∀ shown, view.session = some shown →
      (shown.id ≠ "" → Offers (sandboxView view) Role.heading [] shown.id) ∧
      Offers (sandboxView view) Role.entry ["sandbox.copy"] "[ Copy command ]" ∧
      Offers (sandboxView view) Role.entry ["sandbox.end"] "[ End sandbox ]" ∧
      (∀ c ∈ view.pending, c.path ≠ "" → Offers (sandboxView view) Role.path ["observe.diff"] c.path) ∧
      (view.pending ≠ [] → Offers (sandboxView view) Role.added ["observe.accept"] "[ Accept all ]" ∧
        Offers (sandboxView view) Role.removed ["observe.reject"] "[ Reject all ]") ∧
      Offers (sandboxView view) Role.heading [] "Conversation, newest first") := by
  have hl := (a_sandbox_observation_in_order [] view.said view.spend).2.2.2.2
  refine ⟨?_, ?_, ?_⟩
  · unfold sandboxView
    split
    · exact finish_faults _ _
    · exact finish_faults _ _
  · intro h
    unfold sandboxView noSession
    rw [h]
    simp only [hl]
    refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
    · show Good _
      grow
    · say
  · intro shown h
    unfold sandboxView
    rw [h]
    simp only [hl]
    refine ⟨fun hne => ?_, ?_, ?_, fun c hc hne => ?_, fun hne => ⟨?_, ?_⟩, ?_⟩
    · refine finish_offers _ _ ?_ ?_ hne
      · show Good _
        grow
      · say
    · refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
      · show Good _
        grow
      · say
    · refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
      · show Good _
        grow
      · say
    · refine finish_offers _ _ ?_ ?_ hne
      · show Good _
        grow
      · sayc c, hc
    · have he : view.pending.isEmpty = false := by cases hp : view.pending <;> simp_all
      simp only [he, Bool.false_eq_true, if_false, ite_false]
      refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
      · show Good _
        grow
      · say
    · have he : view.pending.isEmpty = false := by cases hp : view.pending <;> simp_all
      simp only [he, Bool.false_eq_true, if_false, ite_false]
      refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
      · show Good _
        grow
      · say
    · refine finish_offers _ _ ?_ ?_ (lit_ne _ rfl)
      · show Good _
        grow
      · say

end TraceLean.SandboxView
