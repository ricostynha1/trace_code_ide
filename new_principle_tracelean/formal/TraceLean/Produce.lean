import Lean
import TraceLean.View
import TraceLean.Transcript
import TraceLean.Cost

namespace TraceLean.Produce

open TraceLean.Hash
open TraceLean.Transcript
open TraceLean.View

open Lean (ToJson FromJson)

structure Mark where
  start : Nat
  stop : Nat
  role : Role
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-SHOW.actions_by_role -/
def actionsFor : Role → List String
  | Role.plain => []
  | Role.path => ["file.open"]
  | Role.entry => ["file.open"]
  | Role.heading => ["trace.evidence", "trace.check"]
  | Role.requirement => ["trace.requirement", "trace.context", "trace.evidence", "trace.findings"]
  | Role.mark _ _ => ["trace.rollup"]
  | Role.added => ["observe.accept", "observe.reject"]
  | Role.removed => ["observe.accept", "observe.reject"]
  | Role.token _ => []
  | Role.claim _ => []

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
    if span.stop ≤ span.start then keepsOrder previous rest
    else if span.start < previous then keepsOrder previous rest
    else span :: keepsOrder span.stop rest

/-- @models REQ-SHOW.well_formed_by_construction
@models REQ-SHOW.producers_are_total -/
def tidy (size : Nat) (spans : List Span) : List Span :=
  let clamped := spans.map (clampSpan size)
  let sorted := clamped.mergeSort (fun a b => decide (a.start ≤ b.start))
  keepsOrder 0 sorted

/-- @models REQ-SHOW.file_from_text -/
def fileBuffer (path : String) (text : String) (marks : List Mark) : Buffer :=
  { id := "file:" ++ path,
    kind := BufferKind.file path,
    text := text,
    spans := tidy text.length (spansOfMarks marks) }

private def commonPrefix : List String → List String → Nat
  | [], _ => 0
  | _, [] => 0
  | a :: as, b :: bs =>
    if a == b then 1 + commonPrefix as bs else 0

structure DiffLine where
  role : Role
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def marked (marker : String) (role : Role) (line : String) : DiffLine :=
  { role := role, text := marker ++ line }

/-- @models REQ-SHOW.review_from_change -/
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

/-- @models REQ-SHOW.producer_is_pure -/
def reviewBuffer (target : String) (before after : String) : Buffer :=
  let lines := diffLines (before.splitOn "\n") (after.splitOn "\n")
  let text := String.intercalate "\n" (lines.map lineText)
  { id := "review:" ++ target,
    kind := BufferKind.review target,
    text := text,
    spans := tidy text.length (lineSpans 0 lines) }

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

/-- @models REQ-SHOW.menu_from_keymap -/
def menuBuffer (title : String) (entries : List MenuEntry) : Buffer :=
  let text := String.intercalate "\n" (entries.map menuLine)
  { id := "menu:" ++ title,
    kind := BufferKind.menu title,
    text := text,
    spans := tidy text.length (menuSpans 0 entries) }

private def pieceSpans (at_ : Nat) : List (MenuEntry × String) → List Span
  | [] => []
  | (entry, piece) :: rest =>
    ({ start := at_, stop := at_ + piece.length, role := Role.entry,
       actions := entry.action.toList } : Span) :: pieceSpans (at_ + piece.length + 2) rest

/-- @models REQ-LOOK.row_fits -/
def menuRow (title : String) (entries : List MenuEntry) (width : Nat) : Buffer :=
  let full := entries.map menuLine
  let pieces := if (String.intercalate "  " full).length ≤ width then full else entries.map (·.key)
  let text := String.intercalate "  " pieces
  { id := "row:" ++ title,
    kind := BufferKind.menu title,
    text := text,
    spans := tidy text.length (pieceSpans 0 (entries.zip pieces)) }

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

/-- @models REQ-SHOW.record_from_events -/
def recordBuffer (title : String) (events : List Event) : Buffer :=
  let text := String.intercalate "\n" (events.map eventLine)
  { id := "record:" ++ title,
    kind := BufferKind.record title,
    text := text,
    spans := tidy text.length (eventSpans 0 events) }

/-- @models REQ-SHOW.sandbox_from_observation -/
def sandboxEvents (changed : List String) (said : List Event)
    (spend : TraceLean.Cost.Spend) : List Event :=
  changed.map (fun path => ({ kind := "changed", text := path } : Event))
    ++ said
    ++ [({ kind := "cost", text := TraceLean.Cost.estimateLine spend } : Event)]

def sandboxBuffer (changed : List String) (said : List Event)
    (spend : TraceLean.Cost.Spend) : Buffer :=
  recordBuffer "sandbox" (sandboxEvents changed said spend)

structure Tally where
  implemented : Nat
  annotated : Nat
  judged : Nat
  tested : Nat
  proved : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Node where
  id : String
  title : String
  refines : List String
  marks : TraceLean.Evidence.Marks
  clauses : Nat
  modelled : Nat
  held : Tally
  covered : Option (Nat × Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Row where
  indent : Nat
  node : Node
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def spaces : Nat → String
  | 0 => ""
  | Nat.succ n => " " ++ spaces n

structure Piece where
  text : String
  role : Option Role
  actions : List String

private def bare (text : String) : Piece := ⟨text, none, []⟩

private def spanned (text : String) (role : Role) : Piece := ⟨text, some role, actionsFor role⟩

private def piecesText (pieces : List Piece) : String := String.join (pieces.map (·.text))

private def piecesSpans (at_ : Nat) : List Piece → List Span
  | [] => []
  | p :: rest =>
    let stop := at_ + p.text.length
    (match p.role with
     | some r => [({ start := at_, stop := stop, role := r, actions := p.actions } : Span)]
     | none => []) ++ piecesSpans stop rest

private def linesSpans (at_ : Nat) : List (List Piece) → List Span
  | [] => []
  | row :: rest => piecesSpans at_ row ++ linesSpans (at_ + (piecesText row).length + 1) rest

private def childrenOf (nodes : List Node) (parent : String) : List Node :=
  nodes.filter (fun node => node.refines.contains parent)

private def linesUnder (nodes : List Node) : Nat → Node → Nat × Nat
  | 0, node => node.covered.getD (0, 0)
  | Nat.succ f, node =>
    (childrenOf nodes node.id).foldl
      (fun acc child => (acc.1 + (linesUnder nodes f child).1, acc.2 + (linesUnder nodes f child).2))
      (node.covered.getD (0, 0))

private def childrenShort (nodes : List Node) (node : Node) : Option String :=
  let sum := (childrenOf nodes node.id).foldl
    (fun acc child => (acc.1 + (linesUnder nodes nodes.length child).1,
                       acc.2 + (linesUnder nodes nodes.length child).2)) (0, 0)
  let own := node.covered.getD (0, 0)
  if sum.1 < sum.2 then some ("+children " ++ toString ((own.1 + sum.1) * 100 / (own.2 + sum.2)) ++ "%")
  else none

private def countPiece (letter : String) (held of_ : Nat) (role : Role) : Piece :=
  if of_ == 0 then bare (letter ++ "·") else spanned (letter ++ toString held ++ "/" ++ toString of_) role

def isOpen (node : Node) : Bool :=
  let t := node.held
  decide (t.implemented < node.modelled) || decide (t.tested < node.modelled) ||
    decide (t.proved < node.modelled) || decide (t.annotated < node.clauses) ||
    decide (t.judged < node.clauses) ||
    (match node.covered with
     | some c => decide (c.1 < c.2)
     | none => false)

private def markPiece (m : TraceLean.Evidence.Marks) (each : TraceLean.Evidence.Mark × Bool × Char) : Piece :=
  if m.applies each.1 then
    spanned (String.singleton (TraceLean.Evidence.letter each.2.1 each.2.2)) (Role.mark each.1 each.2.1)
  else bare "·"

private def linesPieces (node : Node) : List Piece :=
  match node.covered with
  | none => []
  | some c =>
    (if c.2 == 0 then bare "lines none"
     else ⟨"lines " ++ toString (c.1 * 100 / c.2) ++ "%",
           some (if c.1 == c.2 then Role.added else Role.removed),
           ["trace.coverage " ++ node.id]⟩) :: [bare "  "]

private def rowPieces (nodes : List Node) (node : Node) : List Piece :=
  let t := node.held
  let held := fun (m : TraceLean.Evidence.Mark) => Role.mark m true
  let counts :=
    [countPiece "I" t.implemented node.modelled (Role.claim TraceLean.Annotation.Role.implements),
     countPiece "A" t.annotated node.clauses (held .annotated),
     countPiece "J" t.judged node.clauses (held .judged),
     countPiece "T" t.tested node.modelled (held .tested),
     countPiece "P" t.proved node.modelled (held .proved)]
  let under :=
    match childrenShort nodes node with
    | some said => [⟨said, some Role.removed, []⟩, bare "  "]
    | none => []
  node.marks.each.map (markPiece node.marks) ++ [bare "  ", spanned node.id Role.requirement, bare "  "] ++
    counts.intersperse (bare " ") ++ [bare "  "] ++ linesPieces node ++ under ++ [bare node.title]

/-- @models REQ-SHOW.index_from_requirements
@models REQ-SHOW.children_lines -/
def requirementsBuffer (nodes : List Node) (openOnly : Bool) : Buffer :=
  let rows := (nodes.filter (fun node => !openOnly || isOpen node)).map (rowPieces nodes)
  let text := String.intercalate "\n" (rows.map piecesText)
  { id := "menu:requirements",
    kind := BufferKind.menu "requirements",
    text := text,
    spans := tidy text.length (linesSpans 0 rows) }

private def openUnder (nodes : List Node) : Nat → Node → Bool
  | 0, node => isOpen node
  | Nat.succ f, node => isOpen node || (childrenOf nodes node.id).any (openUnder nodes f)

def shown (nodes : List Node) (openOnly : Bool) : List Node :=
  nodes.filter (fun node => !openOnly || openUnder nodes nodes.length node)

mutual

private def expand (fuel : Nat) (nodes : List Node) (opened : List String) (indent : Nat) (node : Node) : List Row :=
  match fuel with
  | 0 => [{ indent := indent, node := node }]
  | Nat.succ f =>
    { indent := indent, node := node } ::
      (if opened.contains node.id then expandAll f nodes opened (indent + 1) (childrenOf nodes node.id) else [])
termination_by (fuel, 0)

private def expandAll (fuel : Nat) (nodes : List Node) (opened : List String) (indent : Nat) : List Node → List Row
  | [] => []
  | node :: rest => expand fuel nodes opened indent node ++ expandAll fuel nodes opened indent rest
termination_by todo => (fuel, todo.length + 1)

end

def designRows (nodes : List Node) (opened : List String) : List Row :=
  expandAll nodes.length nodes opened 0 (nodes.filter (fun node => node.refines.isEmpty))

private def foldMark (nodes : List Node) (opened : List String) (node : Node) : String :=
  if (childrenOf nodes node.id).isEmpty then "·"
  else if opened.contains node.id then "▾" else "▸"

private def designPieces (graph all : List Node) (opened : List String) (row : Row) : List Piece :=
  let mark := foldMark graph opened row.node
  let fold : Piece :=
    if (childrenOf graph row.node.id).isEmpty then bare mark
    else ⟨mark, some Role.entry, ["design.toggle " ++ row.node.id, "design.expand_all " ++ row.node.id]⟩
  [bare (spaces (row.indent * 2)), fold, bare " "] ++ rowPieces all row.node

def designHeader (openOnly : Bool) : List (String × String) :=
  [("[ expand all ]", "design.expand_everything"), ("[ fold all ]", "design.fold_all"),
   (if openOnly then "[ show all ]" else "[ open only ]", "design.open_only")]

private def headerPieces (openOnly : Bool) : List Piece :=
  ((designHeader openOnly).map (fun b => (⟨b.1, some Role.entry, [b.2]⟩ : Piece))).intersperse (bare "  ")

/-- @models REQ-SHOW.graph_from_refinement
@models REQ-SHOW.open_filtered -/
def designBuffer (nodes : List Node) (opened : List String) (openOnly : Bool) : Buffer :=
  let graph := shown nodes openOnly
  let rows := designRows graph opened
  let lines := headerPieces openOnly :: rows.map (designPieces graph nodes opened)
  let text := String.intercalate "\n" (lines.map piecesText)
  { id := "menu:design",
    kind := BufferKind.menu "design",
    text := text,
    spans := tidy text.length (linesSpans 0 lines) }

private def lineStart (lines : List String) (start : Nat) : Nat :=
  ((lines.take start).map (fun line => line.length + 1)).foldl (fun a b => a + b) 0

private def clipSpan (shift : Nat) (size : Nat) (span : Span) : List Span :=
  match decide (span.stop ≤ shift) with
  | true => []
  | false =>
    match decide (shift + size ≤ span.start) with
    | true => []
    | false =>
      [{ start := span.start - shift, stop := min (span.stop - shift) size,
         role := span.role, actions := span.actions }]

/-- @models REQ-SHOW.window_is_a_buffer -/
def window (buffer : Buffer) (start : Nat) (count : Nat) : Buffer :=
  let lines := plainText buffer
  let taken := (lines.drop start).take count
  let text := String.intercalate "\n" taken
  let shift := lineStart lines start
  let clipped := buffer.spans.foldl (fun acc span => acc ++ clipSpan shift text.length span) []
  { id := buffer.id, kind := buffer.kind, text := text, spans := tidy text.length clipped }

private def wrongMarks : List Mark :=
  [Mark.mk 3 7 Role.heading, Mark.mk 5 9 Role.plain, Mark.mk 9 4 Role.path,
   Mark.mk 0 400 Role.requirement]

/-- @proves REQ-SHOW.well_formed_by_construction -/
theorem a_produced_file_is_well_formed :
    faults (fileBuffer "a.rs" "let x = 1" wrongMarks) = [] := by
  native_decide

/-- @proves REQ-SHOW.review_from_change -/
theorem a_review_is_well_formed :
    faults (reviewBuffer "a.rs" "one\ntwo\nthree" "one\ntwo!\nthree") = [] := by
  native_decide

/-- @proves REQ-SHOW.review_from_change -/
theorem a_diff_keeps_the_common_ends :
    diffLines ["one", "two", "three"] ["one", "TWO", "three"] =
      [{ role := Role.plain, text := " one" },
       { role := Role.removed, text := "-two" },
       { role := Role.added, text := "+TWO" },
       { role := Role.plain, text := " three" }] := by
  native_decide

private def threeLines : Buffer :=
  fileBuffer "a.rs" "one\ntwo\nthree" [Mark.mk 4 7 Role.heading]

/-- @proves REQ-SHOW.window_is_a_buffer -/
theorem a_window_carries_the_spans_that_fall_in_it :
    (window threeLines 1 1).spans = [{ start := 0, stop := 3, role := Role.heading,
                                       actions := actionsFor Role.heading }]
    ∧ faults (window threeLines 1 1) = [] := by
  native_decide

/-- @proves REQ-SHOW.producers_are_total -/
theorem a_window_is_total :
    (window threeLines 9 4).text = "" ∧ (window threeLines 0 9).text = threeLines.text := by
  native_decide

/-- @proves REQ-SHOW.menu_from_keymap -/
theorem a_menu_offers_what_its_keys_dispatch :
    (menuBuffer "leader"
      [{ key := "t", description := "trace", action := none },
       { key := "u", description := "undo", action := some "history.undo" }]).spans.map
        (fun span => span.actions) = [[], ["history.undo"]] := by
  native_decide

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

def graph : List Node :=
  [⟨"ARCH-X", "Root", [], ⟨true, true, false, false, false⟩, 1, 1, ⟨1, 1, 1, 0, 0⟩, none⟩,
   ⟨"REQ-A", "Mid", ["ARCH-X"], ⟨true, true, true, false, false⟩, 2, 2, ⟨1, 2, 2, 2, 0⟩, some (3, 4)⟩,
   ⟨"REQ-B", "Leaf", ["REQ-A"], ⟨true, false, false, false, true⟩, 1, 0, ⟨0, 1, 0, 0, 0⟩, some (0, 2)⟩]

/-- @proves REQ-SHOW.actions_by_role -/
theorem a_role_offers_its_actions (marks : List Mark) :
    (spansOfMarks marks).map (·.actions) = marks.map (fun m => actionsFor m.role) ∧
    ((requirementsBuffer graph false).spans.filter (·.role == .requirement)).map (·.actions)
      = [actionsFor .requirement, actionsFor .requirement, actionsFor .requirement] ∧
    ((designBuffer graph ["ARCH-X", "REQ-A"] false).spans.filter (·.role == .requirement)).map (·.actions)
      = [actionsFor .requirement, actionsFor .requirement, actionsFor .requirement] := by
  refine ⟨?_, ?_⟩
  · simp [spansOfMarks]
  · native_decide

/-- @proves REQ-SHOW.file_from_text -/
theorem a_file_is_its_text (path text : String) (marks : List Mark) :
    (fileBuffer path text marks).text = text ∧
    (fileBuffer "a.rs" "fn f" [⟨0, 2, .path⟩]).spans = [⟨0, 2, .path, ["file.open"]⟩] := by
  refine ⟨rfl, ?_⟩
  native_decide

/-- @proves REQ-SHOW.record_from_events -/
theorem events_become_marked_rows :
    linked (recordBuffer "obs" [⟨"changed", "a.rs"⟩, ⟨"said", "hi"⟩])
      = [("changed: a.rs", ["observe.diff"]), ("said: hi", ["observe.diff"])] := by
  native_decide

/-- @proves REQ-SHOW.index_from_requirements -/
theorem one_row_a_requirement :
    (requirementsBuffer graph false).text
      = "AJ--  ARCH-X  I1/1 A1/1 J1/1 T0/1 P0/1  +children 50%  Root\n" ++
        "AJT-  REQ-A  I1/2 A2/2 J2/2 T2/2 P0/2  lines 75%  +children 50%  Mid\n" ++
        "A-··  REQ-B  I· A1/1 J0/1 T· P·  lines 0%  Leaf" ∧
    ((requirementsBuffer graph false).spans.filter (fun s => s.role == .mark .tested true)).length = 3 ∧
    (linked (requirementsBuffer graph false)).filter (fun l => l.2 != ["trace.rollup"]) =
      [("ARCH-X", actionsFor .requirement), ("REQ-A", actionsFor .requirement),
       ("lines 75%", ["trace.coverage REQ-A"]), ("REQ-B", actionsFor .requirement),
       ("lines 0%", ["trace.coverage REQ-B"])] := by
  native_decide

/-- @proves REQ-SHOW.graph_from_refinement -/
theorem the_graph_unfolds_where_asked :
    (designBuffer graph [] false).text
      = "[ expand all ]  [ fold all ]  [ open only ]\n▸ AJ--  ARCH-X  I1/1 A1/1 J1/1 T0/1 P0/1  +children 50%  Root" ∧
    (designBuffer graph ["ARCH-X"] false).text
      = "[ expand all ]  [ fold all ]  [ open only ]\n▾ AJ--  ARCH-X  I1/1 A1/1 J1/1 T0/1 P0/1  +children 50%  Root\n" ++
        "  ▸ AJT-  REQ-A  I1/2 A2/2 J2/2 T2/2 P0/2  lines 75%  +children 50%  Mid" ∧
    (designBuffer graph ["ARCH-X", "REQ-A"] false).text
      = "[ expand all ]  [ fold all ]  [ open only ]\n▾ AJ--  ARCH-X  I1/1 A1/1 J1/1 T0/1 P0/1  +children 50%  Root\n" ++
        "  ▾ AJT-  REQ-A  I1/2 A2/2 J2/2 T2/2 P0/2  lines 75%  +children 50%  Mid\n" ++
        "    · A-··  REQ-B  I· A1/1 J0/1 T· P·  lines 0%  Leaf" := by
  native_decide

def finished (id : String) (parent : String) (covered : Nat × Nat) : Node :=
  ⟨id, "Done", [parent], ⟨true, true, true, true, false⟩, 1, 1, ⟨1, 1, 1, 1, 1⟩, some covered⟩

/-- @proves REQ-SHOW.children_lines -/
theorem a_row_says_what_is_left_unrun_under_it :
    ((requirementsBuffer graph false).text.splitOn "\n").map (fun l => (l.splitOn "+children ").length)
      = [2, 2, 1] ∧
    (requirementsBuffer [⟨"ARCH-X", "Root", [], ⟨true, true, true, true, false⟩, 1, 1, ⟨1, 1, 1, 1, 1⟩,
      some (2, 2)⟩, finished "REQ-C" "ARCH-X" (3, 3)] false).text
      = "AJTP  ARCH-X  I1/1 A1/1 J1/1 T1/1 P1/1  lines 100%  Root\n" ++
        "AJTP  REQ-C  I1/1 A1/1 J1/1 T1/1 P1/1  lines 100%  Done" := by
  native_decide

/-- @proves REQ-SHOW.open_filtered -/
theorem open_only_keeps_what_is_still_to_do :
    ((requirementsBuffer (graph ++ [finished "REQ-C" "ARCH-X" (1, 1)]) true).text.splitOn "REQ-C").length = 1 ∧
    ((requirementsBuffer (graph ++ [finished "REQ-C" "ARCH-X" (1, 1)]) false).text.splitOn "REQ-C").length = 2 ∧
    (designBuffer (graph ++ [finished "REQ-C" "ARCH-X" (1, 1)]) ["ARCH-X", "REQ-A"] true).text
      = "[ expand all ]  [ fold all ]  [ show all ]\n▾ AJ--  ARCH-X  I1/1 A1/1 J1/1 T0/1 P0/1  +children 57%  Root\n" ++
        "  ▾ AJT-  REQ-A  I1/2 A2/2 J2/2 T2/2 P0/2  lines 75%  +children 50%  Mid\n" ++
        "    · A-··  REQ-B  I· A1/1 J0/1 T· P·  lines 0%  Leaf" := by
  native_decide

/-- @proves REQ-SHOW.sandbox_from_observation -/
theorem changes_then_words_then_cost :
    (sandboxBuffer ["a.rs"] [⟨"agent", "done"⟩] ⟨0, []⟩).text
      = "changed: a.rs\nagent: done\ncost: estimated 0.000000" := by
  native_decide

/-- @proves REQ-SHOW.producer_is_pure -/
theorem a_producer_reads_only_its_arguments :
    designBuffer graph ["REQ-A", "ARCH-X"] true = designBuffer graph ["ARCH-X", "REQ-A"] true ∧
    reviewBuffer "a.rs" "one\ntwo" "one\nTWO" = reviewBuffer "a.rs" "one\ntwo" "one\nTWO" := by
  native_decide

/-- @proves REQ-LOOK.row_fits -/
theorem a_row_fits_its_width :
    linked (menuRow "m" [⟨"o", "open", some "file.open"⟩, ⟨"s", "save", some "file.save"⟩] 30)
      = [("o  open", ["file.open"]), ("s  save", ["file.save"])] ∧
    linked (menuRow "m" [⟨"o", "open", some "file.open"⟩, ⟨"s", "save", some "file.save"⟩] 10)
      = [("o", ["file.open"]), ("s", ["file.save"])] := by
  native_decide

end TraceLean.Produce
