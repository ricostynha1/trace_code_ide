import Lean
import TraceLean.View
import TraceLean.Transcript
import TraceLean.Cost

/-!
# Producing buffers from state

Models `REQ-SHOW`. `REQ-VIEW` says a frontend renders one representation and
computes none of its own; that is only true if something else computes it. This
is that something: for every kind of thing the editor shows, one function from
the state to a buffer.

None of these reads a disk, a terminal or a clock. The state arrives as an
argument, which is what makes each of them a value-to-value function and so
modelled, differentially tested and graded like the rest.

Everything goes through `tidy`, which is why a produced buffer has no faults
whatever it was produced from -- including from marks a parser got wrong, which
is the case nobody writes a test for and the generator finds immediately.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.Produce

open TraceLean.Hash
open TraceLean.Transcript
open TraceLean.View

open Lean (ToJson FromJson)

/-! ## Normalising

A span reaching past the text, running backwards, or overlapping its neighbour
describes something the text does not contain. `faults` reports those; a
producer must not emit them in the first place, whatever it was given.
-/

/-- A region something marked, before it is a span: a parser's output, or a
producer's own idea of where a line sits. -/
structure Mark where
  start : Nat
  stop : Nat
  role : Role
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
What can be done at a region when its role is all that is known.

One place, so a path in a listing and a path in a message offer the same thing.
A producer that knows more than the role -- a menu row, which dispatches one
particular action -- gives its spans actions directly instead.

@models REQ-SHOW.core_produces
-/
def actionsFor : Role → List String
  | Role.plain => []
  | Role.path => ["file.open"]
  | Role.entry => ["file.open"]
  | Role.heading => ["trace.evidence", "trace.check"]
  | Role.requirement => ["trace.requirement", "trace.context", "trace.evidence", "trace.findings"]
  | Role.level _ => ["trace.rollup"]
  | Role.added => ["observe.accept", "observe.reject"]
  | Role.removed => ["observe.accept", "observe.reject"]
  | Role.token _ => []
  | Role.claim _ => []

/-- Turn marks into spans, taking each one's affordances from its role. -/
def spansOfMarks (marks : List Mark) : List Span :=
  marks.map (fun mark =>
    { start := mark.start, stop := mark.stop, role := mark.role,
      actions := actionsFor mark.role })

private def clampSpan (size : Nat) (span : Span) : Span :=
  { start := min span.start size, stop := min span.stop size, role := span.role,
    actions := span.actions }

private def keepsOrder (previous : Nat) : List Span → List Span
  | [] => []
  | span :: rest =>
    -- An empty or backwards span marks nothing, and one starting inside its
    -- predecessor would overlap. Both are dropped rather than moved: moving a
    -- span hides which producer was wrong, and this is the producer's own last
    -- chance to notice.
    if span.stop ≤ span.start then keepsOrder previous rest
    else if span.start < previous then keepsOrder previous rest
    else span :: keepsOrder span.stop rest

/--
Spans that describe the text they are over: in range, in order, not overlapping.

Every producer ends with this, which is what makes `well_formed_by_construction`
a property of all of them rather than of each.

@models REQ-SHOW.well_formed_by_construction
@models REQ-SHOW.producers_are_total
-/
def tidy (size : Nat) (spans : List Span) : List Span :=
  let clamped := spans.map (clampSpan size)
  let sorted := clamped.mergeSort (fun a b => decide (a.start ≤ b.start))
  keepsOrder 0 sorted

/-! ## A file -/

/--
A file as a buffer: its own text, and whatever a parser marked in it.

The marks come from outside because Lean does not parse Rust. What is modelled
is everything that happens to them afterwards, which is where the bugs are.

@models REQ-SHOW.file_from_text
-/
def fileBuffer (path : String) (text : String) (marks : List Mark) : Buffer :=
  { id := "file:" ++ path,
    kind := BufferKind.file path,
    text := text,
    spans := tidy text.length (spansOfMarks marks) }

/-! ## A change under review

A review is a diff, and a diff needs no heuristics to be useful: the lines both
sides share at the front, the lines both sides share at the back, and everything
between them removed and then added. Deterministic, total, and the same answer
every time -- which a heuristic diff is not.
-/

private def commonPrefix : List String → List String → Nat
  | [], _ => 0
  | _, [] => 0
  | a :: as, b :: bs =>
    if a == b then 1 + commonPrefix as bs else 0

/-- One line of a review, and what that line is. -/
structure DiffLine where
  role : Role
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def marked (marker : String) (role : Role) (line : String) : DiffLine :=
  { role := role, text := marker ++ line }

/--
What changed, line by line: context, then what went, then what came.

@models REQ-SHOW.review_from_change
-/
def diffLines (before after : List String) : List DiffLine :=
  let front := commonPrefix before after
  let beforeRest := before.drop front
  let afterRest := after.drop front
  let back := commonPrefix beforeRest.reverse afterRest.reverse
  let removed := beforeRest.take (beforeRest.length - back)
  let added := afterRest.take (afterRest.length - back)
  let tail := beforeRest.drop (beforeRest.length - back)
  (before.take front).map (marked " " Role.plain)
    ++ removed.map (marked "-" Role.removed)
    ++ added.map (marked "+" Role.added)
    ++ tail.map (marked " " Role.plain)

private def lineSpans (at_ : Nat) : List DiffLine → List Span
  | [] => []
  | line :: rest =>
    let here :=
      if line.role == Role.plain then []
      else [({ start := at_, stop := at_ + line.text.length, role := line.role,
               actions := actionsFor line.role } : Span)]
    here ++ lineSpans (at_ + line.text.length + 1) rest

private def lineText (line : DiffLine) : String := line.text

/--
A change between two states of a file, as a buffer a person can accept or reject.

@models REQ-SHOW.producer_is_pure
-/
def reviewBuffer (target : String) (before after : String) : Buffer :=
  let lines := diffLines (before.splitOn "\n") (after.splitOn "\n")
  let text := String.intercalate "\n" (lines.map lineText)
  { id := "review:" ++ target,
    kind := BufferKind.review target,
    text := text,
    spans := tidy text.length (lineSpans 0 lines) }

/-! ## A menu -/

/-- One row of a menu: the key, what it is for, and what it dispatches.

A row that *enters a mode* dispatches nothing, and its span carries no action:
`REQ-VIEW` is explicit that a span's affordances are names the keymap
dispatches, and mode entry is not one of them. -/
structure MenuEntry where
  key : String
  description : String
  action : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def menuLine (entry : MenuEntry) : String :=
  entry.key ++ "  " ++ entry.description

private def menuSpans (at_ : Nat) : List MenuEntry → List Span
  | [] => []
  | entry :: rest =>
    let line := menuLine entry
    let here :=
      ({ start := at_, stop := at_ + line.length, role := Role.entry,
         actions := entry.action.toList } : Span)
    here :: menuSpans (at_ + line.length + 1) rest

/--
A keymap mode as a buffer: one row a key, carrying what that key reaches.

@models REQ-SHOW.menu_from_keymap
-/
def menuBuffer (title : String) (entries : List MenuEntry) : Buffer :=
  let text := String.intercalate "\n" (entries.map menuLine)
  { id := "menu:" ++ title,
    kind := BufferKind.menu title,
    text := text,
    spans := tidy text.length (menuSpans 0 entries) }

/-! ## A record -/

private def eventLine (event : Event) : String :=
  event.kind ++ ": " ++ event.text

private def eventSpans (at_ : Nat) : List Event → List Span
  | [] => []
  | event :: rest =>
    let line := eventLine event
    let here :=
      ({ start := at_, stop := at_ + line.length, role := Role.entry,
         actions := ["observe.diff"] } : Span)
    here :: eventSpans (at_ + line.length + 1) rest

/--
What was observed, as a buffer: one row an event, each leading to its diff.

@models REQ-SHOW.record_from_events
-/
def recordBuffer (title : String) (events : List Event) : Buffer :=
  let text := String.intercalate "\n" (events.map eventLine)
  { id := "record:" ++ title,
    kind := BufferKind.record title,
    text := text,
    spans := tidy text.length (eventSpans 0 events) }

/-! ## A sandboxed agent, watched -/

/--
What a sandboxed agent changed, said and spent, as the rows of one buffer.

Three sources, one order. The changes come first because they are the truth:
the transcript is the tool's account of what it did, and
`REQ-TRANSCRIPT.no_interpretation` is the rule that the account never decides
anything. The estimate comes last because it is a reading of that account
against a table, which is one step further from the workspace again.

@models REQ-SHOW.sandbox_from_observation
-/
def sandboxEvents (changed : List String) (said : List Event)
    (spend : TraceLean.Cost.Spend) : List Event :=
  changed.map (fun path => ({ kind := "changed", text := path } : Event))
    ++ said
    ++ [({ kind := "cost", text := TraceLean.Cost.estimateLine spend } : Event)]

/--
The sandbox station's buffer.

A record like any other record, so the rows a frontend draws and the actions
they carry are the ones `recordBuffer` already gives -- there is no second kind
of buffer here and no second producer.

-/
def sandboxBuffer (changed : List String) (said : List Event)
    (spend : TraceLean.Cost.Spend) : Buffer :=
  recordBuffer "sandbox" (sandboxEvents changed said spend)

/-! ## The requirement set, and the graph it refines along

Two stations' content. Both are lists of requirements with the level their
evidence reached, so both rows look the same and are built by one function; what
differs is the order and the indentation.

The level is carried in the span rather than left for a frontend to read out of
the text. A frontend that parsed `L3` to decide a colour would be computing its
own view of the buffer, which is the one thing `REQ-VIEW` forbids.
-/

/-- A requirement, as a station shows it. -/
structure Node where
  id : String
  title : String
  refines : List String
  level : TraceLean.Evidence.Level
  /-- How many of its clauses something implements, of how many. -/
  implemented : Nat
  clauses : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A node at a depth in the refinement graph. -/
structure Row where
  indent : Nat
  node : Node
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def gradeText (grade : TraceLean.Evidence.Level) : String :=
  match grade with
  | TraceLean.Evidence.Level.L1 => "L1"
  | TraceLean.Evidence.Level.L2 => "L2"
  | TraceLean.Evidence.Level.L3 => "L3"
  | TraceLean.Evidence.Level.L4 => "L4"

private def spaces : Nat → String
  | 0 => ""
  | Nat.succ n => " " ++ spaces n

/-- The two spans every row of either station carries: the grade, and the id.

Both take their actions from their role, so a level in this buffer offers what a
level offers anywhere. The grade is two characters wide, which is why the id
starts four along. -/
private def rowSpans (at_ : Nat) (indent : Nat) (node : Node) : List Span :=
  let lead := at_ + indent * 2
  let level :=
    ({ start := lead, stop := lead + 2, role := Role.level node.level,
       actions := actionsFor (Role.level node.level) } : Span)
  let ident :=
    ({ start := lead + 4, stop := lead + 4 + node.id.length, role := Role.requirement,
       actions := actionsFor Role.requirement } : Span)
  [level, ident]

/-- Cells of five a node's implemented clauses fill, rounded, never past five. -/
private def filledOf (node : Node) : Nat :=
  min 5 ((node.implemented * 5 + node.clauses / 2) / max node.clauses 1)

/-- The coverage bar: filled cells, then empty ones. -/
private def barOf (node : Node) : String :=
  String.mk (List.replicate (filledOf node) '█') ++ String.mk (List.replicate (5 - filledOf node) '░')

private def indexLine (node : Node) : String :=
  gradeText node.level ++ "  " ++ node.id ++ "  " ++ barOf node ++ " " ++
    toString node.implemented ++ "/" ++ toString node.clauses ++ "  " ++ node.title

/-- The bar's filled cells, in the colour of what fills them. -/
private def barSpans (at_ : Nat) (node : Node) : List Span :=
  let start := at_ + 4 + node.id.length + 2
  if filledOf node = 0 then []
  else [{ start := start, stop := start + filledOf node,
          role := Role.claim TraceLean.Annotation.Role.implements,
          actions := actionsFor (Role.claim TraceLean.Annotation.Role.implements) }]

private def indexSpans (at_ : Nat) : List Node → List Span
  | [] => []
  | node :: rest =>
    let line := indexLine node
    rowSpans at_ 0 node ++ barSpans at_ node ++ indexSpans (at_ + line.length + 1) rest

/--
The requirement set as a buffer: one row a requirement, carrying what its
evidence reached and what it is called.

@models REQ-SHOW.index_from_requirements
-/
def requirementsBuffer (nodes : List Node) : Buffer :=
  let text := String.intercalate "\n" (nodes.map indexLine)
  { id := "menu:requirements",
    kind := BufferKind.menu "requirements",
    text := text,
    spans := tidy text.length (indexSpans 0 nodes) }

private def childrenOf (nodes : List Node) (parent : String) : List Node :=
  nodes.filter (fun node => node.refines.contains parent)

/-! `fuel` bounds the depth, and it has to: `refines` is data, so a cycle in it
is representable and a graph walk without a bound would not be a function. A
node reached at the bound is drawn without its children rather than dropped,
because a row missing is a lie about what exists and a row without its children
is only a view cut short. -/

mutual

/-- Expand one node and, under it, everything that refines it. -/
private def expand (fuel : Nat) (nodes : List Node) (indent : Nat) (node : Node) : List Row :=
  match fuel with
  | 0 => [{ indent := indent, node := node }]
  | Nat.succ f =>
    { indent := indent, node := node } ::
      expandAll f nodes (indent + 1) (childrenOf nodes node.id)
termination_by (fuel, 0)

private def expandAll (fuel : Nat) (nodes : List Node) (indent : Nat) : List Node → List Row
  | [] => []
  | node :: rest => expand fuel nodes indent node ++ expandAll fuel nodes indent rest
termination_by todo => (fuel, todo.length + 1)

end

/-- The refinement graph, depth first, roots first.

A root is a requirement that refines nothing -- the architecture documents. Four
levels are drawn, which is one more than the deepest chain this project has. -/
def designRows (nodes : List Node) : List Row :=
  expandAll 3 nodes 0 (nodes.filter (fun node => node.refines.isEmpty))

/-- A row of the graph is a row of the index, indented under what it refines. -/
private def designLine (row : Row) : String :=
  spaces (row.indent * 2) ++ indexLine row.node

private def designSpans (at_ : Nat) : List Row → List Span
  | [] => []
  | row :: rest =>
    let line := designLine row
    rowSpans at_ row.indent row.node ++ barSpans (at_ + row.indent * 2) row.node ++
      designSpans (at_ + line.length + 1) rest

/--
The refinement graph as a buffer: one row a requirement, indented under what it
refines.

@models REQ-SHOW.graph_from_refinement
-/
def designBuffer (nodes : List Node) : Buffer :=
  let rows := designRows nodes
  let text := String.intercalate "\n" (rows.map designLine)
  { id := "menu:design",
    kind := BufferKind.menu "design",
    text := text,
    spans := tidy text.length (designSpans 0 rows) }

/-! ## Showing part of a buffer

A terminal showing forty lines of a long file draws a fraction of it, and a
frontend draws everything it is given -- so what it is given is the fraction.
The window is a buffer like any other, with the same identity, so a delta
carries a scroll the same way it carries an edit.
-/

private def lineStart (lines : List String) (start : Nat) : Nat :=
  ((lines.take start).map (fun line => line.length + 1)).foldl (fun a b => a + b) 0

/-- One span, moved into the window's coordinates and cut to fit.

A span that ends before the window or starts after it marks nothing here and is
dropped. One that straddles an edge is cut: `Nat` subtraction truncating at zero
is exactly the clip wanted at the near edge. -/
private def clipSpan (shift : Nat) (size : Nat) (span : Span) : List Span :=
  match decide (span.stop ≤ shift) with
  | true => []
  | false =>
    match decide (shift + size ≤ span.start) with
    | true => []
    | false =>
      [{ start := span.start - shift, stop := min (span.stop - shift) size,
         role := span.role, actions := span.actions }]

/--
The part of a buffer starting at line `start`, at most `count` lines of it.

@models REQ-SHOW.window_is_a_buffer
-/
def window (buffer : Buffer) (start : Nat) (count : Nat) : Buffer :=
  let lines := plainText buffer
  let taken := (lines.drop start).take count
  let text := String.intercalate "\n" taken
  let shift := lineStart lines start
  let clipped := buffer.spans.foldl (fun acc span => acc ++ clipSpan shift text.length span) []
  { id := buffer.id, kind := buffer.kind, text := text, spans := tidy text.length clipped }

/-! ## What holds of all of them -/

/-- Marks of all three wrong kinds at once: one overlapping its neighbour, one
running backwards, one reaching past the end of the text. A parser that lost
track would emit exactly these. -/
private def wrongMarks : List Mark :=
  [Mark.mk 3 7 Role.heading, Mark.mk 5 9 Role.plain, Mark.mk 9 4 Role.path,
   Mark.mk 0 400 Role.requirement]

/-- A file buffer made from marks that overlap, run backwards and reach past the
end still describes its own text.

@proves REQ-SHOW.well_formed_by_construction -/
-- The sample text holds no brace: the annotation grammar reads a `{` inside a
-- string literal as the start of an interpolation and loses the rest of the
-- declaration (ADR-0008).
theorem a_produced_file_is_well_formed :
    faults (fileBuffer "a.rs" "let x = 1" wrongMarks) = [] := by
  native_decide

/-- And so does a review of two texts that share neither end.

@proves REQ-SHOW.review_from_change -/
theorem a_review_is_well_formed :
    faults (reviewBuffer "a.rs" "one\ntwo\nthree" "one\ntwo!\nthree") = [] := by
  native_decide

/-- A diff keeps what both sides share and says what changed between.

@proves REQ-SHOW.review_from_change -/
theorem a_diff_keeps_the_common_ends :
    diffLines ["one", "two", "three"] ["one", "TWO", "three"] =
      [{ role := Role.plain, text := " one" },
       { role := Role.removed, text := "-two" },
       { role := Role.added, text := "+TWO" },
       { role := Role.plain, text := " three" }] := by
  native_decide

/-- A file of three lines, for the window theorems below to cut up. -/
private def threeLines : Buffer :=
  fileBuffer "a.rs" "one\ntwo\nthree" [Mark.mk 4 7 Role.heading]

/-- A window is a buffer: it describes its own text, and the span that was on
the second line is on the window's first.

@proves REQ-SHOW.window_is_a_buffer -/
theorem a_window_carries_the_spans_that_fall_in_it :
    (window threeLines 1 1).spans = [{ start := 0, stop := 3, role := Role.heading,
                                       actions := actionsFor Role.heading }]
    ∧ faults (window threeLines 1 1) = [] := by
  native_decide

/-- A window past the end is empty rather than an error, and a window of
everything is what it started with.

@proves REQ-SHOW.producers_are_total -/
theorem a_window_is_total :
    (window threeLines 9 4).text = "" ∧ (window threeLines 0 9).text = threeLines.text := by
  native_decide

/-- A menu row that only enters a mode offers nothing to click, and one that
dispatches offers exactly what it dispatches.

@proves REQ-SHOW.menu_from_keymap -/
theorem a_menu_offers_what_its_keys_dispatch :
    (menuBuffer "leader"
      [{ key := "t", description := "trace", action := none },
       { key := "u", description := "undo", action := some "history.undo" }]).spans.map
        (fun span => span.actions) = [[], ["history.undo"]] := by
  native_decide

end TraceLean.Produce
