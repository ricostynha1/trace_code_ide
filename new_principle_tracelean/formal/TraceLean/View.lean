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

/-- @models REQ-VIEW.one_representation
@models REQ-VIEW.everything_is_a_buffer -/
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
@models REQ-SHOW.listing_from_entries -/
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

/-- @models REQ-VIEW.rendering_is_total
@models REQ-VIEW.frontend_adds_nothing -/
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
theorem drawing_the_text_always_conforms (buffer : Buffer) :
    conformance buffer { lines := plainText buffer, offered := [] } = [] := by
  simp [conformance, no_breach_between_equal_lines]

end TraceLean.View
