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

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

def graph : List Node :=
  [⟨"ARCH-X", "Root", [], ⟨true, true, false, false, false⟩, 1, 1, ⟨1, 1, 1, 0, 0⟩, none⟩,
   ⟨"REQ-A", "Mid", ["ARCH-X"], ⟨true, true, true, false, false⟩, 2, 2, ⟨1, 2, 2, 2, 0⟩, some (3, 4)⟩,
   ⟨"REQ-B", "Leaf", ["REQ-A"], ⟨true, false, false, false, true⟩, 1, 0, ⟨0, 1, 0, 0, 0⟩, some (0, 2)⟩]

/-- @proves REQ-SHOW.actions_by_role -/
theorem a_role_offers_its_actions (marks : List Mark) :
    (spansOfMarks marks).map (·.actions) = marks.map (fun m => actionsFor m.role) := by
  simp [spansOfMarks]

/-- @proves REQ-SHOW.file_from_text -/
theorem a_file_is_its_text (path text : String) (marks : List Mark) :
    (fileBuffer path text marks).text = text := rfl

def finished (id : String) (parent : String) (covered : Nat × Nat) : Node :=
  ⟨id, "Done", [parent], ⟨true, true, true, true, false⟩, 1, 1, ⟨1, 1, 1, 1, 1⟩, some covered⟩


private def between : List Span → List Fault
  | [] => []
  | _ :: [] => []
  | a :: b :: rest =>
    let here :=
      if b.start < a.start then [Fault.unordered a.start b.start]
      else if b.start < a.stop then [Fault.overlap a.start b.start]
      else []
    here ++ between (b :: rest)

private theorem faults_eq (buffer : Buffer) : faults buffer =
    buffer.spans.foldl
    (fun acc span =>
      let bad :=
        if span.stop < span.start then [Fault.backwards span.start span.stop]
        else if buffer.text.length < span.stop then [Fault.pastTheEnd span.start span.stop buffer.text.length]
        else []
      acc ++ bad) [] ++ between buffer.spans := by
  unfold faults
  simp only []
  congr 1
  generalize buffer.spans = l
  induction l with
  | nil => rfl
  | cons a t ih =>
    cases t with
    | nil => rfl
    | cons b r => rw [between, ← ih]; rfl

def Fits : Nat → List Span → Nat → Prop
  | p, [], q => p ≤ q
  | p, s :: rest, q => p ≤ s.start ∧ s.start ≤ s.stop ∧ Fits s.stop rest q

theorem fits_le {p q : Nat} {l : List Span} (f : Fits p l q) : p ≤ q := by
  induction l generalizing p with
  | nil => exact f
  | cons s rest ih => exact Nat.le_trans f.1 (Nat.le_trans f.2.1 (ih f.2.2))

theorem fits_lower {p p' q : Nat} {l : List Span} (h : p' ≤ p) (f : Fits p l q) : Fits p' l q := by
  cases l with
  | nil => exact Nat.le_trans h f
  | cons s rest => exact ⟨Nat.le_trans h f.1, f.2⟩

theorem fits_upper {p q q' : Nat} {l : List Span} (f : Fits p l q) (h : q ≤ q') : Fits p l q' := by
  induction l generalizing p with
  | nil => exact Nat.le_trans f h
  | cons s rest ih => exact ⟨f.1, f.2.1, ih f.2.2⟩

theorem fits_append {p q r : Nat} {l m : List Span} (f : Fits p l q) (g : Fits q m r) :
    Fits p (l ++ m) r := by
  induction l generalizing p with
  | nil => exact fits_lower f g
  | cons s rest ih => exact ⟨f.1, f.2.1, ih f.2.2⟩

theorem fits_bound {p q : Nat} {l : List Span} (f : Fits p l q) :
    ∀ s ∈ l, p ≤ s.start ∧ s.start ≤ s.stop ∧ s.stop ≤ q := by
  induction l generalizing p with
  | nil => intro s h; cases h
  | cons a rest ih =>
    intro s h
    cases h with
    | head => exact ⟨f.1, f.2.1, fits_le f.2.2⟩
    | tail _ h =>
      have := ih f.2.2 s h
      exact ⟨Nat.le_trans f.1 (Nat.le_trans f.2.1 this.1), this.2⟩

theorem fits_pairwise {p q : Nat} {l : List Span} (f : Fits p l q) :
    l.Pairwise (fun a b => decide (a.start ≤ b.start) = true) := by
  induction l generalizing p with
  | nil => exact List.Pairwise.nil
  | cons a rest ih =>
    refine List.Pairwise.cons ?_ (ih f.2.2)
    intro b hb
    have := fits_bound f.2.2 b hb
    simp only [decide_eq_true_eq]
    exact Nat.le_trans f.2.1 this.1

private theorem keepsOrder_fits {p q : Nat} {l : List Span} (f : Fits p l q) :
    keepsOrder p l = l.filter (fun s => decide (s.start < s.stop)) := by
  induction l generalizing p with
  | nil => rfl
  | cons s rest ih =>
    have h2 : ¬ s.start < p := Nat.not_lt.mpr f.1
    by_cases h1 : s.stop ≤ s.start
    · have h3 : ¬ s.start < s.stop := Nat.not_lt.mpr h1
      rw [keepsOrder, if_pos h1, ih (fits_lower (Nat.le_trans f.1 f.2.1) f.2.2)]
      simp [List.filter, h3]
    · have h3 : s.start < s.stop := Nat.lt_of_not_le h1
      rw [keepsOrder, if_neg h1, if_neg h2, ih f.2.2]
      simp [List.filter, h3]

private theorem map_clamp {n : Nat} {l : List Span}
    (h : ∀ s ∈ l, s.start ≤ s.stop ∧ s.stop ≤ n) : l.map (clampSpan n) = l := by
  induction l with
  | nil => rfl
  | cons s rest ih =>
    have hs := h s (List.mem_cons_self _ _)
    have hr : ∀ x ∈ rest, x.start ≤ x.stop ∧ x.stop ≤ n := fun x hx => h x (List.mem_cons_of_mem _ hx)
    rw [List.map_cons, ih hr]
    congr 1
    cases s
    simp only [clampSpan] at hs ⊢
    rw [Nat.min_eq_left (Nat.le_trans hs.1 hs.2), Nat.min_eq_left hs.2]

theorem tidy_of_fits {n : Nat} {l : List Span} (f : Fits 0 l n) :
    tidy n l = l.filter (fun s => decide (s.start < s.stop)) := by
  unfold tidy
  simp only []
  rw [map_clamp (fun s hs => (fits_bound f s hs).2), List.mergeSort_of_sorted (fits_pairwise f),
    keepsOrder_fits f]

private theorem mem_keepsOrder {p : Nat} {l : List Span} {s : Span} (h : s ∈ keepsOrder p l) : s ∈ l := by
  induction l generalizing p with
  | nil => cases h
  | cons a rest ih =>
    rw [keepsOrder] at h
    split at h
    · exact List.mem_cons_of_mem _ (ih h)
    · split at h
      · exact List.mem_cons_of_mem _ (ih h)
      · cases h with
        | head => exact List.mem_cons_self _ _
        | tail _ h => exact List.mem_cons_of_mem _ (ih h)

theorem mem_tidy {n : Nat} {l : List Span} {s : Span} (h : s ∈ tidy n l) :
    ∃ s' ∈ l, s.role = s'.role ∧ s.actions = s'.actions ∧ s.start = min s'.start n ∧
      s.stop = min s'.stop n := by
  unfold tidy at h
  simp only [] at h
  have h1 := mem_keepsOrder h
  rw [(List.mergeSort_perm _ _).mem_iff, List.mem_map] at h1
  obtain ⟨s', hs', rfl⟩ := h1
  exact ⟨s', hs', rfl, rfl, rfl, rfl⟩

private theorem keepsOrder_strict {n : Nat} : ∀ {p : Nat} {l : List Span},
    (∀ s ∈ l, s.stop ≤ n) → p ≤ n →
    Fits p (keepsOrder p l) n ∧ ∀ s ∈ keepsOrder p l, s.start < s.stop := by
  intro p l
  induction l generalizing p with
  | nil => intro _ hp; exact ⟨hp, fun s h => by cases h⟩
  | cons a rest ih =>
    intro h hp
    have hr : ∀ x ∈ rest, x.stop ≤ n := fun x hx => h x (List.mem_cons_of_mem _ hx)
    rw [keepsOrder]
    split
    · exact ih hr hp
    · split
      · exact ih hr hp
      · rename_i h1 h2
        have ha := h a (List.mem_cons_self _ _)
        have ⟨f, g⟩ := ih (p := a.stop) hr ha
        refine ⟨⟨Nat.le_of_not_lt h2, Nat.le_of_lt (Nat.lt_of_not_le h1), f⟩, ?_⟩
        intro s hs
        cases hs with
        | head => exact Nat.lt_of_not_le h1
        | tail _ hs => exact g s hs

theorem tidy_strict (n : Nat) (l : List Span) :
    Fits 0 (tidy n l) n ∧ ∀ s ∈ tidy n l, s.start < s.stop := by
  unfold tidy
  simp only []
  apply keepsOrder_strict _ (Nat.zero_le _)
  intro s hs
  rw [(List.mergeSort_perm _ _).mem_iff, List.mem_map] at hs
  obtain ⟨s', _, rfl⟩ := hs
  exact Nat.min_le_right _ _

private theorem foldl_no_bad (size : Nat) : ∀ (l : List Span) (acc : List Fault),
    (∀ s ∈ l, s.start < s.stop ∧ s.stop ≤ size) →
    l.foldl (fun acc span =>
      let bad :=
        if span.stop < span.start then [Fault.backwards span.start span.stop]
        else if size < span.stop then [Fault.pastTheEnd span.start span.stop size]
        else []
      acc ++ bad) acc = acc := by
  intro l
  induction l with
  | nil => intro acc _; rfl
  | cons a rest ih =>
    intro acc h
    have ha := h a (List.mem_cons_self _ _)
    have hr : ∀ x ∈ rest, x.start < x.stop ∧ x.stop ≤ size := fun x hx => h x (List.mem_cons_of_mem _ hx)
    have n1 : ¬ a.stop < a.start := Nat.not_lt.mpr (Nat.le_of_lt ha.1)
    have n2 : ¬ size < a.stop := Nat.not_lt.mpr ha.2
    simp only [List.foldl, n1, n2, if_false, List.append_nil]
    exact ih acc hr

private theorem between_fits : ∀ {p q : Nat} {l : List Span},
    Fits p l q → (∀ s ∈ l, s.start < s.stop) → between l = [] := by
  intro p q l
  induction l generalizing p with
  | nil => intro _ _; rfl
  | cons a rest ih =>
    intro f h
    cases rest with
    | nil => rfl
    | cons b r =>
      have ha := h a (List.mem_cons_self _ _)
      have hb : b.start ≥ a.stop := f.2.2.1
      have n1 : ¬ b.start < a.start := by omega
      have n2 : ¬ b.start < a.stop := by omega
      rw [between]
      simp only [n1, n2, if_false, List.nil_append]
      exact ih f.2.2 (fun x hx => h x (List.mem_cons_of_mem _ hx))

theorem no_fault_when_fits (b : Buffer) (f : Fits 0 b.spans b.text.length)
    (h : ∀ s ∈ b.spans, s.start < s.stop) : faults b = [] := by
  rw [faults_eq, foldl_no_bad _ _ _ (fun s hs => ⟨h s hs, (fits_bound f s hs).2.2⟩), between_fits f h]
  rfl

/-- @proves REQ-SHOW.well_formed_by_construction -/
theorem tidy_leaves_no_fault (id : String) (kind : BufferKind) (text : String) (spans : List Span) :
    faults { id := id, kind := kind, text := text, spans := tidy text.length spans } = [] :=
  no_fault_when_fits _ (tidy_strict _ _).1 (tidy_strict _ _).2

def textUnder (b : Buffer) (s : Span) : String :=
  String.mk ((b.text.toList.drop s.start).take (s.stop - s.start))

def Shows (text : String) (s : Span) (said : String) : Prop :=
  ∃ before after, text = before ++ said ++ after ∧ before.length = s.start ∧
    s.stop = s.start + said.length

theorem textUnder_of_shows {b : Buffer} {s : Span} {said : String} (h : Shows b.text s said) :
    textUnder b s = said := by
  obtain ⟨A, B, h1, h2, h3⟩ := h
  unfold textUnder
  rw [h1, h3, Nat.add_sub_cancel_left, ← h2]
  show String.mk ((((A ++ said ++ B).data).drop A.length).take said.length) = said
  rw [String.data_append, String.data_append, List.append_assoc]
  cases A with
  | mk a =>
    cases said with
    | mk t =>
      show String.mk ((List.drop a.length (a ++ (t ++ B.data))).take t.length) = String.mk t
      rw [List.drop_left, List.take_left]

theorem shows_extend {text : String} {s : Span} {said : String} (more : String)
    (h : Shows text s said) : Shows (text ++ more) s said := by
  obtain ⟨A, B, h1, h2, h3⟩ := h
  exact ⟨A, B ++ more, by rw [h1]; simp only [String.append_assoc], h2, h3⟩

theorem nonempty_length {t : String} (h : t ≠ "") : 0 < t.length := by
  cases t with
  | mk d =>
    cases d with
    | nil => exact absurd rfl h
    | cons c r => simp [String.length]

private theorem foldl_append_string : ∀ (l : List String) (acc : String),
    l.foldl (fun r s => r ++ s) acc = acc ++ l.foldl (fun r s => r ++ s) "" := by
  intro l
  induction l with
  | nil => intro acc; simp
  | cons b r ih =>
    intro acc
    simp only [List.foldl]
    rw [ih (acc ++ b), ih ("" ++ b), String.empty_append, String.append_assoc]

theorem join_cons (a : String) (l : List String) : String.join (a :: l) = a ++ String.join l := by
  unfold String.join
  simp only [List.foldl]
  rw [foldl_append_string, String.empty_append]

private theorem go_eq (sep : String) : ∀ (l : List String) (acc : String),
    String.intercalate.go acc sep l = acc ++ String.join (l.map (fun x => sep ++ x)) := by
  intro l
  induction l with
  | nil => intro acc; simp [String.intercalate.go, String.join]
  | cons b r ih =>
    intro acc
    rw [String.intercalate.go, ih, List.map_cons, join_cons]
    simp only [String.append_assoc]

theorem intercalate_one (sep a : String) : String.intercalate sep [a] = a := by
  simp [String.intercalate, String.intercalate.go]

theorem intercalate_cons_cons (sep a b : String) (r : List String) :
    String.intercalate sep (a :: b :: r) = a ++ sep ++ String.intercalate sep (b :: r) := by
  simp only [String.intercalate]
  rw [go_eq, go_eq, List.map_cons, join_cons]
  simp only [String.append_assoc]

theorem pieces_text_cons (p : Piece) (rest : List Piece) :
    piecesText (p :: rest) = p.text ++ piecesText rest := by
  simp only [piecesText, List.map_cons, join_cons]

theorem pieces_fits : ∀ (ps : List Piece) (at_ : Nat),
    Fits at_ (piecesSpans at_ ps) (at_ + (piecesText ps).length) := by
  intro ps
  induction ps with
  | nil => intro at_; simp [piecesSpans, piecesText, String.join, Fits]
  | cons p rest ih =>
    intro at_
    rw [pieces_text_cons, String.length_append, ← Nat.add_assoc]
    simp only [piecesSpans]
    cases p.role with
    | none => exact fits_lower (Nat.le_add_right _ _) (ih _)
    | some r =>
      exact fits_append (q := at_ + p.text.length)
        ⟨Nat.le_refl _, Nat.le_add_right _ _, Nat.le_refl _⟩ (ih _)

theorem pieces_show : ∀ (ps : List Piece) (at_ : Nat) (P M : String), P.length = at_ →
    ∀ p ∈ ps, ∀ r, p.role = some r →
    ∃ s ∈ piecesSpans at_ ps, s.role = r ∧ s.actions = p.actions ∧
      Shows (P ++ piecesText ps ++ M) s p.text := by
  intro ps
  induction ps with
  | nil => intro _ _ _ _ p hp; cases hp
  | cons q rest ih =>
    intro at_ P M hP p hp r hr
    simp only [piecesSpans]
    cases hp with
    | head =>
      rw [hr]
      refine ⟨_, List.mem_append_left _ (List.mem_singleton_self _), rfl, rfl, ?_⟩
      refine ⟨P, piecesText rest ++ M, ?_, hP, rfl⟩
      rw [pieces_text_cons]
      simp only [String.append_assoc]
    | tail _ hp =>
      obtain ⟨s, hs, h1, h2, h3⟩ :=
        ih (at_ + q.text.length) (P ++ q.text) M (by rw [String.length_append, hP]) p hp r hr
      refine ⟨s, List.mem_append_right _ hs, h1, h2, ?_⟩
      rw [pieces_text_cons]
      have e : P ++ (q.text ++ piecesText rest) ++ M = P ++ q.text ++ piecesText rest ++ M := by
        simp only [String.append_assoc]
      rw [e]
      exact h3

theorem pieces_shown : ∀ (ps : List Piece) (at_ : Nat) (P M : String), P.length = at_ →
    ∀ s ∈ piecesSpans at_ ps, ∃ p ∈ ps, p.role = some s.role ∧ p.actions = s.actions ∧
      Shows (P ++ piecesText ps ++ M) s p.text := by
  intro ps
  induction ps with
  | nil => intro _ _ _ _ s hs; cases hs
  | cons q rest ih =>
    intro at_ P M hP s hs
    simp only [piecesSpans] at hs
    rcases List.mem_append.mp hs with hs | hs
    · revert hs
      cases hq : q.role with
      | none => intro hs; cases hs
      | some r =>
        intro hs
        rw [List.mem_singleton] at hs
        subst hs
        refine ⟨q, List.mem_cons_self _ _, hq, rfl, P, piecesText rest ++ M, ?_, hP, rfl⟩
        rw [pieces_text_cons]
        simp only [String.append_assoc]
    · obtain ⟨p, hp, h1, h2, h3⟩ :=
        ih (at_ + q.text.length) (P ++ q.text) M (by rw [String.length_append, hP]) s hs
      refine ⟨p, List.mem_cons_of_mem _ hp, h1, h2, ?_⟩
      rw [pieces_text_cons]
      have e : P ++ (q.text ++ piecesText rest) ++ M = P ++ q.text ++ piecesText rest ++ M := by
        simp only [String.append_assoc]
      rw [e]
      exact h3

def joinSpans (gap : Nat) : Nat → List (List Piece) → List Span
  | _, [] => []
  | at_, row :: rest => piecesSpans at_ row ++ joinSpans gap (at_ + (piecesText row).length + gap) rest

def joined (sep : String) (rows : List (List Piece)) : String :=
  String.intercalate sep (rows.map piecesText)

theorem joined_cons_cons (sep : String) (row next : List Piece) (rest : List (List Piece)) :
    joined sep (row :: next :: rest) = piecesText row ++ sep ++ joined sep (next :: rest) := by
  simp only [joined, List.map_cons, intercalate_cons_cons]

theorem joined_one (sep : String) (row : List Piece) : joined sep [row] = piecesText row := by
  simp only [joined, List.map_cons, List.map_nil, intercalate_one]

theorem join_fits (sep : String) : ∀ (rows : List (List Piece)) (at_ : Nat),
    Fits at_ (joinSpans sep.length at_ rows) (at_ + (joined sep rows).length) := by
  intro rows
  induction rows with
  | nil => intro at_; simp [joinSpans, joined, String.intercalate, Fits]
  | cons row rest ih =>
    intro at_
    cases rest with
    | nil =>
      simp only [joinSpans, List.append_nil, joined_one]
      exact pieces_fits row at_
    | cons next rest =>
      simp only [joinSpans]
      rw [joined_cons_cons, String.length_append, String.length_append, ← Nat.add_assoc,
        ← Nat.add_assoc]
      exact fits_append (pieces_fits row at_) (fits_lower (Nat.le_add_right _ _) (ih _))

theorem join_show (sep : String) : ∀ (rows : List (List Piece)) (at_ : Nat) (P M : String),
    P.length = at_ → ∀ row ∈ rows, ∀ p ∈ row, ∀ r, p.role = some r →
    ∃ s ∈ joinSpans sep.length at_ rows, s.role = r ∧ s.actions = p.actions ∧
      Shows (P ++ joined sep rows ++ M) s p.text := by
  intro rows
  induction rows with
  | nil => intro _ _ _ _ row h; cases h
  | cons row rest ih =>
    intro at_ P M hP row' hrow p hp r hr
    cases rest with
    | nil =>
      simp only [joinSpans, List.append_nil, joined_one]
      rw [List.mem_singleton] at hrow
      subst hrow
      exact pieces_show row' at_ P M hP p hp r hr
    | cons next rest =>
      simp only [joinSpans]
      rw [joined_cons_cons]
      cases hrow with
      | head =>
        obtain ⟨s, hs, h1, h2, h3⟩ :=
          pieces_show row at_ P (sep ++ joined sep (next :: rest) ++ M) hP p hp r hr
        refine ⟨s, List.mem_append_left _ hs, h1, h2, ?_⟩
        have e : P ++ (piecesText row ++ sep ++ joined sep (next :: rest)) ++ M =
            P ++ piecesText row ++ (sep ++ joined sep (next :: rest) ++ M) := by
          simp only [String.append_assoc]
        rw [e]
        exact h3
      | tail _ hrow =>
        obtain ⟨s, hs, h1, h2, h3⟩ := ih (at_ + (piecesText row).length + sep.length)
          (P ++ piecesText row ++ sep) M (by simp only [String.length_append, hP]) row' hrow p hp r hr
        refine ⟨s, List.mem_append_right _ hs, h1, h2, ?_⟩
        have e : P ++ (piecesText row ++ sep ++ joined sep (next :: rest)) ++ M =
            P ++ piecesText row ++ sep ++ joined sep (next :: rest) ++ M := by
          simp only [String.append_assoc]
        rw [e]
        exact h3

theorem join_shown (sep : String) : ∀ (rows : List (List Piece)) (at_ : Nat) (P M : String),
    P.length = at_ → ∀ s ∈ joinSpans sep.length at_ rows, ∃ row ∈ rows, ∃ p ∈ row,
      p.role = some s.role ∧ p.actions = s.actions ∧ Shows (P ++ joined sep rows ++ M) s p.text := by
  intro rows
  induction rows with
  | nil => intro _ _ _ _ s h; cases h
  | cons row rest ih =>
    intro at_ P M hP s hs
    cases rest with
    | nil =>
      simp only [joinSpans, List.append_nil] at hs
      simp only [joined_one]
      obtain ⟨p, hp, h1, h2, h3⟩ := pieces_shown row at_ P M hP s hs
      exact ⟨row, List.mem_singleton_self _, p, hp, h1, h2, h3⟩
    | cons next rest =>
      simp only [joinSpans] at hs
      rw [joined_cons_cons]
      rcases List.mem_append.mp hs with hs | hs
      · obtain ⟨p, hp, h1, h2, h3⟩ :=
          pieces_shown row at_ P (sep ++ joined sep (next :: rest) ++ M) hP s hs
        refine ⟨row, List.mem_cons_self _ _, p, hp, h1, h2, ?_⟩
        have e : P ++ (piecesText row ++ sep ++ joined sep (next :: rest)) ++ M =
            P ++ piecesText row ++ (sep ++ joined sep (next :: rest) ++ M) := by
          simp only [String.append_assoc]
        rw [e]
        exact h3
      · obtain ⟨row', hrow, p, hp, h1, h2, h3⟩ := ih (at_ + (piecesText row).length + sep.length)
          (P ++ piecesText row ++ sep) M (by simp only [String.length_append, hP]) s hs
        refine ⟨row', List.mem_cons_of_mem _ hrow, p, hp, h1, h2, ?_⟩
        have e : P ++ (piecesText row ++ sep ++ joined sep (next :: rest)) ++ M =
            P ++ piecesText row ++ sep ++ joined sep (next :: rest) ++ M := by
          simp only [String.append_assoc]
        rw [e]
        exact h3

theorem joined_buffer (b : Buffer) (sep : String) (rows : List (List Piece))
    (ht : b.text = joined sep rows) (hs : b.spans = tidy b.text.length (joinSpans sep.length 0 rows)) :
    (∀ row ∈ rows, ∀ p ∈ row, ∀ r, p.role = some r → p.text ≠ "" →
      ∃ s ∈ b.spans, s.role = r ∧ s.actions = p.actions ∧ textUnder b s = p.text) ∧
    (∀ s ∈ b.spans, ∃ row ∈ rows, ∃ p ∈ row,
      p.role = some s.role ∧ p.actions = s.actions ∧ textUnder b s = p.text) := by
  have f : Fits 0 (joinSpans sep.length 0 rows) b.text.length := by
    have := join_fits sep rows 0
    rw [Nat.zero_add, ← ht] at this
    exact this
  rw [tidy_of_fits f] at hs
  have hT : b.text = "" ++ joined sep rows ++ "" := by rw [String.empty_append, String.append_empty, ht]
  constructor
  · intro row hrow p hp r hr hne
    obtain ⟨s, hs', h1, h2, h3⟩ := join_show sep rows 0 "" "" rfl row hrow p hp r hr
    refine ⟨s, ?_, h1, h2, textUnder_of_shows (by rw [hT]; exact h3)⟩
    rw [hs, List.mem_filter]
    obtain ⟨_, _, _, _, h4⟩ := h3
    refine ⟨hs', ?_⟩
    have := nonempty_length hne
    simp only [decide_eq_true_eq]
    omega
  · intro s hs'
    rw [hs, List.mem_filter] at hs'
    obtain ⟨row, hrow, p, hp, h1, h2, h3⟩ := join_shown sep rows 0 "" "" rfl s hs'.1
    exact ⟨row, hrow, p, hp, h1, h2, textUnder_of_shows (by rw [hT]; exact h3)⟩

theorem single_text (p : Piece) : piecesText [p] = p.text := by
  rw [pieces_text_cons]
  simp [piecesText, String.join]

theorem ne_empty_of_length {t : String} (h : 0 < t.length) : t ≠ "" := by
  intro e
  rw [e] at h
  exact Nat.lt_irrefl 0 h

theorem single_rows_buffer {α : Type} (b : Buffer) (sep : String) (xs : List α) (f : α → String)
    (act : α → List String) (r : Role)
    (ht : b.text = String.intercalate sep (xs.map f))
    (hs : b.spans = tidy b.text.length
      (joinSpans sep.length 0 (xs.map (fun x => [(⟨f x, some r, act x⟩ : Piece)])))) :
    (∀ x ∈ xs, f x ≠ "" → ∃ s ∈ b.spans, s.role = r ∧ s.actions = act x ∧ textUnder b s = f x) ∧
    (∀ s ∈ b.spans, ∃ x ∈ xs, s.role = r ∧ s.actions = act x ∧ textUnder b s = f x) := by
  have hj : String.intercalate sep (xs.map f) =
      joined sep (xs.map (fun x => [(⟨f x, some r, act x⟩ : Piece)])) := by
    simp only [joined, List.map_map]
    congr 1
  rw [hj] at ht
  have ⟨h1, h2⟩ := joined_buffer b sep _ ht hs
  constructor
  · intro x hx hne
    exact h1 _ (List.mem_map.mpr ⟨x, hx, rfl⟩) _ (List.mem_singleton_self _) r rfl hne
  · intro s hs'
    obtain ⟨row, hrow, p, hp, e1, e2, e3⟩ := h2 s hs'
    obtain ⟨x, hx, rfl⟩ := List.mem_map.mp hrow
    rw [List.mem_singleton] at hp
    subst hp
    exact ⟨x, hx, (Option.some.inj e1).symm, e2.symm, e3⟩

private theorem menuSpans_eq : ∀ (entries : List MenuEntry) (at_ : Nat),
    menuSpans at_ entries = joinSpans "\n".length at_
      (entries.map (fun e => [(⟨e.key ++ "  " ++ e.description, some Role.entry, e.action.toList⟩ : Piece)])) := by
  intro entries
  induction entries with
  | nil => intro at_; rfl
  | cons e rest ih =>
    intro at_
    simp only [menuSpans, List.map_cons, joinSpans, single_text, piecesSpans, ih]
    rfl

/-- @proves REQ-SHOW.menu_from_keymap -/
theorem a_menu_shows_each_key_with_its_action (title : String) (entries : List MenuEntry) :
    (menuBuffer title entries).text =
        String.intercalate "\n" (entries.map (fun e => e.key ++ "  " ++ e.description)) ∧
    (∀ e ∈ entries, ∃ s ∈ (menuBuffer title entries).spans, s.actions = e.action.toList ∧
      textUnder (menuBuffer title entries) s = e.key ++ "  " ++ e.description) ∧
    (∀ s ∈ (menuBuffer title entries).spans, ∃ e ∈ entries, s.actions = e.action.toList ∧
      textUnder (menuBuffer title entries) s = e.key ++ "  " ++ e.description) := by
  have ⟨h1, h2⟩ := single_rows_buffer (menuBuffer title entries) "\n" entries
    (fun e => e.key ++ "  " ++ e.description) (fun e => e.action.toList) Role.entry rfl
    (by show tidy _ (menuSpans 0 entries) = _; rw [menuSpans_eq]; rfl)
  refine ⟨rfl, ?_, ?_⟩
  · intro e he
    have hne : e.key ++ "  " ++ e.description ≠ "" := by
      apply ne_empty_of_length
      simp only [String.length_append]
      have : "  ".length = 2 := rfl
      omega
    obtain ⟨s, hs, _, e2, e3⟩ := h1 e he hne
    exact ⟨s, hs, e2, e3⟩
  · intro s hs
    obtain ⟨e, he, _, e2, e3⟩ := h2 s hs
    exact ⟨e, he, e2, e3⟩

private theorem eventSpans_eq : ∀ (events : List Event) (at_ : Nat),
    eventSpans at_ events = joinSpans "\n".length at_
      (events.map (fun e => [(⟨e.kind ++ ": " ++ e.text, some Role.entry, ["observe.diff"]⟩ : Piece)])) := by
  intro events
  induction events with
  | nil => intro at_; rfl
  | cons e rest ih =>
    intro at_
    simp only [eventSpans, List.map_cons, joinSpans, single_text, piecesSpans, ih]
    rfl

/-- @proves REQ-SHOW.record_from_events -/
theorem a_record_marks_each_event (title : String) (events : List Event) :
    (recordBuffer title events).text =
        String.intercalate "\n" (events.map (fun e => e.kind ++ ": " ++ e.text)) ∧
    (∀ e ∈ events, ∃ s ∈ (recordBuffer title events).spans, s.role = Role.entry ∧
      textUnder (recordBuffer title events) s = e.kind ++ ": " ++ e.text) ∧
    (∀ s ∈ (recordBuffer title events).spans, ∃ e ∈ events, s.role = Role.entry ∧
      textUnder (recordBuffer title events) s = e.kind ++ ": " ++ e.text) := by
  have ⟨h1, h2⟩ := single_rows_buffer (recordBuffer title events) "\n" events
    (fun e => e.kind ++ ": " ++ e.text) (fun _ => ["observe.diff"]) Role.entry rfl
    (by show tidy _ (eventSpans 0 events) = _; rw [eventSpans_eq]; rfl)
  refine ⟨rfl, ?_, ?_⟩
  · intro e he
    have hne : e.kind ++ ": " ++ e.text ≠ "" := by
      apply ne_empty_of_length
      simp only [String.length_append]
      have : ": ".length = 2 := rfl
      omega
    obtain ⟨s, hs, e1, _, e3⟩ := h1 e he hne
    exact ⟨s, hs, e1, e3⟩
  · intro s hs
    obtain ⟨e, he, e1, _, e3⟩ := h2 s hs
    exact ⟨e, he, e1, e3⟩

private theorem zip_map_self {α β : Type} (g : α → β) : ∀ xs : List α,
    xs.zip (xs.map g) = xs.map (fun x => (x, g x)) := by
  intro xs
  induction xs with
  | nil => rfl
  | cons x rest ih => simp only [List.map_cons, List.zip_cons_cons, ih]

private theorem pieceSpans_eq (g : MenuEntry → String) : ∀ (entries : List MenuEntry) (at_ : Nat),
    pieceSpans at_ (entries.map (fun e => (e, g e))) = joinSpans "  ".length at_
      (entries.map (fun e => [(⟨g e, some Role.entry, e.action.toList⟩ : Piece)])) := by
  intro entries
  induction entries with
  | nil => intro at_; rfl
  | cons e rest ih =>
    intro at_
    simp only [pieceSpans, List.map_cons, joinSpans, single_text, piecesSpans, ih]
    rfl

private theorem row_buffer (title : String) (entries : List MenuEntry) (width : Nat)
    (g : MenuEntry → String)
    (hp : (if (String.intercalate "  " (entries.map menuLine)).length ≤ width then entries.map menuLine
      else entries.map (·.key)) = entries.map g) :
    (menuRow title entries width).text = String.intercalate "  " (entries.map g) ∧
    (∀ e ∈ entries, g e ≠ "" → ∃ s ∈ (menuRow title entries width).spans,
      s.actions = e.action.toList ∧ textUnder (menuRow title entries width) s = g e) ∧
    (∀ s ∈ (menuRow title entries width).spans, ∃ e ∈ entries, s.actions = e.action.toList) := by
  have ht : (menuRow title entries width).text = String.intercalate "  " (entries.map g) := by
    show String.intercalate "  " _ = _
    rw [hp]
  have ⟨h1, h2⟩ := single_rows_buffer (menuRow title entries width) "  " entries g
    (fun e => e.action.toList) Role.entry ht
    (by
      show tidy _ (pieceSpans 0 (entries.zip _)) = _
      rw [hp, zip_map_self, pieceSpans_eq, ht])
  refine ⟨ht, ?_, ?_⟩
  · intro e he hne
    obtain ⟨s, hs, _, e2, e3⟩ := h1 e he hne
    exact ⟨s, hs, e2, e3⟩
  · intro s hs
    obtain ⟨e, he, _, e2, _⟩ := h2 s hs
    exact ⟨e, he, e2⟩

/-- @proves REQ-LOOK.row_fits -/
theorem a_row_fits_its_width (title : String) (entries : List MenuEntry) (width : Nat) :
    ((String.intercalate "  " (entries.map (fun e => e.key ++ "  " ++ e.description))).length ≤ width →
      (menuRow title entries width).text =
        String.intercalate "  " (entries.map (fun e => e.key ++ "  " ++ e.description)) ∧
      (menuRow title entries width).text.length ≤ width ∧
      ∀ e ∈ entries, ∃ s ∈ (menuRow title entries width).spans, s.actions = e.action.toList ∧
        textUnder (menuRow title entries width) s = e.key ++ "  " ++ e.description) ∧
    (¬ (String.intercalate "  " (entries.map (fun e => e.key ++ "  " ++ e.description))).length ≤ width →
      (menuRow title entries width).text = String.intercalate "  " (entries.map (·.key)) ∧
      ∀ e ∈ entries, e.key ≠ "" → ∃ s ∈ (menuRow title entries width).spans,
        s.actions = e.action.toList ∧ textUnder (menuRow title entries width) s = e.key) ∧
    (∀ s ∈ (menuRow title entries width).spans, ∃ e ∈ entries, s.actions = e.action.toList) := by
  have hm : entries.map menuLine = entries.map (fun e => e.key ++ "  " ++ e.description) := rfl
  by_cases h : (String.intercalate "  " (entries.map (fun e => e.key ++ "  " ++ e.description))).length ≤ width
  · have ⟨t, s1, s2⟩ := row_buffer title entries width (fun e => e.key ++ "  " ++ e.description)
      (by rw [hm, if_pos h])
    refine ⟨fun _ => ⟨t, by rw [t]; exact h, ?_⟩, fun n => absurd h n, s2⟩
    intro e he
    apply s1 e he
    apply ne_empty_of_length
    simp only [String.length_append]
    have : "  ".length = 2 := rfl
    omega
  · have ⟨t, s1, s2⟩ := row_buffer title entries width (·.key) (by rw [hm, if_neg h])
    exact ⟨fun y => absurd y h, fun _ => ⟨t, s1⟩, s2⟩

private theorem commonPrefix_spec : ∀ (xs ys : List String),
    xs.take (commonPrefix xs ys) = ys.take (commonPrefix xs ys) ∧
    commonPrefix xs ys ≤ xs.length ∧ commonPrefix xs ys ≤ ys.length := by
  intro xs
  induction xs with
  | nil => intro ys; simp [commonPrefix]
  | cons a as ih =>
    intro ys
    cases ys with
    | nil => simp [commonPrefix]
    | cons b bs =>
      simp only [commonPrefix]
      split
      · rename_i h
        have hab : a = b := (beq_iff_eq _ _).mp h
        subst hab
        have ⟨e, l1, l2⟩ := ih bs
        rw [Nat.add_comm 1]
        simp only [List.take_succ_cons, List.length_cons, e]
        exact ⟨trivial, Nat.succ_le_succ l1, Nat.succ_le_succ l2⟩
      · simp

private theorem commonPrefix_self : ∀ (xs : List String), commonPrefix xs xs = xs.length := by
  intro xs
  induction xs with
  | nil => rfl
  | cons a as ih => simp [commonPrefix, ih, Nat.add_comm]

/-- @proves REQ-SHOW.review_from_change -/
theorem a_review_marks_what_changed (before after : List String) :
    ∃ same removed added kept : List String,
      before = same ++ removed ++ kept ∧ after = same ++ added ++ kept ∧
      diffLines before after =
        same.map (fun l => (⟨Role.plain, " " ++ l⟩ : DiffLine)) ++
        removed.map (fun l => (⟨Role.removed, "-" ++ l⟩ : DiffLine)) ++
        added.map (fun l => (⟨Role.added, "+" ++ l⟩ : DiffLine)) ++
        kept.map (fun l => (⟨Role.plain, " " ++ l⟩ : DiffLine)) := by
  have ⟨ef, _, _⟩ := commonPrefix_spec before after
  generalize hf : commonPrefix before after = front at ef
  have ⟨eb, lb1, lb2⟩ := commonPrefix_spec (before.drop front).reverse (after.drop front).reverse
  generalize hb : commonPrefix (before.drop front).reverse (after.drop front).reverse = back at eb lb1 lb2
  rw [List.take_reverse, List.take_reverse] at eb
  have eb' := congrArg List.reverse eb
  rw [List.reverse_reverse, List.reverse_reverse] at eb'
  rw [List.length_reverse] at lb1 lb2
  refine ⟨before.take front, (before.drop front).take ((before.drop front).length - back),
    (after.drop front).take ((after.drop front).length - back),
    (before.drop front).drop ((before.drop front).length - back), ?_, ?_, ?_⟩
  · rw [List.append_assoc, List.take_append_drop, List.take_append_drop]
  · rw [List.append_assoc, eb', List.take_append_drop, ef, List.take_append_drop]
  · simp only [diffLines, hf, hb, List.append_assoc]
    rfl

theorem an_unchanged_state_marks_nothing (lines : List String) :
    diffLines lines lines = lines.map (fun l => (⟨Role.plain, " " ++ l⟩ : DiffLine)) := by
  simp [diffLines, commonPrefix_self, marked, commonPrefix]

/-- @proves REQ-SHOW.producer_is_pure -/
theorem a_review_reads_only_what_it_is_given (target other before after : String) :
    (reviewBuffer target before after).text = (reviewBuffer other before after).text ∧
    (reviewBuffer target before after).spans = (reviewBuffer other before after).spans ∧
    (reviewBuffer target before after).id = "review:" ++ target ∧
    (reviewBuffer target before after).kind = BufferKind.review target ∧
    faults (reviewBuffer target before after) = [] :=
  ⟨rfl, rfl, rfl, rfl, tidy_leaves_no_fault _ _ _ _⟩

/-- @proves REQ-SHOW.producers_are_total -/
theorem every_kind_has_a_producer_that_answers :
    (∀ kind : BufferKind, ∃ b : Buffer, b.kind = kind ∧ faults b = []) ∧
    (∀ path text marks, (fileBuffer path text marks).kind = BufferKind.file path ∧
      faults (fileBuffer path text marks) = []) ∧
    (∀ path entries, (directoryBuffer path entries).kind = BufferKind.directory path) ∧
    (∀ target before after, (reviewBuffer target before after).kind = BufferKind.review target ∧
      faults (reviewBuffer target before after) = []) ∧
    (∀ title entries, (menuBuffer title entries).kind = BufferKind.menu title ∧
      faults (menuBuffer title entries) = []) ∧
    (∀ title events, (recordBuffer title events).kind = BufferKind.record title ∧
      faults (recordBuffer title events) = []) ∧
    (∀ size spans, Fits 0 (tidy size spans) size ∧ ∀ s ∈ tidy size spans, s.start < s.stop) := by
  refine ⟨?_, fun _ _ _ => ⟨rfl, tidy_leaves_no_fault _ _ _ _⟩, fun _ _ => rfl,
    fun _ _ _ => ⟨rfl, tidy_leaves_no_fault _ _ _ _⟩, fun _ _ => ⟨rfl, tidy_leaves_no_fault _ _ _ _⟩,
    fun _ _ => ⟨rfl, tidy_leaves_no_fault _ _ _ _⟩, tidy_strict⟩
  intro kind
  cases kind with
  | file path => exact ⟨fileBuffer path "" [], rfl, tidy_leaves_no_fault _ _ _ _⟩
  | directory path =>
    exact ⟨directoryBuffer path [], rfl, no_fault_when_fits _ (Nat.le_refl _) (fun s h => by cases h)⟩
  | review target => exact ⟨reviewBuffer target "" "", rfl, tidy_leaves_no_fault _ _ _ _⟩
  | menu title => exact ⟨menuBuffer title [], rfl, tidy_leaves_no_fault _ _ _ _⟩
  | record title => exact ⟨recordBuffer title [], rfl, tidy_leaves_no_fault _ _ _ _⟩

private theorem mem_clipped : ∀ (l : List Span) (acc : List Span) (shift size : Nat) (s : Span),
    s ∈ l.foldl (fun acc span => acc ++ clipSpan shift size span) acc →
    s ∈ acc ∨ ∃ s' ∈ l, s.role = s'.role ∧ s.actions = s'.actions := by
  intro l
  induction l with
  | nil => intro acc _ _ s h; exact Or.inl h
  | cons a rest ih =>
    intro acc shift size s h
    rcases ih _ shift size s h with h | ⟨s', hs', e⟩
    · rcases List.mem_append.mp h with h | h
      · exact Or.inl h
      · right
        refine ⟨a, List.mem_cons_self _ _, ?_⟩
        unfold clipSpan at h
        split at h
        · cases h
        · split at h
          · cases h
          · rw [List.mem_singleton] at h
            subst h
            exact ⟨rfl, rfl⟩
    · exact Or.inr ⟨s', List.mem_cons_of_mem _ hs', e⟩

/-- @proves REQ-SHOW.window_is_a_buffer -/
theorem a_window_is_a_buffer (buffer : Buffer) (start count : Nat) :
    faults (window buffer start count) = [] ∧
    (window buffer start count).id = buffer.id ∧ (window buffer start count).kind = buffer.kind ∧
    (window buffer start count).text =
      String.intercalate "\n" (((buffer.text.splitOn "\n").drop start).take count) ∧
    ∀ s ∈ (window buffer start count).spans, ∃ s' ∈ buffer.spans,
      s.role = s'.role ∧ s.actions = s'.actions := by
  refine ⟨tidy_leaves_no_fault _ _ _ _, rfl, rfl, rfl, ?_⟩
  intro s hs
  obtain ⟨s1, hs1, e1, e2, _, _⟩ := mem_tidy hs
  rcases mem_clipped _ [] _ _ s1 hs1 with h | ⟨s', hs', f1, f2⟩
  · cases h
  · exact ⟨s', hs', e1.trans f1, e2.trans f2⟩

/-- @proves REQ-SHOW.sandbox_from_observation -/
theorem a_sandbox_observation_in_order (changed : List String) (said : List Event)
    (spend : TraceLean.Cost.Spend) :
    (sandboxEvents changed said spend).length = changed.length + said.length + 1 ∧
    ((sandboxEvents changed said spend).take changed.length).map (·.text) = changed ∧
    (∀ e ∈ (sandboxEvents changed said spend).take changed.length, e.kind = "changed") ∧
    ((sandboxEvents changed said spend).drop changed.length).take said.length = said ∧
    (sandboxEvents changed said spend).getLast? =
      some { kind := "cost", text := TraceLean.Cost.estimateLine spend } := by
  have hl : (changed.map (fun path => ({ kind := "changed", text := path } : Event))).length =
      changed.length := List.length_map _ _
  refine ⟨?_, ?_, ?_, ?_, ?_⟩
  · simp [sandboxEvents]
    omega
  · simp only [sandboxEvents, List.append_assoc]
    rw [← hl, List.take_left, List.map_map]
    exact List.map_id _
  · intro e he
    simp only [sandboxEvents, List.append_assoc] at he
    rw [← hl, List.take_left, List.mem_map] at he
    obtain ⟨_, _, rfl⟩ := he
    rfl
  · simp only [sandboxEvents, List.append_assoc]
    rw [← hl, List.drop_left, List.take_left]
  · simp [sandboxEvents, List.getLast?_append]

def Marked (b : Buffer) (r : Role) (said : String) : Prop :=
  ∃ s ∈ b.spans, s.role = r ∧ textUnder b s = said

def Offers (b : Buffer) (r : Role) (actions : List String) (said : String) : Prop :=
  ∃ s ∈ b.spans, s.role = r ∧ s.actions = actions ∧ textUnder b s = said

theorem lit_ne {t : String} (n : Nat) (h : t.length = n + 1) : t ≠ "" :=
  ne_empty_of_length (by rw [h]; exact Nat.succ_pos n)

theorem mem_intersperse {α : Type} (sep : α) : ∀ (l : List α) (x : α), x ∈ l → x ∈ l.intersperse sep := by
  intro l
  induction l with
  | nil => intro x h; cases h
  | cons a rest ih =>
    intro x h
    cases rest with
    | nil => exact h
    | cons b r =>
      simp only [List.intersperse]
      cases h with
      | head => exact List.mem_cons_self _ _
      | tail _ h => exact List.mem_cons_of_mem _ (List.mem_cons_of_mem _ (ih x h))

theorem mem_intersperse_inv {α : Type} (sep : α) : ∀ (l : List α) (x : α),
    x ∈ l.intersperse sep → x ∈ l ∨ x = sep := by
  intro l
  induction l with
  | nil => intro x h; cases h
  | cons a rest ih =>
    intro x h
    cases rest with
    | nil => exact Or.inl h
    | cons b r =>
      simp only [List.intersperse] at h
      rcases h with _ | ⟨_, h⟩
      · exact Or.inl (List.mem_cons_self _ _)
      · rcases h with _ | ⟨_, h⟩
        · exact Or.inr rfl
        · rcases ih x h with h | h
          · exact Or.inl (List.mem_cons_of_mem _ h)
          · exact Or.inr h

private theorem lines_eq : ∀ (rows : List (List Piece)) (at_ : Nat),
    linesSpans at_ rows = joinSpans "\n".length at_ rows := by
  intro rows
  induction rows with
  | nil => intro at_; rfl
  | cons row rest ih =>
    intro at_
    simp only [linesSpans, joinSpans, ih]
    rfl

private theorem count_spanned (letter : String) (held of_ : Nat) (role : Role) (h : of_ ≠ 0) :
    countPiece letter held of_ role = spanned (letter ++ toString held ++ "/" ++ toString of_) role := by
  simp [countPiece, h]

private theorem count_role (letter : String) (held of_ : Nat) (role : Role) (h : role ≠ Role.requirement) :
    (countPiece letter held of_ role).role ≠ some Role.requirement := by
  unfold countPiece
  split
  · simp [bare]
  · simp [spanned, h]

private theorem only_id_is_requirement (nodes : List Node) (node : Node) (p : Piece)
    (hp : p ∈ rowPieces nodes node) (hr : p.role = some Role.requirement) : p.text = node.id := by
  unfold rowPieces at hp
  simp only [List.mem_append, List.mem_cons, List.mem_singleton, List.mem_map] at hp
  rcases hp with ((((((⟨e, _, rfl⟩ | h) | h) | h) | h) | h) | h)
  · unfold markPiece at hr
    split at hr <;> simp [spanned, bare] at hr
  · rcases h with rfl | rfl | rfl | h
    · simp [bare] at hr
    · rfl
    · simp [bare] at hr
    · cases h
  · rcases mem_intersperse_inv _ _ _ h with h | rfl
    · simp only [List.mem_cons, List.mem_singleton] at h
      rcases h with rfl | rfl | rfl | rfl | rfl | h
      · exact absurd hr (count_role _ _ _ _ (by simp))
      · exact absurd hr (count_role _ _ _ _ (by simp))
      · exact absurd hr (count_role _ _ _ _ (by simp))
      · exact absurd hr (count_role _ _ _ _ (by simp))
      · exact absurd hr (count_role _ _ _ _ (by simp))
      · cases h
    · simp [bare] at hr
  · rcases h with rfl | h
    · simp [bare] at hr
    · cases h
  · unfold linesPieces at h
    split at h
    · cases h
    · simp only [List.mem_cons, List.mem_singleton] at h
      rcases h with rfl | rfl | h
      · split at hr
        · simp [bare] at hr
        · split at hr <;> simp at hr
      · simp [bare] at hr
      · cases h
  · split at h
    · simp only [List.mem_cons, List.mem_singleton] at h
      rcases h with rfl | rfl | h
      · simp at hr
      · simp [bare] at hr
      · cases h
    · cases h
  · rcases h with rfl | h
    · simp [bare] at hr
    · cases h

private theorem requirements_buffer (nodes : List Node) (openOnly : Bool) :
    (∀ row ∈ (nodes.filter (fun node => !openOnly || isOpen node)).map (rowPieces nodes), ∀ p ∈ row, ∀ r,
      p.role = some r → p.text ≠ "" →
      ∃ s ∈ (requirementsBuffer nodes openOnly).spans, s.role = r ∧ s.actions = p.actions ∧
        textUnder (requirementsBuffer nodes openOnly) s = p.text) ∧
    (∀ s ∈ (requirementsBuffer nodes openOnly).spans,
      ∃ row ∈ (nodes.filter (fun node => !openOnly || isOpen node)).map (rowPieces nodes), ∃ p ∈ row,
        p.role = some s.role ∧ p.actions = s.actions ∧
        textUnder (requirementsBuffer nodes openOnly) s = p.text) :=
  joined_buffer _ "\n" _ rfl (by show tidy _ (linesSpans 0 _) = _; rw [lines_eq]; rfl)

private theorem row_marked (nodes : List Node) (openOnly : Bool) (node : Node) (h : node ∈ nodes)
    (hk : (!openOnly || isOpen node) = true) (p : Piece) (hp : p ∈ rowPieces nodes node) (r : Role)
    (hr : p.role = some r) (hne : p.text ≠ "") : Marked (requirementsBuffer nodes openOnly) r p.text := by
  obtain ⟨s, hs, e1, _, e3⟩ := (requirements_buffer nodes openOnly).1 _
    (List.mem_map.mpr ⟨node, List.mem_filter.mpr ⟨h, hk⟩, rfl⟩) p hp r hr hne
  exact ⟨s, hs, e1, e3⟩

private theorem requirement_spans (nodes : List Node) (openOnly : Bool) (s : Span)
    (hs : s ∈ (requirementsBuffer nodes openOnly).spans) (hr : s.role = Role.requirement) :
    ∃ n ∈ nodes, (!openOnly || isOpen n) = true ∧ textUnder (requirementsBuffer nodes openOnly) s = n.id := by
  obtain ⟨row, hrow, p, hp, e1, _, e3⟩ := (requirements_buffer nodes openOnly).2 s hs
  obtain ⟨n, hn, rfl⟩ := List.mem_map.mp hrow
  rw [List.mem_filter] at hn
  rw [hr] at e1
  exact ⟨n, hn.1, hn.2, by rw [e3, only_id_is_requirement nodes n p hp e1]⟩

private theorem count_in_row (nodes : List Node) (node : Node) (q : Piece)
    (hq : q ∈ [countPiece "I" node.held.implemented node.modelled (Role.claim TraceLean.Annotation.Role.implements),
      countPiece "A" node.held.annotated node.clauses (Role.mark .annotated true),
      countPiece "J" node.held.judged node.clauses (Role.mark .judged true),
      countPiece "T" node.held.tested node.modelled (Role.mark .tested true),
      countPiece "P" node.held.proved node.modelled (Role.mark .proved true)]) :
    q ∈ rowPieces nodes node := by
  unfold rowPieces
  simp only [List.mem_append]
  exact Or.inl (Or.inl (Or.inl (Or.inl (Or.inr (mem_intersperse _ _ _ hq)))))

private theorem count_text_ne (letter : String) (held of_ : Nat) (h : letter ≠ "") :
    letter ++ toString held ++ "/" ++ toString of_ ≠ "" := by
  apply ne_empty_of_length
  have := nonempty_length h
  simp only [String.length_append]
  omega

/-- @proves REQ-SHOW.index_from_requirements -/
theorem an_index_marks_each_requirement (nodes : List Node) (node : Node) (h : node ∈ nodes) :
    (∃ rows : List String, rows.length = nodes.length ∧
      (requirementsBuffer nodes false).text = String.intercalate "\n" rows) ∧
    (node.id ≠ "" → Marked (requirementsBuffer nodes false) Role.requirement node.id) ∧
    (∀ e ∈ node.marks.each, node.marks.applies e.1 = true →
      Marked (requirementsBuffer nodes false) (Role.mark e.1 e.2.1)
        (String.singleton (TraceLean.Evidence.letter e.2.1 e.2.2))) ∧
    (node.modelled ≠ 0 → Marked (requirementsBuffer nodes false)
      (Role.claim TraceLean.Annotation.Role.implements)
      ("I" ++ toString node.held.implemented ++ "/" ++ toString node.modelled)) ∧
    (node.clauses ≠ 0 → Marked (requirementsBuffer nodes false) (Role.mark .annotated true)
      ("A" ++ toString node.held.annotated ++ "/" ++ toString node.clauses)) ∧
    (node.clauses ≠ 0 → Marked (requirementsBuffer nodes false) (Role.mark .judged true)
      ("J" ++ toString node.held.judged ++ "/" ++ toString node.clauses)) ∧
    (node.modelled ≠ 0 → Marked (requirementsBuffer nodes false) (Role.mark .tested true)
      ("T" ++ toString node.held.tested ++ "/" ++ toString node.modelled)) ∧
    (node.modelled ≠ 0 → Marked (requirementsBuffer nodes false) (Role.mark .proved true)
      ("P" ++ toString node.held.proved ++ "/" ++ toString node.modelled)) ∧
    (∀ s ∈ (requirementsBuffer nodes false).spans, s.role = Role.requirement →
      ∃ n ∈ nodes, textUnder (requirementsBuffer nodes false) s = n.id) := by
  have hk : (!false || isOpen node) = true := rfl
  have counted : ∀ (letter : String) (held of_ : Nat) (role : Role), of_ ≠ 0 → letter ≠ "" →
      countPiece letter held of_ role ∈ rowPieces nodes node →
      Marked (requirementsBuffer nodes false) role (letter ++ toString held ++ "/" ++ toString of_) := by
    intro letter held of_ role hne hl hq
    rw [count_spanned _ _ _ _ hne] at hq
    exact row_marked nodes false node h hk _ hq _ rfl (count_text_ne _ _ _ hl)
  have hf : nodes.filter (fun node => !false || isOpen node) = nodes := by
    apply List.filter_eq_self.mpr
    intro _ _
    rfl
  refine ⟨?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_, ?_⟩
  · refine ⟨(nodes.map (rowPieces nodes)).map piecesText, by simp, ?_⟩
    have e : (requirementsBuffer nodes false).text = String.intercalate "\n"
        (((nodes.filter (fun node => !false || isOpen node)).map (rowPieces nodes)).map piecesText) := rfl
    rw [e, hf]
  · intro hne
    have hp : spanned node.id Role.requirement ∈ rowPieces nodes node := by
      unfold rowPieces
      apply List.mem_append_left; apply List.mem_append_left; apply List.mem_append_left
      apply List.mem_append_left; apply List.mem_append_left; apply List.mem_append_right
      exact List.mem_cons_of_mem _ (List.mem_cons_self _ _)
    exact row_marked nodes false node h hk _ hp _ rfl hne
  · intro e he ha
    have hp : markPiece node.marks e ∈ rowPieces nodes node := by
      unfold rowPieces
      apply List.mem_append_left; apply List.mem_append_left; apply List.mem_append_left
      apply List.mem_append_left; apply List.mem_append_left; apply List.mem_append_left
      exact List.mem_map.mpr ⟨e, he, rfl⟩
    have hm : markPiece node.marks e = spanned (String.singleton (TraceLean.Evidence.letter e.2.1 e.2.2))
        (Role.mark e.1 e.2.1) := by
      unfold markPiece
      rw [if_pos ha]
    rw [hm] at hp
    exact row_marked nodes false node h hk _ hp _ rfl
      (ne_empty_of_length (show 0 < (String.singleton (TraceLean.Evidence.letter e.2.1 e.2.2)).length from
        Nat.zero_lt_one))
  · intro hne
    exact counted "I" _ _ _ hne (lit_ne 0 rfl) (count_in_row nodes node _ (List.mem_cons_self _ _))
  · intro hne
    exact counted "A" _ _ _ hne (lit_ne 0 rfl)
      (count_in_row nodes node _ (List.mem_cons_of_mem _ (List.mem_cons_self _ _)))
  · intro hne
    exact counted "J" _ _ _ hne (lit_ne 0 rfl)
      (count_in_row nodes node _ (List.mem_cons_of_mem _ (List.mem_cons_of_mem _ (List.mem_cons_self _ _))))
  · intro hne
    exact counted "T" _ _ _ hne (lit_ne 0 rfl)
      (count_in_row nodes node _ (List.mem_cons_of_mem _ (List.mem_cons_of_mem _
        (List.mem_cons_of_mem _ (List.mem_cons_self _ _)))))
  · intro hne
    exact counted "P" _ _ _ hne (lit_ne 0 rfl)
      (count_in_row nodes node _ (List.mem_cons_of_mem _ (List.mem_cons_of_mem _
        (List.mem_cons_of_mem _ (List.mem_cons_of_mem _ (List.mem_cons_self _ _))))))
  · intro s hs hr
    obtain ⟨n, hn, _, e⟩ := requirement_spans nodes false s hs hr
    exact ⟨n, hn, e⟩

/-- @proves REQ-SHOW.children_lines -/
theorem a_row_says_what_its_children_ran (nodes : List Node) (openOnly : Bool) (node : Node)
    (h : node ∈ nodes) (hk : (!openOnly || isOpen node) = true) (ran all : Nat)
    (hsum : (childrenOf nodes node.id).foldl
      (fun acc child => (acc.1 + (linesUnder nodes nodes.length child).1,
                         acc.2 + (linesUnder nodes nodes.length child).2)) (0, 0) = (ran, all))
    (hunrun : ran < all) :
    Marked (requirementsBuffer nodes openOnly) Role.removed
      ("+children " ++ toString (((node.covered.getD (0, 0)).1 + ran) * 100 /
        ((node.covered.getD (0, 0)).2 + all)) ++ "%") := by
  have hc : childrenShort nodes node = some ("+children " ++ toString (((node.covered.getD (0, 0)).1 + ran) * 100 /
        ((node.covered.getD (0, 0)).2 + all)) ++ "%") := by
    unfold childrenShort
    simp only []
    rw [hsum]
    simp [hunrun]
  apply row_marked nodes openOnly node h hk
    ⟨"+children " ++ toString (((node.covered.getD (0, 0)).1 + ran) * 100 /
        ((node.covered.getD (0, 0)).2 + all)) ++ "%", some Role.removed, []⟩ _ _ rfl
  · apply ne_empty_of_length
    simp only [String.length_append]
    have : "+children ".length = 10 := rfl
    omega
  · unfold rowPieces
    rw [hc]
    simp

def DescendsWithin (nodes : List Node) : Nat → Node → Node → Prop
  | 0, n, c => n = c
  | k + 1, n, c => n = c ∨ ∃ m ∈ nodes, m.refines.contains n.id = true ∧ DescendsWithin nodes k m c

private theorem openUnder_iff (nodes : List Node) : ∀ (f : Nat) (n : Node),
    openUnder nodes f n = true ↔ ∃ c, isOpen c = true ∧ DescendsWithin nodes f n c := by
  intro f
  induction f with
  | zero =>
    intro n
    simp only [openUnder, DescendsWithin]
    exact ⟨fun h => ⟨n, h, rfl⟩, fun ⟨_, hc, e⟩ => e ▸ hc⟩
  | succ f ih =>
    intro n
    simp only [openUnder, DescendsWithin, Bool.or_eq_true, List.any_eq_true, childrenOf, List.mem_filter]
    constructor
    · rintro (h | ⟨m, ⟨hm, hc⟩, ho⟩)
      · exact ⟨n, h, Or.inl rfl⟩
      · obtain ⟨c, hc', d⟩ := (ih m).mp ho
        exact ⟨c, hc', Or.inr ⟨m, hm, hc, d⟩⟩
    · rintro ⟨c, hc, rfl | ⟨m, hm, hr, d⟩⟩
      · exact Or.inl hc
      · exact Or.inr ⟨m, ⟨hm, hr⟩, (ih m).mpr ⟨c, hc, d⟩⟩

private theorem expand_head (nodes : List Node) (opened : List String) (f indent : Nat) (n : Node) :
    ({ indent := indent, node := n } : Row) ∈ expand f nodes opened indent n := by
  cases f with
  | zero => rw [expand]; exact List.mem_singleton_self _
  | succ f => rw [expand]; exact List.mem_cons_self _ _

private theorem mem_expandAll (nodes : List Node) (opened : List String) (f indent : Nat) (r : Row) :
    ∀ (cs : List Node), r ∈ expandAll f nodes opened indent cs ↔ ∃ c ∈ cs, r ∈ expand f nodes opened indent c := by
  intro cs
  induction cs with
  | nil => rw [expandAll]; simp
  | cons c rest ih =>
    rw [expandAll, List.mem_append, ih]
    simp

private theorem mem_expand_succ (nodes : List Node) (opened : List String) (f indent : Nat) (n : Node)
    (r : Row) : r ∈ expand (f + 1) nodes opened indent n ↔
      r = { indent := indent, node := n } ∨
      (opened.contains n.id = true ∧ ∃ c ∈ childrenOf nodes n.id, r ∈ expand f nodes opened (indent + 1) c) := by
  rw [expand, List.mem_cons, ← mem_expandAll]
  constructor
  · rintro (h | h)
    · exact Or.inl h
    · split at h
      · exact Or.inr ⟨by assumption, h⟩
      · cases h
  · rintro (h | ⟨ho, h⟩)
    · exact Or.inl h
    · right
      rw [if_pos ho]
      exact h

private theorem expand_rows (nodes : List Node) (opened : List String) : ∀ (f indent : Nat) (n : Node) (r : Row),
    r ∈ expand f nodes opened indent n →
    (r.indent = indent ∧ r.node = n) ∨
    (indent < r.indent ∧ r.node ∈ nodes ∧ ∃ m : Node,
      ({ indent := r.indent - 1, node := m } : Row) ∈ expand f nodes opened indent n ∧
      opened.contains m.id = true ∧ r.node.refines.contains m.id = true) := by
  intro f
  induction f with
  | zero =>
    intro indent n r h
    rw [expand, List.mem_singleton] at h
    subst h
    exact Or.inl ⟨rfl, rfl⟩
  | succ f ih =>
    intro indent n r h
    rcases (mem_expand_succ nodes opened f indent n r).mp h with rfl | ⟨ho, c, hc, hr⟩
    · exact Or.inl ⟨rfl, rfl⟩
    · right
      have hc' := List.mem_filter.mp hc
      rcases ih (indent + 1) c r hr with ⟨e1, e2⟩ | ⟨l, hn, m, hm, o, rr⟩
      · refine ⟨by omega, e2 ▸ hc'.1, n, ?_, ho, e2 ▸ hc'.2⟩
        rw [e1, Nat.add_sub_cancel]
        exact expand_head _ _ _ _ _
      · refine ⟨by omega, hn, m, ?_, o, rr⟩
        exact (mem_expand_succ nodes opened f indent n _).mpr (Or.inr ⟨ho, c, hc, hm⟩)

private theorem expand_under (nodes : List Node) (opened : List String) : ∀ (f indent : Nat) (n : Node) (r : Row),
    r ∈ expand f nodes opened indent n → r.indent < indent + f → opened.contains r.node.id = true →
    ∀ c ∈ childrenOf nodes r.node.id,
      ({ indent := r.indent + 1, node := c } : Row) ∈ expand f nodes opened indent n := by
  intro f
  induction f with
  | zero =>
    intro indent n r h hl
    rw [expand, List.mem_singleton] at h
    subst h
    exact absurd hl (by simp)
  | succ f ih =>
    intro indent n r h hl ho c hc
    rcases (mem_expand_succ nodes opened f indent n r).mp h with rfl | ⟨ho', c', hc', hr⟩
    · exact (mem_expand_succ nodes opened f indent n _).mpr (Or.inr ⟨ho, c, hc, expand_head _ _ _ _ _⟩)
    · exact (mem_expand_succ nodes opened f indent n _).mpr
        (Or.inr ⟨ho', c', hc', ih (indent + 1) c' r hr (by omega) ho c hc⟩)

private theorem mem_designRows (nodes : List Node) (opened : List String) (r : Row) :
    r ∈ designRows nodes opened ↔
      ∃ root ∈ nodes, root.refines.isEmpty = true ∧ r ∈ expand nodes.length nodes opened 0 root := by
  unfold designRows
  rw [mem_expandAll]
  simp [List.mem_filter, and_assoc]

private theorem design_buffer (nodes : List Node) (opened : List String) (openOnly : Bool) :
    (∀ row ∈ headerPieces openOnly :: (designRows (shown nodes openOnly) opened).map
        (designPieces (shown nodes openOnly) nodes opened), ∀ p ∈ row, ∀ r,
      p.role = some r → p.text ≠ "" →
      ∃ s ∈ (designBuffer nodes opened openOnly).spans, s.role = r ∧ s.actions = p.actions ∧
        textUnder (designBuffer nodes opened openOnly) s = p.text) ∧
    (∀ s ∈ (designBuffer nodes opened openOnly).spans,
      ∃ row ∈ headerPieces openOnly :: (designRows (shown nodes openOnly) opened).map
        (designPieces (shown nodes openOnly) nodes opened), ∃ p ∈ row,
        p.role = some s.role ∧ p.actions = s.actions ∧
        textUnder (designBuffer nodes opened openOnly) s = p.text) :=
  joined_buffer _ "\n" _ rfl (by show tidy _ (linesSpans 0 _) = _; rw [lines_eq]; rfl)

private theorem design_requirements (nodes : List Node) (opened : List String) (openOnly : Bool) (s : Span)
    (hs : s ∈ (designBuffer nodes opened openOnly).spans) (hr : s.role = Role.requirement) :
    ∃ r ∈ designRows (shown nodes openOnly) opened,
      textUnder (designBuffer nodes opened openOnly) s = r.node.id := by
  obtain ⟨row, hrow, p, hp, e1, _, e3⟩ := (design_buffer nodes opened openOnly).2 s hs
  rw [hr] at e1
  rcases List.mem_cons.mp hrow with rfl | hrow
  · unfold headerPieces at hp
    rcases mem_intersperse_inv _ _ _ hp with hp | rfl
    · simp only [designHeader, List.map, List.mem_cons, List.mem_singleton] at hp
      rcases hp with rfl | rfl | rfl | h <;> first | (simp at e1) | cases h
    · simp [bare] at e1
  · obtain ⟨r, hr', rfl⟩ := List.mem_map.mp hrow
    refine ⟨r, hr', ?_⟩
    rw [e3]
    unfold designPieces at hp
    simp only [List.mem_append, List.mem_cons, List.mem_singleton] at hp
    rcases hp with (rfl | rfl | rfl | h) | h
    · simp [bare] at e1
    · split at e1 <;> simp [bare] at e1
    · simp [bare] at e1
    · cases h
    · exact only_id_is_requirement nodes r.node p h e1

/-- @proves REQ-SHOW.graph_from_refinement -/
theorem a_graph_unfolds_where_opened (nodes : List Node) (opened : List String) (openOnly : Bool) :
    (∀ n ∈ shown nodes openOnly, n.refines = [] →
      ({ indent := 0, node := n } : Row) ∈ designRows (shown nodes openOnly) opened) ∧
    (∀ r ∈ designRows (shown nodes openOnly) opened, r.node ∈ shown nodes openOnly ∧
      ((r.indent = 0 ∧ r.node.refines = []) ∨
       (0 < r.indent ∧ ∃ m : Node, ({ indent := r.indent - 1, node := m } : Row) ∈ designRows (shown nodes openOnly) opened ∧
         opened.contains m.id = true ∧ r.node.refines.contains m.id = true))) ∧
    (∀ r ∈ designRows (shown nodes openOnly) opened, r.indent < (shown nodes openOnly).length →
      opened.contains r.node.id = true → ∀ c ∈ shown nodes openOnly, c.refines.contains r.node.id = true →
      ({ indent := r.indent + 1, node := c } : Row) ∈ designRows (shown nodes openOnly) opened) ∧
    (∀ r ∈ designRows (shown nodes openOnly) opened, r.node.id ≠ "" →
      Marked (designBuffer nodes opened openOnly) Role.requirement r.node.id) ∧
    (∀ s ∈ (designBuffer nodes opened openOnly).spans, s.role = Role.requirement →
      ∃ r ∈ designRows (shown nodes openOnly) opened,
        textUnder (designBuffer nodes opened openOnly) s = r.node.id) := by
  refine ⟨?_, ?_, ?_, ?_, design_requirements nodes opened openOnly⟩
  · intro n hn he
    exact (mem_designRows _ _ _).mpr ⟨n, hn, by rw [he]; rfl, expand_head _ _ _ _ _⟩
  · intro r hr
    obtain ⟨root, hroot, hempty, hx⟩ := (mem_designRows _ _ _).mp hr
    rcases expand_rows _ opened _ 0 root r hx with ⟨e1, e2⟩ | ⟨l, hn, m, hm, o, rr⟩
    · exact ⟨e2 ▸ hroot, Or.inl ⟨e1, e2 ▸ List.isEmpty_iff.mp hempty⟩⟩
    · exact ⟨hn, Or.inr ⟨l, m, (mem_designRows _ _ _).mpr ⟨root, hroot, hempty, hm⟩, o, rr⟩⟩
  · intro r hr hl ho c hc hcr
    obtain ⟨root, hroot, hempty, hx⟩ := (mem_designRows _ _ _).mp hr
    exact (mem_designRows _ _ _).mpr ⟨root, hroot, hempty,
      expand_under _ opened _ 0 root r hx (by omega) ho c (List.mem_filter.mpr ⟨hc, hcr⟩)⟩
  · intro r hr hne
    obtain ⟨s, hs, e1, _, e3⟩ := (design_buffer nodes opened openOnly).1
      (designPieces (shown nodes openOnly) nodes opened r)
      (List.mem_cons_of_mem _ (List.mem_map.mpr ⟨r, hr, rfl⟩))
      ⟨r.node.id, some Role.requirement, actionsFor Role.requirement⟩
      (by unfold designPieces rowPieces; simp [spanned]) _ rfl hne
    exact ⟨s, hs, e1, e3⟩

/-- @proves REQ-SHOW.open_filtered -/
theorem only_open_requirements_when_asked (nodes : List Node) (opened : List String) :
    (∀ n ∈ nodes, isOpen n = true → n.id ≠ "" →
      Marked (requirementsBuffer nodes true) Role.requirement n.id) ∧
    (∀ s ∈ (requirementsBuffer nodes true).spans, s.role = Role.requirement →
      ∃ n ∈ nodes, isOpen n = true ∧ textUnder (requirementsBuffer nodes true) s = n.id) ∧
    (∀ n, n ∈ shown nodes true ↔
      n ∈ nodes ∧ ∃ c, isOpen c = true ∧ DescendsWithin nodes nodes.length n c) ∧
    (∀ n ∈ nodes, isOpen n = true → n ∈ shown nodes true) ∧
    (∀ c ∈ nodes, isOpen c = true → ∀ n ∈ nodes, c.refines.contains n.id = true → n ∈ shown nodes true) ∧
    (∀ s ∈ (designBuffer nodes opened true).spans, s.role = Role.requirement →
      ∃ n ∈ shown nodes true, textUnder (designBuffer nodes opened true) s = n.id) ∧
    shown nodes false = nodes := by
  have hshown : ∀ n, n ∈ shown nodes true ↔
      n ∈ nodes ∧ ∃ c, isOpen c = true ∧ DescendsWithin nodes nodes.length n c := by
    intro n
    unfold shown
    rw [List.mem_filter, Bool.not_true, Bool.false_or, openUnder_iff]
  refine ⟨?_, ?_, hshown, ?_, ?_, ?_, ?_⟩
  · intro n hn ho hne
    exact row_marked nodes true n hn (by simp [ho]) ⟨n.id, some Role.requirement, actionsFor Role.requirement⟩
      (by unfold rowPieces; simp [spanned]) _ rfl hne
  · intro s hs hr
    obtain ⟨n, hn, ho, e⟩ := requirement_spans nodes true s hs hr
    exact ⟨n, hn, by simpa using ho, e⟩
  · intro n hn ho
    refine (hshown n).mpr ⟨hn, n, ho, ?_⟩
    cases nodes.length <;> simp [DescendsWithin]
  · intro c hc ho n hn hr
    refine (hshown n).mpr ⟨hn, c, ho, ?_⟩
    cases h : nodes.length with
    | zero => rw [List.length_eq_zero] at h; subst h; cases hn
    | succ k =>
      refine Or.inr ⟨c, hc, hr, ?_⟩
      cases k <;> simp [DescendsWithin]
  · intro s hs hr
    obtain ⟨r, hr', e⟩ := design_requirements nodes opened true s hs hr
    obtain ⟨root, hroot, _, hx⟩ := (mem_designRows _ _ _).mp hr'
    rcases expand_rows _ opened _ 0 root r hx with ⟨_, e2⟩ | ⟨_, hn, _⟩
    · exact ⟨r.node, e2 ▸ hroot, e⟩
    · exact ⟨r.node, hn, e⟩
  · unfold shown
    apply List.filter_eq_self.mpr
    intro _ _
    rfl

end TraceLean.Produce
