import TraceLean.Layout

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


def lineStep (acc : String × List Span) (p : String × Role × List String) : String × List Span :=
  (acc.1 ++ p.1,
   if (p.2.1 != Role.plain || !p.2.2.isEmpty) = true then
     acc.2 ++ [{ start := acc.1.length, stop := acc.1.length + p.1.length, role := p.2.1, actions := p.2.2 }]
   else acc.2)

theorem line_eq (out : Lines) (ps : List (String × Role × List String)) :
    line out ps =
      { text := (ps.foldl lineStep (if out.text.isEmpty then out.text else out.text ++ "\n", out.spans)).1,
        spans := (ps.foldl lineStep (if out.text.isEmpty then out.text else out.text ++ "\n", out.spans)).2,
        width := out.width } := rfl

theorem fold_step : ∀ (ps : List (String × Role × List String)) (acc : String × List Span),
    (∃ m, (ps.foldl lineStep acc).1 = acc.1 ++ m) ∧ (∃ e, (ps.foldl lineStep acc).2 = acc.2 ++ e) ∧
    (Fits 0 acc.2 acc.1.length → Fits 0 (ps.foldl lineStep acc).2 (ps.foldl lineStep acc).1.length) ∧
    (∀ p ∈ ps, (p.2.1 != Role.plain || !p.2.2.isEmpty) = true →
      ∃ s ∈ (ps.foldl lineStep acc).2, s.role = p.2.1 ∧ s.actions = p.2.2 ∧
        Shows (ps.foldl lineStep acc).1 s p.1) := by
  intro ps
  induction ps with
  | nil =>
    intro acc
    exact ⟨⟨"", (String.append_empty _).symm⟩, ⟨[], (List.append_nil _).symm⟩, id, fun p h => by cases h⟩
  | cons q rest ih =>
    intro acc
    obtain ⟨⟨m, hm⟩, ⟨e, he⟩, hf, hs⟩ := ih (lineStep acc q)
    simp only [List.foldl] at hm he hf hs ⊢
    refine ⟨⟨q.1 ++ m, ?_⟩, ?_, ?_, ?_⟩
    · rw [hm]; simp only [lineStep, String.append_assoc]
    · obtain ⟨X, hX⟩ : ∃ X, (lineStep acc q).2 = acc.2 ++ X := by
        unfold lineStep
        split
        · exact ⟨_, rfl⟩
        · exact ⟨[], (List.append_nil _).symm⟩
      exact ⟨X ++ e, by rw [he, hX, List.append_assoc]⟩
    · intro f
      apply hf
      unfold lineStep
      simp only [String.length_append]
      split
      · exact fits_append f ⟨Nat.le_refl _, Nat.le_add_right _ _, Nat.le_refl _⟩
      · exact fits_upper f (Nat.le_add_right _ _)
    · intro p hp hc
      rcases List.mem_cons.mp hp with rfl | hp
      · refine ⟨{ start := acc.1.length, stop := acc.1.length + p.1.length, role := p.2.1, actions := p.2.2 },
          ?_, rfl, rfl, ?_⟩
        · rw [he]
          apply List.mem_append_left
          unfold lineStep
          rw [if_pos hc]
          exact List.mem_append_right _ (List.mem_singleton_self _)
        · rw [hm]
          exact ⟨acc.1, m, by simp only [lineStep], rfl, rfl⟩
      · exact hs p hp hc

def LGrows (o o' : Lines) : Prop :=
  (∃ m, o'.text = o.text ++ m) ∧ (∃ e, o'.spans = o.spans ++ e) ∧ o'.width = o.width ∧
  (Fits 0 o.spans o.text.length → Fits 0 o'.spans o'.text.length)

def Says (o : Lines) (r : Role) (actions : List String) (said : String) : Prop :=
  ∃ s ∈ o.spans, s.role = r ∧ s.actions = actions ∧ Shows o.text s said

theorem lgrows_refl (o : Lines) : LGrows o o :=
  ⟨⟨"", (String.append_empty _).symm⟩, ⟨[], (List.append_nil _).symm⟩, rfl, id⟩

theorem lgrows_trans {a b c : Lines} (h1 : LGrows a b) (h2 : LGrows b c) : LGrows a c := by
  obtain ⟨⟨m1, e1⟩, ⟨s1, f1⟩, w1, g1⟩ := h1
  obtain ⟨⟨m2, e2⟩, ⟨s2, f2⟩, w2, g2⟩ := h2
  exact ⟨⟨m1 ++ m2, by rw [e2, e1, String.append_assoc]⟩, ⟨s1 ++ s2, by rw [f2, f1, List.append_assoc]⟩,
    w2.trans w1, fun h => g2 (g1 h)⟩

theorem says_grows {o o' : Lines} {r : Role} {a : List String} {t : String} (h : Says o r a t)
    (g : LGrows o o') : Says o' r a t := by
  obtain ⟨s, hs, e1, e2, e3⟩ := h
  obtain ⟨⟨m, hm⟩, ⟨e, he⟩, _, _⟩ := g
  exact ⟨s, by rw [he]; exact List.mem_append_left _ hs, e1, e2, by rw [hm]; exact shows_extend m e3⟩

theorem line_grows (out : Lines) (ps : List (String × Role × List String)) : LGrows out (line out ps) := by
  rw [line_eq]
  obtain ⟨⟨m, hm⟩, ⟨e, he⟩, hf, _⟩ :=
    fold_step ps (if out.text.isEmpty then out.text else out.text ++ "\n", out.spans)
  refine ⟨?_, ⟨e, he⟩, rfl, ?_⟩
  · simp only [] at hm ⊢
    rw [hm]
    split
    · exact ⟨m, rfl⟩
    · exact ⟨"\n" ++ m, by rw [String.append_assoc]⟩
  · intro f
    apply hf
    simp only []
    split
    · exact f
    · exact fits_upper f (by rw [String.length_append]; exact Nat.le_add_right _ _)

theorem line_says (out : Lines) (ps : List (String × Role × List String)) (t : String) (r : Role)
    (a : List String) (hp : (t, r, a) ∈ ps) (hc : (r != Role.plain || !a.isEmpty) = true) :
    Says (line out ps) r a t := by
  rw [line_eq]
  exact (fold_step ps _).2.2.2 (t, r, a) hp hc

theorem foldl_lgrows {α : Type} (f : Lines → α → Lines) (hf : ∀ o x, LGrows o (f o x)) :
    ∀ (l : List α) (o : Lines), LGrows o (l.foldl f o) := by
  intro l
  induction l with
  | nil => intro o; exact lgrows_refl o
  | cons x rest ih => intro o; exact lgrows_trans (hf o x) (ih (f o x))

theorem foldl_says {α : Type} (f : Lines → α → Lines) (hf : ∀ o x, LGrows o (f o x)) {r : Role}
    {a : List String} {t : String} (x : α) (hx : ∀ o, Says (f o x) r a t) :
    ∀ (l : List α) (o : Lines), x ∈ l → Says (l.foldl f o) r a t := by
  intro l
  induction l with
  | nil => intro _ h; cases h
  | cons y rest ih =>
    intro o h
    rcases List.mem_cons.mp h with rfl | h
    · exact says_grows (hx o) (foldl_lgrows f hf rest _)
    · exact ih _ h

theorem blank_grows (out : Lines) : LGrows out (blank out) := line_grows out []

theorem wrapped_grows (out : Lines) (indent text : String) (role : Role) (actions : List String) :
    LGrows out (wrapped out indent text role actions) := by
  unfold wrapped
  split
  · exact line_grows _ _
  · exact foldl_lgrows _ (fun o row => line_grows o _) _ out

theorem finish_offers (o : Lines) (title : String) {r : Role} {a : List String} {t : String}
    (f : Fits 0 o.spans o.text.length) (h : Says o r a t) (hne : t ≠ "") : Offers (finish o title) r a t := by
  obtain ⟨s, hs, e1, e2, e3⟩ := h
  refine ⟨s, ?_, e1, e2, textUnder_of_shows e3⟩
  show s ∈ tidy o.text.length o.spans
  rw [tidy_of_fits f, List.mem_filter]
  obtain ⟨_, _, _, _, h4⟩ := e3
  have := nonempty_length hne
  exact ⟨hs, by simp only [decide_eq_true_eq]; omega⟩

theorem start_fits (width : Nat) : Fits 0 (start width).spans (start width).text.length := Nat.le_refl 0

theorem finish_faults (o : Lines) (title : String) : faults (finish o title) = [] :=
  tidy_leaves_no_fault _ _ _ _

theorem lgrows_ite {c : Prop} [Decidable c] {o a b : Lines} (ha : LGrows o a) (hb : LGrows o b) :
    LGrows o (if c then a else b) := by
  split
  · exact ha
  · exact hb

theorem costLines_grows (out : Lines) (estimate : String) : LGrows out (costLines out estimate) :=
  lgrows_trans (line_grows _ _) (wrapped_grows _ _ _ _ _)

theorem changeLine_grows (out : Lines) (change : Pending) : LGrows out (changeLine out change) :=
  line_grows _ _

def Good (o : Lines) : Prop := Fits 0 o.spans o.text.length

theorem good_start (w : Nat) : Good (start w) := start_fits w

theorem good_line {o : Lines} (ps : List (String × Role × List String)) (h : Good o) : Good (line o ps) :=
  (line_grows o ps).2.2.2 h

theorem good_blank {o : Lines} (h : Good o) : Good (blank o) := (blank_grows o).2.2.2 h

theorem good_wrapped {o : Lines} (indent text : String) (role : Role) (actions : List String) (h : Good o) :
    Good (wrapped o indent text role actions) := (wrapped_grows o indent text role actions).2.2.2 h

theorem good_costLines {o : Lines} (estimate : String) (h : Good o) : Good (costLines o estimate) :=
  (costLines_grows o estimate).2.2.2 h

theorem good_foldl {α : Type} (f : Lines → α → Lines) (hf : ∀ o x, Good o → Good (f o x)) :
    ∀ (l : List α) (o : Lines), Good o → Good (l.foldl f o) := by
  intro l
  induction l with
  | nil => intro o h; exact h
  | cons x rest ih => intro o h; exact ih _ (hf o x h)

theorem good_changeLine {o : Lines} (change : Pending) (h : Good o) : Good (changeLine o change) :=
  (changeLine_grows o change).2.2.2 h

theorem says_blank {o : Lines} {r : Role} {a : List String} {t : String} (h : Says o r a t) :
    Says (blank o) r a t := says_grows h (blank_grows o)

macro "grow" : tactic => `(tactic| repeat (first
  | with_reducible apply good_line
  | with_reducible apply good_blank
  | with_reducible apply good_wrapped
  | with_reducible apply good_costLines
  | with_reducible apply good_changeLine
  | with_reducible apply good_foldl
  | intro _ _ _
  | assumption
  | with_reducible exact good_start _
  | split))

macro "say" : tactic => `(tactic| repeat (first
  | ((with_reducible refine line_says _ _ _ _ _ ?_ ?_) <;> (simp; done))
  | with_reducible refine says_grows ?_ (line_grows _ _)
  | with_reducible refine says_blank ?_
  | with_reducible refine says_grows ?_ (wrapped_grows _ _ _ _ _)
  | with_reducible refine says_grows ?_ (costLines_grows _ _)
  | with_reducible refine says_grows ?_ (foldl_lgrows _ ?_ _ _)
  | intro _ _
  | with_reducible exact changeLine_grows _ _
  | with_reducible exact wrapped_grows _ _ _ _ _
  | split))

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
