import Lean

/-!
# The history, drawn as a tree

Models `REQ-UNDO.tree_is_drawn` and `REQ-UNDO.filtered_view`: the rows the
undo tree is drawn in, each node under the node it was made after, and the
nodes a filter keeps, each under its nearest kept ancestor.
-/

open Lean

namespace TraceLean.HistoryView

/-- One node as the view shows it. -/
structure Point where
  node : Nat
  parent : Option Nat
  here : Bool
  said : String
  file : Option String
  saved : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A row of the drawing: the node (`none` for the tree as opened) and its
cells, two to a column. -/
structure GraphRow where
  node : Option Nat
  cells : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Which nodes the view shows. -/
inductive Filter where
  | all | file | saved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The filter a switch's label names. -/
def filterNamed (name : String) : Option Filter :=
  match name with
  | "All" => some .all
  | "File" => some .file
  | "Saved" => some .saved
  | _ => none

def isHere (ps : List Point) : Option Nat → Bool
  | none => !ps.any (·.here)
  | some n => ps.any (fun p => p.node == n && p.here)

/-- The first column from `c` on that no line runs through and no branch has
taken. Each step passes a blocked column, and there are no more of those
than the fuel. -/
def freeColumn (open_ : List Bool) (taken : List Nat) : Nat → Nat → Nat
  | 0, c => c
  | fuel + 1, c =>
    if open_.getD c false || taken.contains c then freeColumn open_ taken fuel (c + 1) else c

/-- The children of `node` not yet drawn, each once, in the order given. -/
def kidsOf (ps : List Point) (node : Option Nat) (drawn : List Nat) : List Nat :=
  ps.foldl (fun acc p =>
    if p.parent == node && !drawn.contains p.node && !acc.contains p.node then acc ++ [p.node] else acc) []

/-- A column for each branch after the first child, each the first free one. -/
def takenFor (open_ : List Bool) (col : Nat) (branches : List Nat) : List Nat :=
  branches.foldl (fun acc _ =>
    acc ++ [freeColumn open_ acc (open_.length + acc.length + 1) (col + 1)]) []

/-- The cells of one row. -/
def cellsOf (open_ : List Bool) (col : Nat) (taken : List Nat) (glyph : Char) : List Char :=
  let base := open_.foldr (fun o acc => (if o then '│' else ' ') :: ' ' :: acc) []
  let marked := base.set (col * 2) glyph
  match taken.getLast? with
  | none => marked
  | some last =>
    let dashed := marked.enum.map (fun (i, ch) =>
      if col * 2 + 1 ≤ i && i < last * 2 && ch == ' ' then '─' else ch)
    taken.foldl (fun cs c => cs.set (c * 2) (if c == last then '╮' else '┬')) dashed

/-- Whether a node was drawn already; the start never is. -/
def seenAlready (drawn : List Nat) : Option Nat → Bool
  | some n => drawn.contains n
  | none => false

def markDrawn (drawn : List Nat) : Option Nat → List Nat
  | some n => drawn ++ [n]
  | none => drawn

/-- One node drawn: its row, the columns open after it, and what is left to draw. -/
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

/-- Draw from the stack `pending` (top first), spending one fuel a pop. -/
def graphLoop (ps : List Point) :
    Nat → List (Option Nat × Nat) → List Bool → List Nat → List GraphRow → List GraphRow
  | 0, _, _, _, out => out
  | _ + 1, [], _, _, out => out
  | fuel + 1, (node, col) :: rest, open_, drawn, out =>
    if seenAlready drawn node then graphLoop ps fuel rest open_ drawn out
    else graphLoop ps fuel (stepOf ps node col rest open_ drawn).pending
      (stepOf ps node col rest open_ drawn).columns (markDrawn drawn node)
      (out ++ [(stepOf ps node col rest open_ drawn).row])

/-- The tree as rows, top to bottom: the tree as opened, then each node under
what it was made after, each once, at most a row a node and one for the start.

@models REQ-UNDO.tree_is_drawn -/
def historyGraph (points : List Point) : List GraphRow :=
  graphLoop points (points.length + 1) [(none, 0)] [] [] []

def keep (f : Filter) (file : String) (p : Point) : Bool :=
  match f with
  | .all => true
  | .file => p.file == some file
  | .saved => p.saved

/-- The nearest node from `start` up, `start` included, that the filter keeps. -/
def keptAncestor (ps : List Point) (f : Filter) (file : String) : Nat → Option Nat → Option Nat
  | 0, _ => none
  | _ + 1, none => none
  | fuel + 1, some n =>
    match ps.find? (·.node == n) with
    | none => none
    | some p => if keep f file p then some n else keptAncestor ps f file fuel p.parent

/-- The nodes the filter keeps, each under its nearest kept ancestor, the one
nearest the workspace's position marked.

@models REQ-UNDO.filtered_view -/
def shownPoints (points : List Point) (filter : Filter) (file : String) : List Point :=
  let fuel := points.length + 1
  let current := (points.find? (·.here)).map (·.node)
  let target := keptAncestor points filter file fuel current
  (points.filter (keep filter file)).map (fun p =>
    { p with parent := keptAncestor points filter file fuel p.parent,
             here := some p.node == target })

/-- A filtered view shows nothing the filter refused.

@proves REQ-UNDO.filtered_view -/
theorem shown_are_kept (points : List Point) (f : Filter) (file : String) :
    (shownPoints points f file).all (keep f file) = true := by
  unfold shownPoints keep
  cases f
  all_goals simp [List.all_eq_true]

end TraceLean.HistoryView
