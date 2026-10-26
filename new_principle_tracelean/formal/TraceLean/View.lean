import Lean
import TraceLean.Hash
import TraceLean.Evidence
import TraceLean.Annotation

namespace TraceLean.View

open TraceLean.Hash

open Lean (ToJson FromJson)

inductive BufferKind where
  | file (path : String)
  | directory (path : String)
  | review (target : String)
  | menu (title : String)
  | record (title : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive TokenKind where
  | keyword
  | string
  | escape
  | number
  | constant
  | comment
  | type
  | function
  | macroCall
  | operator
  | property
  | heading
  | bold
  | italic
  | link
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Role where
  | plain
  | path
  | entry
  | heading
  | requirement
  | mark (mark : TraceLean.Evidence.Mark) (held : Bool)
  | added
  | removed
  | token (kind : TokenKind)
  | claim (role : TraceLean.Annotation.Role)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Span where
  start : Nat
  stop : Nat
  role : Role
  actions : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def actionName (action : String) : String :=
  match action.splitOn " " with
  | [] => action
  | name :: _ => name

def actionTarget (action : String) : Option String :=
  match action.splitOn " " with
  | [] => none
  | [_] => none
  | _ :: rest => some (" ".intercalate rest)

structure Buffer where
  id : String
  kind : BufferKind
  text : String
  spans : List Span
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Fault where
  | pastTheEnd (start : Nat) (stop : Nat) (length : Nat)
  | backwards (start : Nat) (stop : Nat)
  | overlap (first : Nat) (second : Nat)
  | unordered (first : Nat) (second : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def faultsBetween : List Span → List Fault
  | [] => []
  | _ :: [] => []
  | a :: b :: rest =>
    let here :=
      if b.start < a.start then [Fault.unordered a.start b.start]
      else if b.start < a.stop then [Fault.overlap a.start b.start]
      else []
    here ++ faultsBetween (b :: rest)

/-- @models REQ-VIEW.text_is_the_content
@models REQ-VIEW.structure_over_text -/
def faults (buffer : Buffer) : List Fault :=
  let size := buffer.text.length
  let each := buffer.spans.foldl
    (fun acc span =>
      let bad :=
        if span.stop < span.start then [Fault.backwards span.start span.stop]
        else if size < span.stop then [Fault.pastTheEnd span.start span.stop size]
        else []
      acc ++ bad) []
  each ++ faultsBetween buffer.spans

/-- @models REQ-VIEW.affordances_named -/
def actionsAt (buffer : Buffer) (offset : Nat) : List String :=
  buffer.spans.foldl
    (fun acc span =>
      match decide (span.start ≤ offset && offset < span.stop) with
      | true => span.actions.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) acc
      | false => acc) []

def declaredActions (buffer : Buffer) : List String :=
  let all := buffer.spans.foldl (fun acc span => acc ++ span.actions) []
  (all.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) []).mergeSort
    (fun a b => decide (a ≤ b))

/-- @models REQ-VIEW.rendering_is_total -/
def plainText (buffer : Buffer) : List String :=
  buffer.text.splitOn "\n"

private def listingLine (entry : Nat × String) : String :=
  String.mk (List.replicate (2 * entry.1) ' ') ++ entry.2

private def listingSpans (entries : List (Nat × String)) (at_ : Nat) : List Span :=
  match entries with
  | [] => []
  | entry :: rest =>
    let line := listingLine entry
    let indent := 2 * entry.1
    let span : Span :=
      { start := at_ + indent, stop := at_ + line.length, role := Role.path,
        actions := ["file.open"] }
    span :: listingSpans rest (at_ + line.length + 1)

/-- @models REQ-VIEW.structure_has_one_type
@models REQ-SHOW.listing_from_entries
@models REQ-VIEW.everything_is_a_buffer -/
def directoryBuffer (path : String) (entries : List (Nat × String)) : Buffer :=
  { id := "dir:" ++ path,
    kind := BufferKind.directory path,
    text := String.intercalate "\n" (entries.map listingLine),
    spans := listingSpans entries 0 }

structure Delta where
  text : Option String
  spans : Option (List Span)
  kind : Option BufferKind
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-VIEW.changes_are_deltas -/
def delta (before after : Buffer) : Delta :=
  { text := if before.text == after.text then none else some after.text,
    spans := if before.spans == after.spans then none else some after.spans,
    kind := if before.kind == after.kind then none else some after.kind }

def applyDelta (buffer : Buffer) (d : Delta) : Buffer :=
  { id := buffer.id,
    kind := d.kind.getD buffer.kind,
    text := d.text.getD buffer.text,
    spans := d.spans.getD buffer.spans }

/-- @proves REQ-VIEW.changes_are_deltas -/
theorem a_delta_rebuilds_what_it_came_from (before after : Buffer)
    (sameId : before.id = after.id) :
    applyDelta before (delta before after) = after := by
  unfold applyDelta delta
  cases before
  cases after
  simp_all
  constructor
  · split
    · simp_all
    · rfl
  constructor
  · split
    · simp_all
    · rfl
  · split
    · simp_all
    · rfl

structure Rendering where
  lines : List String
  offered : List (Nat × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Breach where
  | lineDiffers (line : Nat) (shown : String) (expected : String)
  | lineMissing (line : Nat) (expected : String)
  | lineExtra (line : Nat) (shown : String)
  | actionInvented (offset : Nat) (action : String)
  | actionDropped (offset : Nat) (action : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def lineBreaches (at_ : Nat) : List String → List String → List Breach
  | [], [] => []
  | shown :: rest, [] => Breach.lineExtra at_ shown :: lineBreaches (at_ + 1) rest []
  | [], expected :: rest => Breach.lineMissing at_ expected :: lineBreaches (at_ + 1) [] rest
  | shown :: restShown, expected :: restExpected =>
    let here := if shown == expected then [] else [Breach.lineDiffers at_ shown expected]
    here ++ lineBreaches (at_ + 1) restShown restExpected

/-- @models REQ-VIEW.frontend_adds_nothing
@models REQ-VIEW.one_representation -/
def conformance (buffer : Buffer) (rendering : Rendering) : List Breach :=
  let lines := lineBreaches 0 rendering.lines (plainText buffer)
  let invented := rendering.offered.foldl
    (fun acc pair =>
      if (actionsAt buffer pair.1).contains pair.2 then acc
      else acc ++ [Breach.actionInvented pair.1 pair.2]) []
  let positions := rendering.offered.foldl
    (fun acc pair => if acc.contains pair.1 then acc else acc ++ [pair.1]) []
  let dropped := positions.foldl
    (fun acc offset =>
      (actionsAt buffer offset).foldl
        (fun inner action =>
          if rendering.offered.contains (offset, action) then inner
          else inner ++ [Breach.actionDropped offset action]) acc) []
  lines ++ invented ++ dropped

structure Presented where
  painted : String
  name : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-VIEW.presentation_may_be_symbolic -/
def accessible : List Presented → String
  | [] => ""
  | piece :: rest => piece.name ++ accessible rest

def symbolic (row : List Presented) : List Presented :=
  row.filter (fun piece => piece.painted != piece.name)

/-- @models REQ-VIEW.screen_is_readable -/
def presentedConformance (buffer : Buffer) (rows : List (List Presented))
    (offered : List (Nat × String)) : List Breach :=
  conformance buffer { lines := rows.map accessible, offered := offered }

private theorem accessible_ignores_painting (glyph : String) (row : List Presented) :
    accessible (row.map (fun piece => { piece with painted := glyph })) = accessible row := by
  induction row with
  | nil => rfl
  | cons piece rest ih => simp [accessible, ih]

private theorem lines_ignore_painting (glyph : String) (rows : List (List Presented)) :
    (rows.map (fun row => row.map (fun piece => { piece with painted := glyph }))).map accessible
      = rows.map accessible := by
  induction rows with
  | nil => rfl
  | cons row rest ih => simp [accessible_ignores_painting, ih]

/-- @proves REQ-VIEW.presentation_may_be_symbolic -/
theorem a_row_is_read_as_its_names_whatever_is_painted (row : List Presented) (glyph : String) :
    accessible (row.map (fun piece => { piece with painted := glyph })) = accessible row ∧
      ∀ piece rest, accessible (piece :: rest) = piece.name ++ accessible rest :=
  ⟨accessible_ignores_painting glyph row, fun _ _ => rfl⟩

theorem a_symbol_is_read_as_its_name (buffer : Buffer) (rows : List (List Presented))
    (offered : List (Nat × String)) (glyph : String) :
    presentedConformance buffer
        (rows.map (fun row => row.map (fun piece => { piece with painted := glyph }))) offered
      = presentedConformance buffer rows offered := by
  unfold presentedConformance
  rw [lines_ignore_painting]

def listing : Buffer := directoryBuffer "src" [(0, "main.rs"), (1, "deep.rs")]

private theorem no_breach_between_equal_lines (at_ : Nat) (lines : List String) :
    lineBreaches at_ lines lines = [] := by
  induction lines generalizing at_ with
  | nil => simp [lineBreaches]
  | cons line rest ih => simp [lineBreaches, ih]

/-- @proves REQ-VIEW.rendering_is_total -/
theorem every_kind_renders_as_its_text (buffer : Buffer) (kind : BufferKind) :
    plainText { buffer with kind := kind } = plainText buffer ∧ plainText buffer = buffer.text.splitOn "\n" :=
  ⟨rfl, rfl⟩

theorem drawing_the_text_always_conforms (buffer : Buffer) :
    conformance buffer { lines := plainText buffer, offered := [] } = [] := by
  simp [conformance, no_breach_between_equal_lines]

private theorem dedup_fold_mem (l acc : List String) (x : String) :
    x ∈ l.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) acc ↔ x ∈ acc ∨ x ∈ l := by
  induction l generalizing acc with
  | nil => simp
  | cons a rest ih =>
    rw [List.foldl_cons, ih]
    split
    · rename_i hc
      have ha : a ∈ acc := List.elem_iff.mp hc
      simp only [List.mem_cons]
      constructor
      · intro h; rcases h with h | h
        · exact Or.inl h
        · exact Or.inr (Or.inr h)
      · intro h; rcases h with h | h | h
        · exact Or.inl h
        · subst h; exact Or.inl ha
        · exact Or.inr h
    · simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · intro h; rcases h with (h | h) | h
        · exact Or.inl h
        · exact Or.inr (Or.inl h)
        · exact Or.inr (Or.inr h)
      · intro h; rcases h with h | h | h
        · exact Or.inl (Or.inl h)
        · exact Or.inl (Or.inr h)
        · exact Or.inr h

private theorem actionsAt_fold (spans : List Span) (offset : Nat) (acc : List String) (a : String) :
    a ∈ spans.foldl
      (fun acc span =>
        match decide (span.start ≤ offset && offset < span.stop) with
        | true => span.actions.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) acc
        | false => acc) acc ↔
      a ∈ acc ∨ ∃ s ∈ spans, s.start ≤ offset ∧ offset < s.stop ∧ a ∈ s.actions := by
  induction spans generalizing acc with
  | nil => simp
  | cons s rest ih =>
    rw [List.foldl_cons, ih]
    by_cases hc : s.start ≤ offset ∧ offset < s.stop
    · have : decide (s.start ≤ offset && offset < s.stop) = true := by simp [hc.1, hc.2]
      simp only [this, dedup_fold_mem, List.mem_cons]
      constructor
      · intro h; rcases h with (h | h) | ⟨t, ht, h⟩
        · exact Or.inl h
        · exact Or.inr ⟨s, Or.inl rfl, hc.1, hc.2, h⟩
        · exact Or.inr ⟨t, Or.inr ht, h⟩
      · intro h; rcases h with h | ⟨t, ht | ht, h⟩
        · exact Or.inl (Or.inl h)
        · subst ht; exact Or.inl (Or.inr h.2.2)
        · exact Or.inr ⟨t, ht, h⟩
    · have : decide (s.start ≤ offset && offset < s.stop) = false := by
        simp only [decide_eq_false_iff_not, Bool.and_eq_true, decide_eq_true_eq]; exact hc
      simp only [this, List.mem_cons]
      constructor
      · intro h; rcases h with h | ⟨t, ht, h⟩
        · exact Or.inl h
        · exact Or.inr ⟨t, Or.inr ht, h⟩
      · intro h; rcases h with h | ⟨t, ht | ht, h⟩
        · exact Or.inl h
        · subst ht; exact absurd ⟨h.1, h.2.1⟩ hc
        · exact Or.inr ⟨t, ht, h⟩

/-- @proves REQ-VIEW.affordances_named -/
theorem actions_at_are_the_named_actions_of_covering_spans (buffer : Buffer) (offset : Nat)
    (action : String) :
    action ∈ actionsAt buffer offset ↔
      ∃ s ∈ buffer.spans, s.start ≤ offset ∧ offset < s.stop ∧ action ∈ s.actions := by
  unfold actionsAt
  rw [actionsAt_fold]
  simp

private theorem foldl_append_eq (g : α → List β) (l : List α) (init : List β) :
    l.foldl (fun acc x => acc ++ g x) init = init ++ (l.map g).join := by
  induction l generalizing init with
  | nil => simp
  | cons x rest ih => rw [List.foldl_cons, ih]; simp

private theorem lineBreaches_nil (at_ : Nat) (shown expected : List String)
    (h : lineBreaches at_ shown expected = []) : shown = expected := by
  induction shown generalizing at_ expected with
  | nil =>
    cases expected with
    | nil => rfl
    | cons e rest => simp [lineBreaches] at h
  | cons x rest ih =>
    cases expected with
    | nil => simp [lineBreaches] at h
    | cons e erest =>
      simp only [lineBreaches, List.append_eq_nil] at h
      have hx : x = e := by
        have := h.1
        split at this
        · rename_i hb; simpa using hb
        · cases this
      rw [hx, ih _ _ h.2]

private theorem invented_eq (buffer : Buffer) (offered : List (Nat × String)) :
    offered.foldl
      (fun acc pair =>
        if (actionsAt buffer pair.1).contains pair.2 then acc
        else acc ++ [Breach.actionInvented pair.1 pair.2]) [] =
      (offered.map (fun pair => if (actionsAt buffer pair.1).contains pair.2 then []
        else [Breach.actionInvented pair.1 pair.2])).join := by
  have : (fun (acc : List Breach) (pair : Nat × String) =>
      if (actionsAt buffer pair.1).contains pair.2 then acc
      else acc ++ [Breach.actionInvented pair.1 pair.2]) =
      (fun acc pair => acc ++ (if (actionsAt buffer pair.1).contains pair.2 then []
        else [Breach.actionInvented pair.1 pair.2])) := by
    funext acc pair; split <;> simp
  rw [this, foldl_append_eq]
  simp

/-- @proves REQ-VIEW.frontend_adds_nothing -/
theorem an_invented_affordance_is_a_breach (buffer : Buffer) (rendering : Rendering)
    (offset : Nat) (action : String) (offered : (offset, action) ∈ rendering.offered)
    (notCarried : ¬ ∃ s ∈ buffer.spans, s.start ≤ offset ∧ offset < s.stop ∧ action ∈ s.actions) :
    Breach.actionInvented offset action ∈ conformance buffer rendering := by
  unfold conformance
  simp only [invented_eq, List.mem_append, List.mem_join, List.mem_map]
  left; right
  refine ⟨_, ⟨(offset, action), offered, rfl⟩, ?_⟩
  have hn : action ∉ actionsAt buffer offset := fun hm =>
    notCarried ((actions_at_are_the_named_actions_of_covering_spans buffer offset action).mp hm)
  simp [hn]

/-- @proves REQ-VIEW.one_representation -/
theorem a_conforming_frontend_draws_the_core_text (buffer : Buffer) (rendering : Rendering)
    (h : conformance buffer rendering = []) : rendering.lines = plainText buffer := by
  unfold conformance at h
  simp only [List.append_eq_nil] at h
  exact lineBreaches_nil 0 _ _ h.1.1

/-- @proves REQ-VIEW.screen_is_readable -/
theorem the_screen_is_checked_by_its_accessible_text (buffer : Buffer)
    (rows : List (List Presented)) (offered : List (Nat × String))
    (h : presentedConformance buffer rows offered = []) :
    rows.map accessible = plainText buffer :=
  a_conforming_frontend_draws_the_core_text buffer _ h

private theorem bad_nil (size : Nat) (span : Span) :
    (if span.stop < span.start then [Fault.backwards span.start span.stop]
      else if size < span.stop then [Fault.pastTheEnd span.start span.stop size] else []) = [] ↔
      span.start ≤ span.stop ∧ span.stop ≤ size := by
  split
  · simp; omega
  · split
    · simp; omega
    · simp; omega

private theorem between_nil_iff (l : List Span) (hw : ∀ s ∈ l, s.start ≤ s.stop) :
    faultsBetween l = [] ↔ l.Pairwise (fun a b => a.stop ≤ b.start) := by
  induction l with
  | nil => simp [faultsBetween]
  | cons a rest ih =>
    cases rest with
    | nil => simp [faultsBetween]
    | cons b rest' =>
      have ih' := ih (fun s hs => hw s (List.mem_cons_of_mem _ hs))
      have ha := hw a (List.mem_cons_self _ _)
      have hb := hw b (List.mem_cons_of_mem _ (List.mem_cons_self _ _))
      simp only [faultsBetween, List.append_eq_nil]
      rw [ih', List.pairwise_cons (a := a)]
      have here : (if b.start < a.start then [Fault.unordered a.start b.start]
          else if b.start < a.stop then [Fault.overlap a.start b.start] else []) = [] ↔
          a.stop ≤ b.start := by
        split
        · simp; omega
        · split
          · simp; omega
          · simp; omega
      rw [here]
      constructor
      · intro ⟨h1, h2⟩
        refine ⟨?_, h2⟩
        intro c hc
        rcases List.mem_cons.mp hc with hc | hc
        · subst hc; exact h1
        · have := (List.pairwise_cons.mp h2).1 c hc
          omega
      · intro ⟨h1, h2⟩
        exact ⟨h1 b (List.mem_cons_self _ _), h2⟩

/-- @proves REQ-VIEW.text_is_the_content
@proves REQ-VIEW.structure_over_text -/
theorem fault_free_spans_lie_in_order_over_the_text (buffer : Buffer) :
    faults buffer = [] ↔
      (∀ s ∈ buffer.spans, s.start ≤ s.stop ∧ s.stop ≤ buffer.text.length) ∧
        buffer.spans.Pairwise (fun a b => a.stop ≤ b.start) := by
  unfold faults
  dsimp only
  rw [foldl_append_eq (fun (span : Span) =>
    if span.stop < span.start then [Fault.backwards span.start span.stop]
    else if buffer.text.length < span.stop then
      [Fault.pastTheEnd span.start span.stop buffer.text.length] else [])]
  simp only [List.nil_append, List.append_eq_nil, List.join_eq_nil, List.mem_map]
  constructor
  · intro ⟨h1, h2⟩
    have hw : ∀ s ∈ buffer.spans, s.start ≤ s.stop ∧ s.stop ≤ buffer.text.length :=
      fun s hs => (bad_nil _ s).mp (h1 _ ⟨s, hs, rfl⟩)
    exact ⟨hw, (between_nil_iff _ (fun s hs => (hw s hs).1)).mp h2⟩
  · intro ⟨hw, hp⟩
    refine ⟨?_, (between_nil_iff _ (fun s hs => (hw s hs).1)).mpr hp⟩
    intro l ⟨s, hs, hl⟩
    rw [← hl]
    exact (bad_nil _ s).mpr (hw s hs)

private theorem intercalate_go_data (acc sep : String) (l : List String) :
    (String.intercalate.go acc sep l).data = acc.data ++ (l.map (fun x => sep.data ++ x.data)).join := by
  induction l generalizing acc with
  | nil => simp [String.intercalate.go]
  | cons x rest ih =>
    show (String.intercalate.go (acc ++ sep ++ x) sep rest).data = _
    rw [ih]
    simp [String.data_append]

private def listingText (entries : List (Nat × String)) : List Char :=
  (String.intercalate "\n" (entries.map listingLine)).data

private theorem listingText_one (e : Nat × String) : listingText [e] = (listingLine e).data := by
  simp [listingText, String.intercalate, intercalate_go_data]

private theorem listingText_cons (e f : Nat × String) (rest : List (Nat × String)) :
    listingText (e :: f :: rest) = (listingLine e).data ++ '\n' :: listingText (f :: rest) := by
  simp [listingText, String.intercalate, intercalate_go_data]

private theorem listingLine_data (e : Nat × String) :
    (listingLine e).data = List.replicate (2 * e.1) ' ' ++ e.2.data := by
  simp [listingLine, String.data_append]

private theorem listingLine_length (e : Nat × String) :
    (listingLine e).length = 2 * e.1 + e.2.length := by
  simp [listingLine, String.length_append]

private theorem listingText_length_pos (e : Nat × String) (rest : List (Nat × String)) :
    (listingLine e).length ≤ (listingText (e :: rest)).length := by
  cases rest with
  | nil => rw [listingText_one]; exact Nat.le_refl _
  | cons f r => rw [listingText_cons]; simp [String.length]

private theorem listingSpans_bounds (entries : List (Nat × String)) (at_ : Nat) :
    ∀ s ∈ listingSpans entries at_, at_ ≤ s.start ∧ s.start ≤ s.stop ∧
      s.stop ≤ at_ + (listingText entries).length ∧ s.role = Role.path ∧ s.actions = ["file.open"] := by
  induction entries generalizing at_ with
  | nil => simp [listingSpans]
  | cons e rest ih =>
    intro s hs
    simp only [listingSpans, List.mem_cons] at hs
    have hlen := listingLine_length e
    rcases hs with hs | hs
    · subst hs
      have := listingText_length_pos e rest
      simp only [String.length] at this
      refine ⟨by simp, by simp; omega, ?_, rfl, rfl⟩
      simp only [String.length] at hlen ⊢
      omega
    · cases rest with
      | nil => simp [listingSpans] at hs
      | cons f r =>
        obtain ⟨h1, h2, h3, h4, h5⟩ := ih _ s hs
        refine ⟨by omega, h2, ?_, h4, h5⟩
        rw [listingText_cons]
        simp only [List.length_append, List.length_cons]
        simp only [String.length] at h3 ⊢
        omega

private theorem listingSpans_ordered (entries : List (Nat × String)) (at_ : Nat) :
    (listingSpans entries at_).Pairwise (fun a b => a.stop ≤ b.start) := by
  induction entries generalizing at_ with
  | nil => simp [listingSpans]
  | cons e rest ih =>
    simp only [listingSpans]
    rw [List.pairwise_cons]
    refine ⟨?_, ih _⟩
    intro s hs
    have := (listingSpans_bounds rest _ s hs).1
    simp only at this ⊢
    omega

private theorem listingSpans_length (entries : List (Nat × String)) (at_ : Nat) :
    (listingSpans entries at_).length = entries.length := by
  induction entries generalizing at_ with
  | nil => rfl
  | cons e rest ih => simp [listingSpans, ih]

private theorem line_slice (e : Nat × String) (T R : List Char) (at_ : Nat)
    (h : T.drop at_ = (listingLine e).data ++ R) :
    (T.drop (at_ + 2 * e.1)).take (at_ + (listingLine e).length - (at_ + 2 * e.1)) = e.2.data ∧
      T.drop (at_ + (listingLine e).length) = R := by
  rw [listingLine_data] at h
  have hl := listingLine_length e
  simp only [String.length] at hl
  constructor
  · rw [show at_ + 2 * e.1 = 2 * e.1 + at_ by omega, ← List.drop_drop, h, List.append_assoc,
      List.drop_left' (List.length_replicate _ _)]
    simp only [String.length, hl]
    rw [show at_ + (2 * e.1 + e.2.data.length) - (2 * e.1 + at_) = e.2.data.length by omega]
    exact List.take_left _ _
  · simp only [String.length, hl]
    rw [show at_ + (2 * e.1 + e.2.data.length) = (2 * e.1 + e.2.data.length) + at_ by omega,
      ← List.drop_drop, h]
    apply List.drop_left'
    simp only [List.length_append, List.length_replicate]

private theorem listingSpans_mark (entries : List (Nat × String)) (at_ : Nat) (T : List Char)
    (h : T.drop at_ = listingText entries) :
    ∀ p ∈ (listingSpans entries at_).zip entries,
      (T.drop p.1.start).take (p.1.stop - p.1.start) = p.2.2.data ∧
        (T.drop p.1.stop = [] ∨ ∃ rest, T.drop p.1.stop = '\n' :: rest) := by
  induction entries generalizing at_ with
  | nil => simp [listingSpans]
  | cons e rest ih =>
    intro p hp
    simp only [listingSpans, List.zip_cons_cons, List.mem_cons] at hp
    cases rest with
    | nil =>
      rw [listingText_one, ← List.append_nil (listingLine e).data] at h
      have hs := line_slice e T [] at_ h
      rcases hp with hp | hp
      · subst hp
        exact ⟨hs.1, Or.inl hs.2⟩
      · simp [listingSpans] at hp
    | cons f r =>
      rw [listingText_cons] at h
      have hs := line_slice e T _ at_ h
      rcases hp with hp | hp
      · subst hp
        exact ⟨hs.1, Or.inr ⟨_, hs.2⟩⟩
      · apply ih _ _ p hp
        rw [show at_ + (listingLine e).length + 1 = 1 + (at_ + (listingLine e).length) by omega,
          ← List.drop_drop, hs.2]
        rfl

/-- @proves REQ-VIEW.everything_is_a_buffer -/
theorem a_listing_is_a_buffer_with_identity_kind_and_text (path path' : String)
    (entries entries' : List (Nat × String)) :
    (directoryBuffer path entries).kind = BufferKind.directory path ∧
    (directoryBuffer path entries).text.data = listingText entries ∧
    ((directoryBuffer path entries).id = (directoryBuffer path' entries').id → path = path') := by
  refine ⟨rfl, rfl, ?_⟩
  intro h
  have hd := congrArg String.data h
  simp only [directoryBuffer, String.data_append] at hd
  exact String.ext (List.append_cancel_left hd)

/-- @proves REQ-VIEW.structure_has_one_type -/
theorem synthetic_structure_passes_the_same_check (path : String) (entries : List (Nat × String)) :
    faults (directoryBuffer path entries) = [] := by
  rw [fault_free_spans_lie_in_order_over_the_text]
  refine ⟨?_, listingSpans_ordered entries 0⟩
  intro s hs
  obtain ⟨_, h2, h3, _, _⟩ := listingSpans_bounds entries 0 s hs
  refine ⟨h2, ?_⟩
  simpa [directoryBuffer, listingText, String.length] using h3

/-- @proves REQ-SHOW.listing_from_entries -/
theorem a_listing_marks_each_entry_on_its_own_line (path : String) (entries : List (Nat × String)) :
    (directoryBuffer path entries).spans.length = entries.length ∧
    ∀ p ∈ (directoryBuffer path entries).spans.zip entries,
      ((directoryBuffer path entries).text.data.drop p.1.start).take (p.1.stop - p.1.start)
          = p.2.2.data ∧
        ((directoryBuffer path entries).text.data.drop p.1.stop = [] ∨
          ∃ rest, (directoryBuffer path entries).text.data.drop p.1.stop = '\n' :: rest) ∧
        p.1.role = Role.path ∧ p.1.actions = ["file.open"] := by
  refine ⟨listingSpans_length entries 0, ?_⟩
  intro p hp
  have hm := listingSpans_mark entries 0 (directoryBuffer path entries).text.data rfl p hp
  have hb := listingSpans_bounds entries 0 p.1 (List.of_mem_zip hp).1
  exact ⟨hm.1, hm.2, hb.2.2.2.1, hb.2.2.2.2⟩

end TraceLean.View
