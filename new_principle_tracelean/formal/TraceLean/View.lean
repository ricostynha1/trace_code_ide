import Lean
import TraceLean.Hash
import TraceLean.Evidence
import TraceLean.Annotation

/-!
# The representation every frontend renders

Models `REQ-VIEW`. One value says what is on screen: a buffer is an identity, a
kind and text, and structure is spans over that text saying what a region *is*
and what can be done there. A file is a buffer; so is a directory listing, a
diff under review and a menu.

The point is not tidiness. Two frontends that each compute their own view give
two answers to "what is on screen", and the unmaintained answer rots without
anything failing -- which is what happened to the TUI this port came from. One
value has one answer, and a value can be modelled, tested and graded; pixels
cannot.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.View

open TraceLean.Hash

open Lean (ToJson FromJson)

/-- What a buffer holds. Every shown thing is one of these. -/
inductive BufferKind where
  /-- The contents of a file on disk. -/
  | file (path : String)
  /-- A directory, listed. -/
  | directory (path : String)
  /-- A change an external agent made, waiting for a person. -/
  | review (target : String)
  /-- A list of choices: the which-key bar, a command list. -/
  | menu (title : String)
  /-- Anything recorded rather than chosen: a transcript, a log. -/
  | record (title : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What a token of source code is. -/
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
  /-- Markdown prose: a heading line, bold, italic, a link. -/
  | heading
  | bold
  | italic
  | link
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What a region of text *is*. Never how it looks: a role a frontend does not
recognise is still rendered as text, and one it does recognise it may render as
richly as its medium allows. -/
inductive Role where
  | plain
  /-- A path, in a listing or in a message. -/
  | path
  /-- One item of a listing or menu. -/
  | entry
  /-- A heading: a section, a file name at the top of a diff. -/
  | heading
  /-- A requirement or clause identifier. -/
  | requirement
  /-- An evidence level, and which one.

  The grade is carried rather than left in the text. A frontend that had to
  read `L3` out of the characters to colour it would be parsing the buffer,
  which is the one thing a frontend may not do; and one that could not tell L1
  from L4 would have to paint every level the same, which is the whole of what
  a colour is for here. -/
  | level (grade : TraceLean.Evidence.Level)
  /-- A line a change added. -/
  | added
  /-- A line a change removed. -/
  | removed
  /-- A token of source code, and what kind: what syntax highlighting colours.
  The kind comes from parsing, in the core, for the reason a level carries its
  grade. -/
  | token (kind : TokenKind)
  /-- What a claim on a requirement is -- implements, tests, models, proves --
  so each is coloured as its gutter chip is. -/
  | claim (role : TraceLean.Annotation.Role)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A region of a buffer's text, what it is, and what can be done there.

`actions` names what is available, using the names the keymap dispatches. A
frontend reads them; it does not invent them. -/
structure Span where
  start : Nat
  stop : Nat
  role : Role
  actions : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- An action as a span carries it: a name the keymap dispatches, optionally
followed by one space and the target it is about (`screen.show file:a.rs`) --
for a span whose text does not spell what it is about, as a tab's does not. -/
def actionName (action : String) : String :=
  match action.splitOn " " with
  | [] => action
  | name :: _ => name

/-- The target an action carries after its name, if any. See `actionName`. -/
def actionTarget (action : String) : Option String :=
  match action.splitOn " " with
  | [] => none
  | [_] => none
  | _ :: rest => some (" ".intercalate rest)

/-- What is on screen, or could be.

@models REQ-VIEW.one_representation
@models REQ-VIEW.everything_is_a_buffer -/
structure Buffer where
  id : String
  kind : BufferKind
  text : String
  spans : List Span
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-! ## Well-formedness

A span that reaches past the text, or runs backwards, or overlaps its
neighbour, describes something the text does not contain. Reported rather than
clamped: a frontend cannot render what is not there, and silently moving the
span would hide which producer was wrong.
-/

/-- What is wrong with a buffer's structure. -/
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

/--
Everything wrong with a buffer's spans, in the order the spans are given.

@models REQ-VIEW.text_is_the_content
@models REQ-VIEW.structure_over_text
-/
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

/-! ## Reading the buffer -/

/--
What can be done at a position: the actions of every span covering it, in span
order, without repeats.

An action a frontend offers and this does not return is an affordance nobody
declared, which is the thing that cannot be checked.

@models REQ-VIEW.affordances_named
-/
def actionsAt (buffer : Buffer) (offset : Nat) : List String :=
  buffer.spans.foldl
    (fun acc span =>
      match decide (span.start ≤ offset && offset < span.stop) with
      | true => span.actions.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) acc
      | false => acc) []

/-- Every action any span of a buffer names, sorted and without repeats. -/
def declaredActions (buffer : Buffer) : List String :=
  let all := buffer.spans.foldl (fun acc span => acc ++ span.actions) []
  (all.foldl (fun seen a => if seen.contains a then seen else seen ++ [a]) []).mergeSort
    (fun a b => decide (a ≤ b))

/--
The rendering every frontend has to agree with: the buffer's text, as lines.

A frontend with a richer medium draws more than this. What it may not do is
show something else: the text is the content, and this is that text.
-/
def plainText (buffer : Buffer) : List String :=
  buffer.text.splitOn "\n"

/-! ## Synthetic buffers

A directory listing has no grammar and needs none: the core produces its spans
directly, and they are the same spans a parser would have produced for a file.
One type, two producers.
-/

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

/--
A directory listing as a buffer: indentation, then the name, one entry a line.

@models REQ-VIEW.structure_has_one_type
@models REQ-SHOW.listing_from_entries
-/
def directoryBuffer (path : String) (entries : List (Nat × String)) : Buffer :=
  { id := "dir:" ++ path,
    kind := BufferKind.directory path,
    text := String.intercalate "\n" (entries.map listingLine),
    spans := listingSpans entries 0 }

/-- A listing describes its own text: nothing it marks reaches past what it holds.

@proves REQ-VIEW.text_is_the_content -/
theorem a_listing_is_well_formed :
    faults (directoryBuffer "src" [(0, "main.rs"), (1, "deep.rs"), (0, "lib.rs")]) = [] := by
  native_decide

/-! ## Changes

What is on screen changes constantly, and a frontend that re-reads everything on
every keystroke is a frontend that stutters. A change is therefore a delta, and
the delta is enough: applying it gives exactly the buffer it came from.
-/

/-- A change to what is shown. -/
structure Delta where
  /-- The new text, when it changed. -/
  text : Option String
  /-- The new spans, when they changed. -/
  spans : Option (List Span)
  /-- The new kind, when it changed. -/
  kind : Option BufferKind
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What changed between two states of one buffer.

@models REQ-VIEW.changes_are_deltas -/
def delta (before after : Buffer) : Delta :=
  { text := if before.text == after.text then none else some after.text,
    spans := if before.spans == after.spans then none else some after.spans,
    kind := if before.kind == after.kind then none else some after.kind }

/-- Apply a change. -/
def applyDelta (buffer : Buffer) (d : Delta) : Buffer :=
  { id := buffer.id,
    kind := d.kind.getD buffer.kind,
    text := d.text.getD buffer.text,
    spans := d.spans.getD buffer.spans }

/-- A delta carries everything that changed: applying it to the state it was
taken from gives back the state it was taken to. A frontend that keeps up with
deltas is showing what a frontend that re-read everything would show.

@proves REQ-VIEW.changes_are_deltas -/
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

/-! ## Checking a frontend

A frontend is a process that, given a buffer, says what it drew: the lines it
put on screen and, for each position, the actions it offered there. That answer
is a value, so it crosses the same line protocol every other side of this system
crosses -- and the same check runs against a terminal, a web view or anything
else somebody writes.

What is checked is not how it looks. It is that the text is the buffer's text,
and that the actions offered are exactly the actions declared: a frontend that
invents one is offering something nobody can trace, and one that drops the last
action on a line has quietly removed a feature.
-/

/-- What a frontend says it drew. -/
structure Rendering where
  lines : List String
  /-- Every action offered, with the position it was offered at. -/
  offered : List (Nat × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A way a rendering fails to be the buffer it came from. -/
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

/--
Everything a frontend got wrong about a buffer.

@models REQ-VIEW.rendering_is_total
@models REQ-VIEW.frontend_adds_nothing
-/
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

/-- A frontend that draws the buffer's own text and offers exactly what the
buffer declares is conformant, whatever else it does.

@proves REQ-VIEW.frontend_adds_nothing -/
theorem drawing_the_buffer_is_conformant :
    conformance
      (directoryBuffer "src" [(0, "main.rs"), (0, "lib.rs")])
      { lines := ["main.rs", "lib.rs"],
        offered := [(0, "file.open"), (8, "file.open")] } = [] := by
  native_decide

/-! ## Presenting a region as a symbol

A window has glyphs a terminal does not, and a bar of four words is a bar
nobody recognises at a glance. Read literally, `text_is_the_content` forbade an
icon: an icon is not the buffer's text.

So the line moves from *painted* to *named*. A frontend may paint whatever its
medium affords; what is read off a screen is the accessible name each painted
region carries, and that name must be the region's own text. The freedom is the
glyph, the obligation is the name.
-/

/-- A region as a frontend put it on the screen: the glyphs it painted, and the
name those glyphs carry -- what a screen reader announces and what the capture
harness reads. -/
structure Presented where
  painted : String
  name : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What a row of presented regions reads as: the names, in order.

The glyphs are not consulted. That is the whole content of the clause.

@models REQ-VIEW.presentation_may_be_symbolic -/
def accessible : List Presented → String
  | [] => ""
  | piece :: rest => piece.name ++ accessible rest

/-- The regions a frontend chose to present as something other than their name.

A frontend reports these so that a run can say it exercised the symbolic case
rather than assuming it did. -/
def symbolic (row : List Presented) : List Presented :=
  row.filter (fun piece => piece.painted != piece.name)

/-- A frontend that presented symbolically, checked against the buffer it was
given.

Its rows are read by their names and then compared exactly as any other
rendering is: there is no second notion of conformance, only a second way of
arriving at the lines.

@models REQ-VIEW.screen_is_readable -/
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

/-- Repainting every region with the same glyph changes nothing about whether a
frontend conformed.

Which is the clause, stated so that it cannot be satisfied by accident: what a
frontend paints is outside the check, and the names are the whole of it.

@proves REQ-VIEW.presentation_may_be_symbolic -/
theorem a_symbol_is_read_as_its_name (buffer : Buffer) (rows : List (List Presented))
    (offered : List (Nat × String)) (glyph : String) :
    presentedConformance buffer
        (rows.map (fun row => row.map (fun piece => { piece with painted := glyph }))) offered
      = presentedConformance buffer rows offered := by
  unfold presentedConformance
  rw [lines_ignore_painting]

/-! ## What holds of buffers -/

/-- A listing of two entries, one nested. -/
def listing : Buffer := directoryBuffer "src" [(0, "main.rs"), (1, "deep.rs")]

/-- A directory becomes one entry a line, each entry marked as a path that
opens it.

@proves REQ-SHOW.listing_from_entries -/
theorem a_listing_is_one_entry_a_line :
    listing.text = "main.rs\n  deep.rs" ∧
    listing.spans = [⟨0, 7, .path, ["file.open"]⟩, ⟨10, 17, .path, ["file.open"]⟩] := by
  native_decide

/-- A listing nothing parsed is a buffer of the one type, with an identity, a
kind and text.

@proves REQ-VIEW.everything_is_a_buffer -/
theorem a_listing_is_a_buffer :
    listing.id = "dir:src" ∧ listing.kind = .directory "src" ∧ listing.text = "main.rs\n  deep.rs" := by
  native_decide

/-- The core's spans for a listing are the same `Span` a parser's marks become,
held to the same well-formedness, so the listing needs no grammar of its own.

@proves REQ-VIEW.structure_has_one_type -/
theorem a_synthetic_buffer_meets_the_parsed_rules :
    faults listing = [] ∧ listing.spans.all (fun s => s.role == .path) = true := by
  native_decide

/-- Structure is spans over the text, checked against it: a span past the end,
one running backwards and two overlapping are each faults.

@proves REQ-VIEW.structure_over_text -/
theorem spans_answer_to_the_text :
    faults ⟨"b", .record "b", "abc", [⟨2, 9, .plain, []⟩]⟩ = [.pastTheEnd 2 9 3] ∧
    faults ⟨"b", .record "b", "abc", [⟨2, 1, .plain, []⟩]⟩ = [.backwards 2 1] ∧
    faults ⟨"b", .record "b", "abc", [⟨0, 2, .plain, []⟩, ⟨1, 3, .plain, []⟩]⟩ = [.overlap 0 1] := by
  native_decide

/-- What can be done at a position is the action names its spans declare, each
once, in span order.

@proves REQ-VIEW.affordances_named -/
theorem affordances_are_action_names :
    actionsAt ⟨"b", .record "b", "ab", [⟨0, 2, .path, ["file.open"]⟩, ⟨0, 1, .heading, ["trace.check", "file.open"]⟩]⟩ 0
      = ["file.open", "trace.check"] ∧
    actionsAt ⟨"b", .record "b", "ab", [⟨0, 2, .path, ["file.open"]⟩, ⟨0, 1, .heading, ["trace.check"]⟩]⟩ 1
      = ["file.open"] := by
  native_decide

private theorem no_breach_between_equal_lines (at_ : Nat) (lines : List String) :
    lineBreaches at_ lines lines = [] := by
  induction lines generalizing at_ with
  | nil => simp [lineBreaches]
  | cons line rest ih => simp [lineBreaches, ih]

/-- Every buffer, of every kind, can be rendered conformantly: drawing its text
and offering nothing is always allowed.

@proves REQ-VIEW.rendering_is_total -/
theorem drawing_the_text_always_conforms (buffer : Buffer) :
    conformance buffer { lines := plainText buffer, offered := [] } = [] := by
  simp [conformance, no_breach_between_equal_lines]

/-- What is shown is judged against the core's one buffer: a frontend that drew
its own idea of the text is in breach, line by line.

@proves REQ-VIEW.one_representation -/
theorem a_frontend_computing_its_own_view_is_caught :
    conformance listing { lines := ["main.rs", "deep.rs"], offered := [] }
      = [.lineDiffers 1 "deep.rs" "  deep.rs"] := by
  native_decide

/-- What a frontend drew is read back by the names of what it painted: a glyph
named as its text conforms; a name that is not the text is a breach.

@proves REQ-VIEW.screen_is_readable -/
theorem the_screen_is_read_by_its_names :
    presentedConformance listing [[⟨"📄", "main.rs"⟩], [⟨"  ", "  "⟩, ⟨"📄", "deep.rs"⟩]] [] = [] ∧
    presentedConformance listing [[⟨"📄", "file"⟩], [⟨"  deep.rs", "  deep.rs"⟩]] []
      = [.lineDiffers 0 "file" "main.rs"] := by
  native_decide

end TraceLean.View
