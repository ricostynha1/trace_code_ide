import Lean
import TraceLean.Produce

open Lean

namespace TraceLean.HistoryView

structure Point where
  node : Nat
  parent : Option Nat
  here : Bool
  said : String
  file : Option String
  saved : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure GraphRow where
  node : Option Nat
  cells : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Filter where
  | all | file | saved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def filterNamed (name : String) : Option Filter :=
  match name with
  | "All" => some .all
  | "File" => some .file
  | "Saved" => some .saved
  | _ => none

def isHere (ps : List Point) : Option Nat → Bool
  | none => !ps.any (·.here)
  | some n => ps.any (fun p => p.node == n && p.here)

def climbs (ps : List Point) (target : Nat) : Nat → Option Nat → Bool
  | 0, _ => false
  | _, none => false
  | fuel + 1, some current =>
    if current == target then true
    else climbs ps target fuel ((ps.find? (fun p => p.node == current)).bind (·.parent))

def onCurrentBranch (ps : List Point) : Option Nat → Bool
  | none => true
  | some target => climbs ps target (ps.length + 1) ((ps.find? (·.here)).map (·.node))

def freeColumn (open_ : List Bool) (taken : List Nat) : Nat → Nat → Nat
  | 0, c => c
  | fuel + 1, c =>
    if open_.getD c false || taken.contains c then freeColumn open_ taken fuel (c + 1) else c

def kidsOf (ps : List Point) (node : Option Nat) (drawn : List Nat) : List Nat :=
  ps.foldl (fun acc p =>
    if p.parent == node && !drawn.contains p.node && !acc.contains p.node then acc ++ [p.node] else acc) []

def takenFor (open_ : List Bool) (col : Nat) (branches : List Nat) : List Nat :=
  branches.foldl (fun acc _ =>
    acc ++ [freeColumn open_ acc (open_.length + acc.length + 1) (col + 1)]) []

def cellsOf (open_ : List Bool) (col : Nat) (taken : List Nat) (glyph : Char) : List Char :=
  let base := open_.foldr (fun o acc => (if o then '│' else ' ') :: ' ' :: acc) []
  let marked := base.set (col * 2) glyph
  match taken.getLast? with
  | none => marked
  | some last =>
    let dashed := marked.enum.map (fun (i, ch) =>
      if col * 2 + 1 ≤ i && i < last * 2 && ch == ' ' then '─' else ch)
    taken.foldl (fun cs c => cs.set (c * 2) (if c == last then '╮' else '┬')) dashed

def seenAlready (drawn : List Nat) : Option Nat → Bool
  | some n => drawn.contains n
  | none => false

def markDrawn (drawn : List Nat) : Option Nat → List Nat
  | some n => drawn ++ [n]
  | none => drawn

structure Step where
  row : GraphRow
  columns : List Bool
  pending : List (Option Nat × Nat)

def stepOf (ps : List Point) (node : Option Nat) (col : Nat) (rest : List (Option Nat × Nat))
    (open_ : List Bool) (drawn : List Nat) : Step :=
  let kids := kidsOf ps node (markDrawn drawn node)
  let taken := takenFor open_ col (kids.drop 1)
  let width := max (taken.foldl (fun w c => max w (c + 1)) (col + 1)) open_.length
  let widened := open_ ++ List.replicate (width - open_.length) false
  let glyph := if isHere ps node then '●' else '○'
  let closed := widened.set col (!kids.isEmpty)
  let branches := ((kids.drop 1).zip taken).map (fun (k, c) => (some k, c))
  let first := (kids.head?.map (fun k => (some k, col))).toList
  { row := GraphRow.mk node (String.mk (cellsOf widened col taken glyph)),
    columns := taken.foldl (fun o c => o.set c true) closed,
    pending := branches ++ first ++ rest }

def graphLoop (ps : List Point) :
    Nat → List (Option Nat × Nat) → List Bool → List Nat → List GraphRow → List GraphRow
  | 0, _, _, _, out => out
  | _ + 1, [], _, _, out => out
  | fuel + 1, (node, col) :: rest, open_, drawn, out =>
    if seenAlready drawn node then graphLoop ps fuel rest open_ drawn out
    else graphLoop ps fuel (stepOf ps node col rest open_ drawn).pending
      (stepOf ps node col rest open_ drawn).columns (markDrawn drawn node)
      (out ++ [(stepOf ps node col rest open_ drawn).row])

/-- @models REQ-UNDO.tree_is_drawn -/
def historyGraph (points : List Point) : List GraphRow :=
  graphLoop points (points.length + 1) [(none, 0)] [] [] []

def keep (f : Filter) (file : String) (p : Point) : Bool :=
  match f with
  | .all => true
  | .file => p.file == some file
  | .saved => p.saved

def keptAncestor (ps : List Point) (f : Filter) (file : String) : Nat → Option Nat → Option Nat
  | 0, _ => none
  | _ + 1, none => none
  | fuel + 1, some n =>
    match ps.find? (·.node == n) with
    | none => none
    | some p => if keep f file p then some n else keptAncestor ps f file fuel p.parent

/-- @models REQ-UNDO.filtered_view -/
def shownPoints (points : List Point) (filter : Filter) (file : String) : List Point :=
  let fuel := points.length + 1
  let current := (points.find? (·.here)).map (·.node)
  let target := keptAncestor points filter file fuel current
  (points.filter (keep filter file)).map (fun p =>
    { p with parent := keptAncestor points filter file fuel p.parent,
             here := some p.node == target })

/-- @proves REQ-UNDO.filtered_view -/
theorem shown_are_kept (points : List Point) (f : Filter) (file : String) :
    (shownPoints points f file).all (keep f file) = true := by
  unfold shownPoints keep
  cases f
  all_goals simp [List.all_eq_true]

open TraceLean.View
open TraceLean.Produce

def pointName : Option Nat → String
  | none => "#0"
  | some n => "#" ++ toString (n + 1)

def switches : List (Filter × String) := [(.all, "All"), (.file, "File"), (.saved, "Saved")]

def switchLine (filter : Filter) : String × List Span :=
  switches.foldl (fun (acc : String × List Span) (pair : Filter × String) =>
    let at_ := acc.1.length
    let role := if pair.1 == filter then Role.heading else Role.entry
    (acc.1 ++ pair.2 ++ "  ",
     acc.2 ++ [{ start := at_, stop := at_ + pair.2.length, role := role, actions := ["history.filter"] }]))
    ("", [])

def treeRow (all shown : List Point) (graph : Nat) (acc : String × List Span) (row : GraphRow) :
    String × List Span :=
  let text := acc.1 ++ "\n"
  let at_ := text.length
  let cut := row.cells.toList.take graph
  let cells := String.mk (cut ++ List.replicate (graph - cut.length) ' ')
  let said :=
    match row.node with
    | none => "the tree as it was opened"
    | some n => ((shown.find? (·.node == n)).map (·.said)).getD ""
  let name := pointName row.node
  let from_ := at_ + graph + 1
  let role :=
    if row.cells.toList.contains '●' then Role.heading
    else if onCurrentBranch all row.node then Role.added
    else Role.entry
  (text ++ cells ++ " " ++ name ++ "  " ++ said,
   acc.2 ++ [{ start := from_, stop := from_ + name.length, role := role, actions := ["history.jump"] }])

/-- @models REQ-UNDO.tree_is_shown -/
def historyView (points : List Point) (filter : Filter) (file : Option String) : Buffer :=
  let shown := shownPoints points filter (file.getD "")
  let drawn := historyGraph shown
  let top := switchLine filter
  let heading :=
    match filter, file with
    | .file, some f => top.1 ++ f
    | _, _ => top.1
  let graph :=
    match drawn with
    | [] => 0
    | _ => (drawn.foldl (fun m r => max m r.cells.length) 0) - 1
  let done := drawn.foldl (treeRow points shown graph) (heading, top.2)
  { id := "record:history", kind := .record "history", text := done.1,
    spans := tidy done.1.length done.2 }

def near (diff : List DiffLine) (i : Nat) : Bool :=
  ((diff.drop (i - 2)).take (min (i + 3) diff.length - (i - 2))).any (fun d => d.role != Role.plain)

def nearLines (diff : List DiffLine) : Nat → Bool → List DiffLine → List (String × Role)
  | _, _, [] => []
  | i, skipped, d :: rest =>
    if near diff i then (d.text, d.role) :: nearLines diff (i + 1) false rest
    else if !skipped then ("…", Role.plain) :: nearLines diff (i + 1) true rest
    else nearLines diff (i + 1) true rest

structure Changed where
  path : String
  before : String
  after : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def changedLines (c : Changed) : List (String × Role) :=
  let diff := diffLines (c.before.splitOn "\n") (c.after.splitOn "\n")
  (c.path, Role.path) :: nearLines diff 0 false diff

def joinLines (acc : String × List Span) (line : String × Role) : String × List Span :=
  let text := if acc.1.isEmpty then acc.1 else acc.1 ++ "\n"
  let at_ := text.length
  let spans :=
    if line.2 == Role.plain then acc.2
    else acc.2 ++ [{ start := at_, stop := at_ + line.1.length, role := line.2, actions := [] }]
  (text ++ line.1, spans)

/-- @models REQ-UNDO.hover_shows_change -/
def changeView (node : Nat) (said : String) (changed : List Changed) : Buffer :=
  let name := pointName (some node)
  let lines := (name ++ "  " ++ said, Role.heading) :: changed.bind changedLines
  let done := lines.foldl joinLines ("", [])
  { id := "record:change " ++ name, kind := .record ("change " ++ name), text := done.1,
    spans := tidy done.1.length done.2 }

def branched : List Point :=
  [⟨0, none, false, "a", none, false⟩, ⟨1, some 0, false, "b", none, false⟩,
   ⟨2, some 0, true, "c", none, false⟩]

/-- @proves REQ-UNDO.tree_is_drawn -/
theorem a_branch_opens_a_column :
    historyGraph branched =
      [⟨none, "○ "⟩, ⟨some 0, "○─╮ "⟩, ⟨some 2, "│ ● "⟩, ⟨some 1, "○   "⟩] := by
  native_decide

/-- @proves REQ-UNDO.tree_is_shown -/
theorem every_node_is_named_and_jumps :
    (historyView branched .all none).text
      = "All  File  Saved  \n○   #0  the tree as it was opened\n○─╮ #1  a\n│ ● #3  c\n○   #2  b" ∧
    ((historyView branched .all none).spans.filter (·.actions == ["history.jump"])).length = 4 := by
  native_decide

/-- @proves REQ-UNDO.hover_shows_change -/
theorem a_change_shows_its_lines :
    (changeView 1 "b" [⟨"a.rs", "x\ny", "x\nz"⟩]).text = "#2  b\na.rs\n x\n-y\n+z" ∧
    ((changeView 1 "b" [⟨"a.rs", "x\ny", "x\nz"⟩]).spans.map (·.role))
      = [Role.heading, Role.path, Role.removed, Role.added] := by
  native_decide

end TraceLean.HistoryView
