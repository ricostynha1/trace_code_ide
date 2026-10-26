import Lean
import TraceLean.View
import TraceLean.Produce

namespace TraceLean.Screen

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce

inductive Axis where
  | across
  | down
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Direction where
  | left
  | right
  | up
  | down
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Rect where
  left : Nat
  top : Nat
  width : Nat
  height : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Layout where
  | pane (id : String) (buffer : String)
  | split (axis : Axis) (parts : List (Nat × Layout))
  deriving Repr, Inhabited

private def axisName : Axis → String
  | Axis.across => "across"
  | Axis.down => "down"

private def axisOfName : String → Option Axis
  | "across" => some Axis.across
  | "down" => some Axis.down
  | _ => none

partial def layoutToJson : Layout → Lean.Json
  | Layout.pane id buffer =>
    Lean.Json.mkObj [("pane", Lean.Json.mkObj
      [("id", Lean.Json.str id), ("buffer", Lean.Json.str buffer)])]
  | Layout.split axis parts =>
    let encoded := parts.map (fun part =>
      Lean.Json.arr #[Lean.Json.num (Int.ofNat part.1), layoutToJson part.2])
    Lean.Json.mkObj [("split", Lean.Json.mkObj
      [("axis", Lean.Json.str (axisName axis)),
       ("parts", Lean.Json.arr encoded.toArray)])]

instance : Lean.ToJson Layout := ⟨layoutToJson⟩

private def natOfJson (json : Lean.Json) : Except String Nat :=
  match json.getInt? with
  | Except.ok n => if n < 0 then Except.error "negative weight" else Except.ok n.toNat
  | Except.error e => Except.error e

partial def layoutOfJson (json : Lean.Json) : Except String Layout := do
  match json.getObjVal? "pane" with
  | Except.ok inner => do
    let id ← inner.getObjValAs? String "id"
    let buffer ← inner.getObjValAs? String "buffer"
    return Layout.pane id buffer
  | Except.error _ => do
    let inner ← json.getObjVal? "split"
    let name ← inner.getObjValAs? String "axis"
    let axis ←
      match axisOfName name with
      | some axis => Except.ok axis
      | none => Except.error ("no such axis: " ++ name)
    let parts ← inner.getObjVal? "parts"
    let items ← parts.getArr?
    let decoded ← items.toList.mapM (fun item => do
      let pair ← item.getArr?
      if h : pair.size = 2 then
        let weight ← natOfJson pair[0]
        let layout ← layoutOfJson pair[1]
        return (weight, layout)
      else
        Except.error "a part is a weight and a layout")
    return Layout.split axis decoded

instance : Lean.FromJson Layout := ⟨layoutOfJson⟩

/-- @models REQ-SCREEN.screen_is_a_value -/
structure Screen where
  opened : List Buffer
  layout : Layout
  focus : String
  nextPane : Nat
  deriving Repr, Inhabited

instance : Lean.ToJson Screen where
  toJson screen :=
    Lean.Json.mkObj
      [("opened", Lean.toJson screen.opened),
       ("layout", layoutToJson screen.layout),
       ("focus", Lean.Json.str screen.focus),
       ("nextPane", Lean.Json.num (Int.ofNat screen.nextPane))]

instance : Lean.FromJson Screen where
  fromJson? json := do
    let opened ← json.getObjValAs? (List Buffer) "opened"
    let layout ← layoutOfJson (← json.getObjVal? "layout")
    let focus ← json.getObjValAs? String "focus"
    let next ← natOfJson (← json.getObjVal? "nextPane")
    return { opened := opened, layout := layout, focus := focus, nextPane := next }

mutual
  def panes : Layout → List (String × String)
    | Layout.pane id buffer => [(id, buffer)]
    | Layout.split _ parts => panesOfParts parts

  private def panesOfParts : List (Nat × Layout) → List (String × String)
    | [] => []
    | (_, layout) :: rest => panes layout ++ panesOfParts rest
end

def paneIds (layout : Layout) : List String :=
  (panes layout).map (fun pane => pane.1)

def shownBuffers (layout : Layout) : List String :=
  (panes layout).map (fun pane => pane.2)

private def hasDuplicate : List String → Bool
  | [] => false
  | name :: rest => rest.contains name || hasDuplicate rest

/-- @models REQ-SCREEN.panes_are_distinct -/
def distinctPanes (layout : Layout) : Bool :=
  !hasDuplicate (paneIds layout)

/-- @models REQ-SCREEN.every_pane_is_opened
@models REQ-SCREEN.focus_is_placed -/
def coherent (screen : Screen) : Bool :=
  let openedIds := screen.opened.map (fun buffer => buffer.id)
  (shownBuffers screen.layout).all (fun buffer => openedIds.contains buffer)
    && (paneIds screen.layout).contains screen.focus
    && distinctPanes screen.layout

private def sumOf : List Nat → Nat
  | [] => 0
  | n :: rest => n + sumOf rest

private def runningCuts (extent : Nat) (total : Nat) (soFar : Nat) : List Nat → List Nat
  | [] => []
  | weight :: rest =>
    let upto := soFar + weight
    let cut := extent * upto / total
    let previous := extent * soFar / total
    (cut - previous) :: runningCuts extent total upto rest

def shares (extent : Nat) (weights : List Nat) : List Nat :=
  let total := sumOf weights
  if total = 0 then
    runningCuts extent weights.length 0 (weights.map (fun _ => 1))
  else
    runningCuts extent total 0 weights

private def placingMask : List (Nat × Layout) → List Nat
  | [] => []
  | (_, layout) :: rest =>
    (if (panes layout).isEmpty then 0 else 1) :: placingMask rest

private def effectiveWeights : List (Nat × Layout) → List Nat
  | [] => []
  | (weight, layout) :: rest =>
    (if (panes layout).isEmpty then 0 else weight) :: effectiveWeights rest

private def basisOf (parts : List (Nat × Layout)) : List Nat :=
  let weighted := effectiveWeights parts
  if sumOf weighted = 0 then placingMask parts else weighted

mutual
  /-- @models REQ-SCREEN.layout_tiles_the_region -/
  def place (rect : Rect) : Layout → List (String × Rect)
    | Layout.pane id _ => [(id, rect)]
    | Layout.split axis parts =>
      let extent := match axis with
        | Axis.across => rect.width
        | Axis.down => rect.height
      let start := match axis with
        | Axis.across => rect.left
        | Axis.down => rect.top
      placeParts rect axis start (shares extent (basisOf parts)) parts

  private def placeParts (rect : Rect) (axis : Axis) (at_ : Nat) :
      List Nat → List (Nat × Layout) → List (String × Rect)
    | [], _ => []
    | _, [] => []
    | size :: sizes, (_, layout) :: parts =>
      let here : Rect := match axis with
        | Axis.across => { left := at_, top := rect.top, width := size, height := rect.height }
        | Axis.down => { left := rect.left, top := at_, width := rect.width, height := size }
      place here layout ++ placeParts rect axis (at_ + size) sizes parts
end

private def isOpened (screen : Screen) (id : String) : Bool :=
  (screen.opened.map (fun buffer => buffer.id)).contains id

mutual
  def setBuffer (pane : String) (buffer : String) : Layout → Layout
    | Layout.pane id shown =>
      if id = pane then Layout.pane id buffer else Layout.pane id shown
    | Layout.split axis parts => Layout.split axis (setBufferParts pane buffer parts)

  private def setBufferParts (pane : String) (buffer : String) :
      List (Nat × Layout) → List (Nat × Layout)
    | [] => []
    | (weight, layout) :: rest =>
      (weight, setBuffer pane buffer layout) :: setBufferParts pane buffer rest
end

def showBuffer (id : String) (screen : Screen) : Screen :=
  if isOpened screen id then
    { screen with layout := setBuffer screen.focus id screen.layout }
  else
    screen

/-- @models REQ-SCREEN.opened_outlives_shown
@models REQ-SCREEN.station_opens_a_buffer -/
def openBuffer (buffer : Buffer) (screen : Screen) : Screen :=
  if isOpened screen buffer.id then
    showBuffer buffer.id screen
  else
    showBuffer buffer.id { screen with opened := screen.opened ++ [buffer] }

def explorer : String := "explorer"
def document : String := "document"
def side : String := "side"

private def hold (buffer : Buffer) (opened : List Buffer) : List Buffer :=
  if (opened.map (fun held => held.id)).contains buffer.id then opened else opened ++ [buffer]

/-- @models REQ-SCREEN.workbench_has_three_places -/
def workbench (listing : Buffer) (document_ : Buffer) (side_ : Buffer) : Screen :=
  Screen.mk
    (hold side_ (hold document_ (hold listing [])))
    (Layout.split Axis.across
      [(1, Layout.pane explorer listing.id),
       (2, Layout.pane document document_.id),
       (1, Layout.pane side side_.id)])
    explorer
    0

def documentRecords : List String :=
  ["requirement ", "judge ", "context ", "coverage ", "definitions of ", "uses of ", "search ", "keys"]

def homeOf : BufferKind → String
  | BufferKind.directory _ => explorer
  | BufferKind.file _ => document
  | BufferKind.review _ => document
  | BufferKind.menu _ => side
  | BufferKind.record title =>
    if documentRecords.any (fun lead => title.startsWith lead) then document else side

/-- @models REQ-SCREEN.buffer_goes_home -/
def destination (kind : BufferKind) (screen : Screen) : String :=
  if (paneIds screen.layout).contains (homeOf kind) then homeOf kind else screen.focus

theorem destination_is_placed_or_stays (kind : BufferKind) (screen : Screen) :
    (paneIds screen.layout).contains (destination kind screen) = true ∨
      destination kind screen = screen.focus := by
  unfold destination
  split
  · left; assumption
  · right; rfl

mutual
  def splitAt (pane : String) (fresh : String) (axis : Axis) : Layout → Option Layout
    | Layout.pane id buffer =>
      if id = pane then
        some (Layout.split axis [(1, Layout.pane id buffer), (1, Layout.pane fresh buffer)])
      else
        none
    | Layout.split axis' parts =>
      match splitAtParts pane fresh axis parts with
      | none => none
      | some changed => some (Layout.split axis' changed)

  private def splitAtParts (pane : String) (fresh : String) (axis : Axis) :
      List (Nat × Layout) → Option (List (Nat × Layout))
    | [] => none
    | (weight, layout) :: rest =>
      match splitAt pane fresh axis layout with
      | some changed => some ((weight, changed) :: rest)
      | none =>
        match splitAtParts pane fresh axis rest with
        | some changed => some ((weight, layout) :: changed)
        | none => none
end

private def freeFrom (taken : List String) (start : Nat) : Nat → Nat
  | 0 => start
  | Nat.succ fuel =>
    if taken.contains ("pane" ++ toString start) then freeFrom taken (start + 1) fuel
    else start

/-- @models REQ-SCREEN.split_keeps_the_buffer -/
def splitFocus (axis : Axis) (screen : Screen) : Screen :=
  let taken := paneIds screen.layout
  let free := freeFrom taken screen.nextPane (taken.length + 1)
  match splitAt screen.focus ("pane" ++ toString free) axis screen.layout with
  | none => screen
  | some layout => { screen with layout := layout, nextPane := free + 1 }

mutual
  def without (buffer : String) : Layout → Option Layout
    | Layout.pane id shown => if shown = buffer then none else some (Layout.pane id shown)
    | Layout.split axis parts =>
      match withoutParts buffer parts with
      | [] => none
      | [(_, only)] => some only
      | kept => some (Layout.split axis kept)

  private def withoutParts (buffer : String) : List (Nat × Layout) → List (Nat × Layout)
    | [] => []
    | (weight, layout) :: rest =>
      match without buffer layout with
      | none => withoutParts buffer rest
      | some kept => (weight, kept) :: withoutParts buffer rest
end

private def firstPane : List (String × String) → String
  | [] => ""
  | pane :: _ => pane.1

/-- @models REQ-SCREEN.close_collapses_the_pane -/
def closeBuffer (buffer : String) (screen : Screen) : Screen :=
  let remaining := screen.opened.filter (fun held => held.id ≠ buffer)
  match remaining with
  | [] => screen
  | first :: _ =>
    let layout :=
      match without buffer screen.layout with
      | some kept => kept
      | none => Layout.pane screen.focus first.id
    let focus :=
      if (paneIds layout).contains screen.focus then screen.focus else firstPane (panes layout)
    { screen with opened := remaining, layout := layout, focus := focus }

private def holdsPane (pane : String) (layout : Layout) : Bool :=
  (paneIds layout).contains pane

private def shiftAt (index : Nat) (amount : Int) (parts : List (Nat × Layout)) :
    List (Nat × Layout) :=
  match parts.get? index, parts.get? (index + 1) with
  | some here, some next =>
    let hereWeight : Int := Int.ofNat here.1 + amount
    let nextWeight : Int := Int.ofNat next.1 - amount
    if hereWeight ≥ 1 && nextWeight ≥ 1 then
      (parts.set index (hereWeight.toNat, here.2)).set (index + 1) (nextWeight.toNat, next.2)
    else parts
  | _, _ => parts

private def indexHolding (pane : String) : Nat → List (Nat × Layout) → Option Nat
  | _, [] => none
  | at_, part :: rest =>
    if holdsPane pane part.2 then some at_ else indexHolding pane (at_ + 1) rest

mutual
  def resizeAt (pane : String) (amount : Int) : Layout → Layout
    | Layout.pane id buffer => Layout.pane id buffer
    | Layout.split axis parts =>
      match indexHolding pane 0 parts with
      | none => Layout.split axis parts
      | some index =>
        let nested :=
          match axis, parts.get? index with
          | Axis.across, some (_, Layout.split Axis.across _) => true
          | Axis.down, some (_, Layout.split Axis.down _) => true
          | _, _ => false
        if nested then
          Layout.split axis (resizeAtOnly pane amount index 0 parts)
        else if index + 1 < parts.length then
          Layout.split axis (shiftAt index amount parts)
        else if index = 0 then
          Layout.split axis (resizeAtParts pane amount parts)
        else
          Layout.split axis (shiftAt (index - 1) (-amount) parts)

  private def resizeAtOnly (pane : String) (amount : Int) (target : Nat) :
      Nat → List (Nat × Layout) → List (Nat × Layout)
    | _, [] => []
    | at_, (weight, layout) :: rest =>
      (weight, if at_ == target then resizeAt pane amount layout else layout) ::
        resizeAtOnly pane amount target (at_ + 1) rest

  private def resizeAtParts (pane : String) (amount : Int) :
      List (Nat × Layout) → List (Nat × Layout)
    | [] => []
    | (weight, layout) :: rest =>
      (weight, resizeAt pane amount layout) :: resizeAtParts pane amount rest
end

/-- @models REQ-SCREEN.resize_moves_one_divider
@models REQ-SCREEN.resize_has_a_floor -/
def resizeFocus (amount : Int) (screen : Screen) : Screen :=
  { screen with layout := resizeAt screen.focus amount screen.layout }

private def overlaps (a : Rect) (b : Rect) : Direction → Bool
  | Direction.left => a.top < b.top + b.height && b.top < a.top + a.height
  | Direction.right => a.top < b.top + b.height && b.top < a.top + a.height
  | Direction.up => a.left < b.left + b.width && b.left < a.left + a.width
  | Direction.down => a.left < b.left + b.width && b.left < a.left + a.width

private def beyond (from_ : Rect) (other : Rect) : Direction → Bool
  | Direction.left => other.left + other.width ≤ from_.left
  | Direction.right => from_.left + from_.width ≤ other.left
  | Direction.up => other.top + other.height ≤ from_.top
  | Direction.down => from_.top + from_.height ≤ other.top

private def nearer (from_ : Rect) (dir : Direction) (a : String × Rect) (b : String × Rect) :
    String × Rect :=
  let distance (rect : Rect) : Nat :=
    match dir with
    | Direction.left => from_.left - (rect.left + rect.width)
    | Direction.right => rect.left - (from_.left + from_.width)
    | Direction.up => from_.top - (rect.top + rect.height)
    | Direction.down => rect.top - (from_.top + from_.height)
  let earlier (one : Rect) (other : Rect) : Bool :=
    one.top < other.top || (one.top = other.top && one.left < other.left)
  if distance b.2 < distance a.2 then b
  else if distance a.2 < distance b.2 then a
  else if earlier b.2 a.2 then b
  else a

private def pick (from_ : Rect) (dir : Direction) :
    List (String × Rect) → Option (String × Rect)
  | [] => none
  | candidate :: rest =>
    match pick from_ dir rest with
    | none => some candidate
    | some best => some (nearer from_ dir candidate best)

private def rectOf (pane : String) : List (String × Rect) → Option Rect
  | [] => none
  | placed :: rest => if placed.1 = pane then some placed.2 else rectOf pane rest

/-- @models REQ-SCREEN.focus_follows_geometry -/
def focusStep (rect : Rect) (screen : Screen) (dir : Direction) : String :=
  let placed := place rect screen.layout
  match rectOf screen.focus placed with
  | none => screen.focus
  | some from_ =>
    let candidates := placed.filter (fun other =>
      other.1 ≠ screen.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir)
    match pick from_ dir candidates with
    | none => screen.focus
    | some landed => landed.1

inductive Arrangement where
  | split (axis : Axis)
  | close
  | focus (dir : Direction)
  | focusPane (pane : String)
  | resize (amount : Int)
  | showBuffer (buffer : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

private def focusedBuffer (screen : Screen) : Option String :=
  match (panes screen.layout).find? (fun pane => pane.1 = screen.focus) with
  | none => none
  | some pane => some pane.2

/-- @models REQ-SCREEN.one_arrangement_path -/
def arrange (how : Arrangement) (rect : Rect) (screen : Screen) : Screen :=
  match how with
  | Arrangement.split axis => splitFocus axis screen
  | Arrangement.close =>
    match focusedBuffer screen with
    | none => screen
    | some buffer => closeBuffer buffer screen
  | Arrangement.focus dir => { screen with focus := focusStep rect screen dir }
  | Arrangement.focusPane pane =>
    if (paneIds screen.layout).contains pane then { screen with focus := pane } else screen
  | Arrangement.resize amount => resizeFocus amount screen
  | Arrangement.showBuffer buffer => showBuffer buffer screen

def lastSegment (path : String) : String :=
  (path.splitOn "/").getLastD path

def titleOf (buffer : Buffer) : String :=
  match buffer.kind with
  | BufferKind.file path => lastSegment path
  | BufferKind.directory _ => "files"
  | BufferKind.review target => "review " ++ lastSegment target
  | BufferKind.menu title => title
  | BufferKind.record title => title

private def markFocused (focused : Option String) : List Buffer → List Span → List Span
  | buffer :: rest, span :: spans =>
    (if some buffer.id = focused then { span with role := Role.heading } else span)
      :: markFocused focused rest spans
  | _, spans => spans

def foldersOf (path : String) : List String :=
  (path.splitOn "/").dropLast

def lastOf (n : Nat) (xs : List String) : List String :=
  xs.drop (xs.length - n)

def distinguishing (path : String) (others : List String) : String :=
  let mine := foldersOf path
  let theirs := others.map foldersOf
  let apart := fun n => theirs.all fun folders => lastOf n folders != lastOf n mine
  let n := ((List.range mine.length).map (· + 1)).find? apart |>.getD mine.length
  "/".intercalate (lastOf n mine)

def stripTitle (opened : List Buffer) (buffer : Buffer) : String :=
  match buffer.kind with
  | BufferKind.file path =>
    let name := lastSegment path
    let others := opened.filterMap fun other =>
      match other.kind with
      | BufferKind.file p => if p != path && lastSegment p == name then some p else none
      | _ => none
    let folders := distinguishing path others
    if others.isEmpty || folders.isEmpty then name else name ++ " · " ++ folders
  | _ => titleOf buffer

private def stripEntries (opened : List Buffer) (at_ : Nat) : List Buffer → List MenuEntry
  | [] => []
  | buffer :: rest =>
    { key := toString at_, description := stripTitle opened buffer,
      action := some ("screen.show " ++ buffer.id) }
      :: stripEntries opened (at_ + 1) rest

/-- @models REQ-SCREEN.strip_is_the_opened_set -/
def strip (screen : Screen) : Buffer :=
  let rows := menuBuffer "opened" (stripEntries screen.opened 1 screen.opened)
  { rows with spans := markFocused (focusedBuffer screen) screen.opened rows.spans }

def stationEntries : List MenuEntry :=
  [{ key := "project", description := "Open a project",
     action := some "screen.station.project" },
   { key := "trace", description := "What this file claims, and what else claims it",
     action := some "screen.station.trace" },
   { key := "sandbox", description := "Watch a sandboxed agent",
     action := some "screen.station.sandbox" },
   { key := "design", description := "Requirements, as the refinement graph",
     action := some "screen.station.design" },
   { key := "history", description := "The undo tree",
     action := some "screen.station.history" },
   { key := "findings", description := "What the checker found, and what to create",
     action := some "screen.station.findings" },
   { key := "definitions", description := "What can be configured: colours, keys, languages",
     action := some "screen.station.definitions" }]

/-- @models REQ-SCREEN.stations_are_constant -/
def stations (screen : Screen) : Buffer :=
  let _ := screen
  menuBuffer "stations" stationEntries

/-- @models REQ-SCREEN.station_produces_a_buffer -/
def stationKind (station : String) : Option BufferKind :=
  if station == "project" then some (BufferKind.directory ".")
  else if station == "design" then some (BufferKind.menu "design")
  else if station == "sandbox" then some (BufferKind.record "sandbox")
  else if station == "history" then some (BufferKind.record "history")
  else if station == "trace" then some (BufferKind.record "trace")
  else if station == "findings" then some (BufferKind.record "findings")
  else if station == "definitions" then some (BufferKind.record "definitions")
  else none

/-- @proves REQ-SCREEN.stations_are_constant -/
theorem stations_constant (a b : Screen) : stations a = stations b := rfl

/-- @proves REQ-SCREEN.opened_outlives_shown -/
theorem show_keeps_opened (id : String) (screen : Screen) :
    (showBuffer id screen).opened = screen.opened := by
  unfold showBuffer
  split <;> rfl

/-- @proves REQ-SCREEN.opened_outlives_shown -/
theorem open_twice_holds_one (buffer : Buffer) (screen : Screen) :
    isOpened screen buffer.id = true →
    (openBuffer buffer screen).opened = screen.opened := by
  intro held
  unfold openBuffer
  simp [held, show_keeps_opened]

/-- @specifies REQ-SCREEN.panes_are_distinct -/
def PanesDistinct (layout : Layout) (answer : Bool) : Prop :=
  answer = true ↔ List.Nodup (paneIds layout)

private theorem no_duplicate_iff_nodup (names : List String) :
    hasDuplicate names = false ↔ List.Nodup names := by
  induction names
  case nil => simp [hasDuplicate]
  case cons name rest ih => simp [hasDuplicate, List.nodup_cons, ih]

/-- @pins REQ-SCREEN.panes_are_distinct -/
theorem panes_distinct_pinned :
    (∀ x1, PanesDistinct x1 (distinctPanes x1)) ∧
    (∀ x1 y1 y2, PanesDistinct x1 y1 → PanesDistinct x1 y2 → y1 = y2) := by
  constructor
  · intro layout
    unfold PanesDistinct distinctPanes
    rw [← no_duplicate_iff_nodup]
    cases hasDuplicate (paneIds layout)
    all_goals simp
  · intro layout y1 y2 h1 h2
    unfold PanesDistinct at h1 h2
    cases y1
    all_goals cases y2
    all_goals simp_all

/-- @proves REQ-SCREEN.panes_are_distinct -/
theorem distinct_panes_answers (layout : Layout) : PanesDistinct layout (distinctPanes layout) :=
  panes_distinct_pinned.1 layout

def bare (id : String) (kind : BufferKind) : Buffer := ⟨id, kind, "", []⟩

def bench : Screen :=
  workbench (bare "dir:." (.directory ".")) (bare "file:a.rs" (.file "a.rs")) (bare "menu:design" (.menu "design"))

def region : Rect := ⟨0, 0, 80, 24⟩

def held (screen : Screen) : List (String × String) × String × List String :=
  (panes screen.layout, screen.focus, screen.opened.map (·.id))

def weights : Layout → List Nat
  | Layout.split _ parts => parts.map (·.1)
  | Layout.pane _ _ => []

mutual
  theorem panes_setBuffer (p b : String) : ∀ l : Layout,
      panes (setBuffer p b l) = (panes l).map (fun q => if q.1 = p then (q.1, b) else q)
    | Layout.pane id shown => by
      by_cases h : id = p <;> simp [setBuffer, panes, h]
    | Layout.split axis parts => by
      simp only [setBuffer, panes]
      exact panes_setBufferParts p b parts
  theorem panes_setBufferParts (p b : String) : ∀ parts : List (Nat × Layout),
      panesOfParts (setBufferParts p b parts) =
        (panesOfParts parts).map (fun q => if q.1 = p then (q.1, b) else q)
    | [] => by simp [setBufferParts, panesOfParts]
    | (w, l) :: rest => by
      simp only [setBufferParts, panesOfParts, List.map_append]
      rw [panes_setBuffer p b l, panes_setBufferParts p b rest]
end

def covers (r : Rect) (x y : Nat) : Bool :=
  decide (r.left ≤ x ∧ x < r.left + r.width ∧ r.top ≤ y ∧ y < r.top + r.height)

def coverCount (placed : List (String × Rect)) (x y : Nat) : Nat :=
  (placed.filter (fun q => covers q.2 x y)).length

def band (rect : Rect) (axis : Axis) (at_ : Nat) (size : Nat) : Rect :=
  match axis with
  | Axis.across => { left := at_, top := rect.top, width := size, height := rect.height }
  | Axis.down => { left := rect.left, top := at_, width := rect.width, height := size }

def Fits : List Nat → List (Nat × Layout) → Prop
  | [], [] => True
  | size :: sizes, (_, layout) :: parts => (0 < size → panes layout ≠ []) ∧ Fits sizes parts
  | _, _ => False

theorem coverCount_append (a b : List (String × Rect)) (x y : Nat) :
    coverCount (a ++ b) x y = coverCount a x y + coverCount b x y := by
  simp [coverCount, List.filter_append]

theorem band_split (rect : Rect) (axis : Axis) (at_ s t x y : Nat) :
    (if covers (band rect axis at_ s) x y then 1 else 0) +
      (if covers (band rect axis (at_ + s) t) x y then 1 else 0) =
    (if covers (band rect axis at_ (s + t)) x y then 1 else 0) := by
  cases axis <;> simp only [band, covers] <;> split <;> split <;> split <;>
    simp only [decide_eq_true_eq] at * <;> omega

theorem band_zero (rect : Rect) (axis : Axis) (at_ x y : Nat) :
    covers (band rect axis at_ 0) x y = false := by
  cases axis <;> simp [band, covers] <;> omega

mutual
  theorem place_nil : ∀ (l : Layout) (r : Rect), panes l = [] → place r l = []
    | Layout.pane id b, r, h => by simp [panes] at h
    | Layout.split axis parts, r, h => by
      simp only [place]
      exact placeParts_nil parts r axis _ _ (by simpa [panes] using h)
  theorem placeParts_nil : ∀ (parts : List (Nat × Layout)) (r : Rect) (axis : Axis) (at_ : Nat)
      (sizes : List Nat), panesOfParts parts = [] → placeParts r axis at_ sizes parts = []
    | [], r, axis, at_, sizes, _ => by cases sizes <;> simp [placeParts]
    | (w, l) :: rest, r, axis, at_, sizes, h => by
      simp only [panesOfParts, List.append_eq_nil] at h
      cases sizes with
      | nil => simp [placeParts]
      | cons s ss =>
        simp only [placeParts]
        rw [place_nil l _ h.1, placeParts_nil rest r axis _ ss h.2]
        rfl
end

theorem runningCuts_length (e T s : Nat) (ws : List Nat) :
    (runningCuts e T s ws).length = ws.length := by
  induction ws generalizing s with
  | nil => simp [runningCuts]
  | cons w rest ih => simp [runningCuts, ih]

theorem shares_length (e : Nat) (ws : List Nat) : (shares e ws).length = ws.length := by
  unfold shares
  simp only
  split <;> simp [runningCuts_length]

theorem basisOf_length (parts : List (Nat × Layout)) : (basisOf parts).length = parts.length := by
  have hm : ∀ ps : List (Nat × Layout), (placingMask ps).length = ps.length := by
    intro ps
    induction ps with
    | nil => simp [placingMask]
    | cons p rest ih => simp [placingMask, ih]
  have hw : ∀ ps : List (Nat × Layout), (effectiveWeights ps).length = ps.length := by
    intro ps
    induction ps with
    | nil => simp [effectiveWeights]
    | cons p rest ih => simp [effectiveWeights, ih]
  unfold basisOf
  simp only
  split
  · exact hm parts
  · exact hw parts

theorem div_mono (a b T : Nat) (h : a ≤ b) : a / T ≤ b / T := by
  rcases Nat.eq_zero_or_pos T with hT | hT
  · subst hT; simp
  · exact (Nat.le_div_iff_mul_le hT).mpr (Nat.le_trans (Nat.div_mul_le_self a T) h)

theorem sumOf_runningCuts (e T s : Nat) (ws : List Nat) :
    sumOf (runningCuts e T s ws) = e * (s + sumOf ws) / T - e * s / T := by
  induction ws generalizing s with
  | nil => simp [runningCuts, sumOf]
  | cons w rest ih =>
    simp only [runningCuts, sumOf]
    rw [ih]
    have h1 : e * s / T ≤ e * (s + w) / T :=
      div_mono _ _ T (Nat.mul_le_mul_left _ (by omega))
    have h2 : e * (s + w) / T ≤ e * (s + w + sumOf rest) / T :=
      div_mono _ _ T (Nat.mul_le_mul_left _ (by omega))
    rw [show s + (w + sumOf rest) = s + w + sumOf rest by omega]
    omega

theorem fits_effective (e T s : Nat) (parts : List (Nat × Layout)) :
    Fits (runningCuts e T s (effectiveWeights parts)) parts := by
  induction parts generalizing s with
  | nil => simp [effectiveWeights, runningCuts, Fits]
  | cons p rest ih =>
    obtain ⟨w, l⟩ := p
    simp only [effectiveWeights, runningCuts, Fits]
    refine ⟨?_, ih _⟩
    intro hpos hnil
    simp [hnil] at hpos

theorem fits_mask (e T s : Nat) (parts : List (Nat × Layout)) :
    Fits (runningCuts e T s (placingMask parts)) parts := by
  induction parts generalizing s with
  | nil => simp [placingMask, runningCuts, Fits]
  | cons p rest ih =>
    obtain ⟨w, l⟩ := p
    simp only [placingMask, runningCuts, Fits]
    refine ⟨?_, ih _⟩
    intro hpos hnil
    simp [hnil] at hpos

theorem mask_pos (parts : List (Nat × Layout)) (h : panesOfParts parts ≠ []) :
    0 < sumOf (placingMask parts) := by
  induction parts with
  | nil => simp [panesOfParts] at h
  | cons p rest ih =>
    obtain ⟨w, l⟩ := p
    simp only [placingMask, sumOf]
    by_cases hl : panes l = []
    · have hr : panesOfParts rest ≠ [] := by simpa [panesOfParts, hl] using h
      have := ih hr
      omega
    · simp [hl]
      omega

theorem shares_basis (e : Nat) (parts : List (Nat × Layout)) (h : panesOfParts parts ≠ []) :
    Fits (shares e (basisOf parts)) parts ∧ sumOf (shares e (basisOf parts)) = e := by
  have run : ∀ ws : List Nat, 0 < sumOf ws → shares e ws = runningCuts e (sumOf ws) 0 ws := by
    intro ws hpos
    unfold shares
    simp [Nat.pos_iff_ne_zero.mp hpos]
  have total : ∀ ws : List Nat, 0 < sumOf ws → sumOf (runningCuts e (sumOf ws) 0 ws) = e := by
    intro ws hpos
    rw [sumOf_runningCuts]
    simp [Nat.mul_div_cancel _ hpos]
  unfold basisOf
  simp only
  split
  · have hp := mask_pos parts h
    rw [run _ hp]
    exact ⟨fits_mask _ _ _ _, total _ hp⟩
  · rename_i hne
    have hp : 0 < sumOf (effectiveWeights parts) := Nat.pos_of_ne_zero hne
    rw [run _ hp]
    exact ⟨fits_effective _ _ _ _, total _ hp⟩

mutual
  theorem count_place : ∀ (l : Layout) (r : Rect) (x y : Nat), panes l ≠ [] →
      coverCount (place r l) x y = if covers r x y then 1 else 0
    | Layout.pane id b, r, x, y, _ => by
      simp only [place, coverCount, List.filter]
      split <;> simp_all
    | Layout.split axis parts, r, x, y, h => by
      have hp : panesOfParts parts ≠ [] := by simpa [panes] using h
      cases axis
      · simp only [place]
        rw [count_placeParts parts r Axis.across _ _ x y (shares_basis r.width parts hp).1,
          (shares_basis r.width parts hp).2]
        rfl
      · simp only [place]
        rw [count_placeParts parts r Axis.down _ _ x y (shares_basis r.height parts hp).1,
          (shares_basis r.height parts hp).2]
        rfl
  theorem count_placeParts : ∀ (parts : List (Nat × Layout)) (rect : Rect) (axis : Axis)
      (at_ : Nat) (sizes : List Nat) (x y : Nat), Fits sizes parts →
      coverCount (placeParts rect axis at_ sizes parts) x y =
        if covers (band rect axis at_ (sumOf sizes)) x y then 1 else 0
    | [], rect, axis, at_, [], x, y, _ => by
      simp [placeParts, coverCount, sumOf, band_zero]
    | [], _, _, _, _ :: _, _, _, h => by simp [Fits] at h
    | _ :: _, _, _, _, [], _, _, h => by simp [Fits] at h
    | (w, l) :: rest, rect, axis, at_, s :: ss, x, y, h => by
      simp only [Fits] at h
      simp only [placeParts]
      rw [coverCount_append, count_placeParts rest rect axis (at_ + s) ss x y h.2, sumOf,
        ← band_split]
      congr 1
      by_cases hl : panes l = []
      · have hs : s = 0 := by
          false_or_by_contra
          exact h.1 (by omega) hl
        subst hs
        rw [place_nil l _ hl]
        simp [coverCount, band_zero]
      · have := count_place l (band rect axis at_ s) x y hl
        cases axis <;> exact this
end

mutual
  theorem place_ids : ∀ (l : Layout) (r : Rect), (place r l).map Prod.fst = paneIds l
    | Layout.pane _ _, _ => by simp [place, paneIds, panes]
    | Layout.split axis parts, r => by
      simp only [place, paneIds, panes]
      exact placeParts_ids parts r axis _ _ (by rw [shares_length, basisOf_length])
  theorem placeParts_ids : ∀ (parts : List (Nat × Layout)) (r : Rect) (axis : Axis) (at_ : Nat)
      (sizes : List Nat), sizes.length = parts.length →
      (placeParts r axis at_ sizes parts).map Prod.fst = (panesOfParts parts).map Prod.fst
    | [], r, axis, at_, sizes, _ => by cases sizes <;> simp [placeParts, panesOfParts]
    | (w, l) :: rest, r, axis, at_, sizes, h => by
      cases sizes with
      | nil => simp at h
      | cons s ss =>
        simp only [placeParts, panesOfParts, List.map_append]
        rw [placeParts_ids rest r axis _ ss (by simpa using h)]
        congr 1
        cases axis <;> exact place_ids l _
end

/-- @proves REQ-SCREEN.layout_tiles_the_region -/
theorem layout_tiles_region (rect : Rect) (layout : Layout) (h : panes layout ≠ []) :
    (place rect layout).map Prod.fst = paneIds layout ∧
    ∀ x y, coverCount (place rect layout) x y = if covers rect x y then 1 else 0 :=
  ⟨place_ids layout rect, fun x y => count_place layout rect x y h⟩

/-- @proves REQ-SCREEN.screen_is_a_value -/
theorem screen_is_one_value (a b : Screen) :
    a = b ↔ a.opened = b.opened ∧ a.layout = b.layout ∧ a.focus = b.focus ∧
      a.nextPane = b.nextPane := by
  constructor
  · intro h
    subst h
    simp
  · intro ⟨h1, h2, h3, h4⟩
    cases a
    cases b
    simp_all

/-- @proves REQ-SCREEN.every_pane_is_opened
@proves REQ-SCREEN.focus_is_placed -/
theorem coherent_iff (s : Screen) :
    coherent s = true ↔
      (∀ b ∈ shownBuffers s.layout, ∃ held ∈ s.opened, held.id = b) ∧
      s.focus ∈ paneIds s.layout ∧ (paneIds s.layout).Nodup := by
  unfold coherent distinctPanes
  rw [← no_duplicate_iff_nodup]
  simp [List.all_eq_true, and_assoc]

theorem paneIds_setBuffer (p b : String) (l : Layout) :
    paneIds (setBuffer p b l) = paneIds l := by
  unfold paneIds
  rw [panes_setBuffer, List.map_map]
  apply List.map_congr_left
  intro q _
  simp only [Function.comp]
  split <;> rfl

theorem shown_setBuffer (p b : String) (l : Layout) (x : String)
    (h : x ∈ shownBuffers (setBuffer p b l)) : x ∈ shownBuffers l ∨ x = b := by
  unfold shownBuffers at *
  rw [panes_setBuffer, List.map_map] at h
  obtain ⟨q, hq, hx⟩ := List.mem_map.mp h
  simp only [Function.comp] at hx
  split at hx
  · exact Or.inr hx.symm
  · exact Or.inl (List.mem_map.mpr ⟨q, hq, hx⟩)

/-- @proves REQ-SCREEN.every_pane_is_opened
@proves REQ-SCREEN.focus_is_placed -/
theorem show_keeps_coherent (id : String) (s : Screen) (h : coherent s = true) :
    coherent (showBuffer id s) = true := by
  rw [coherent_iff] at *
  obtain ⟨hshown, hfocus, hnodup⟩ := h
  unfold showBuffer
  split
  · rename_i hopen
    refine ⟨?_, ?_, ?_⟩
    · intro x hx
      rcases shown_setBuffer _ _ _ x hx with hx | hx
      · exact hshown x hx
      · subst hx
        unfold isOpened at hopen
        simpa using hopen
    · simpa [paneIds_setBuffer] using hfocus
    · simpa [paneIds_setBuffer] using hnodup
  · exact ⟨hshown, hfocus, hnodup⟩

theorem hold_has (buffer : Buffer) (opened : List Buffer) :
    buffer.id ∈ (hold buffer opened).map (fun held => held.id) := by
  unfold hold
  split
  · rename_i h
    simpa using h
  · simp

theorem hold_keeps (buffer : Buffer) (opened : List Buffer) (x : String)
    (h : x ∈ opened.map (fun held => held.id)) :
    x ∈ (hold buffer opened).map (fun held => held.id) := by
  unfold hold
  split
  · exact h
  · simp only [List.map_append, List.mem_append]
    exact Or.inl h

/-- @proves REQ-SCREEN.workbench_has_three_places -/
theorem workbench_three_places (listing document_ side_ : Buffer) :
    (∃ parts, (workbench listing document_ side_).layout = Layout.split Axis.across parts) ∧
    panes (workbench listing document_ side_).layout =
      [(explorer, listing.id), (document, document_.id), (side, side_.id)] ∧
    (workbench listing document_ side_).focus = explorer ∧
    coherent (workbench listing document_ side_) = true := by
  refine ⟨⟨_, rfl⟩, rfl, rfl, ?_⟩
  rw [coherent_iff]
  have hl := hold_keeps side_ _ _ (hold_keeps document_ _ _ (hold_has listing []))
  have hd := hold_keeps side_ _ _ (hold_has document_ (hold listing []))
  have hs := hold_has side_ (hold document_ (hold listing []))
  refine ⟨?_, ?_, ?_⟩
  · intro b hb
    simp only [workbench, shownBuffers, panes, panesOfParts, List.map, List.mem_cons,
      List.append_nil, List.cons_append, List.nil_append] at hb
    simp only [workbench, List.mem_map] at hl hd hs
    rcases hb with hb | hb | hb | hb
    · subst hb; exact hl
    · subst hb; exact hd
    · subst hb; exact hs
    · simp at hb
  · simp [workbench, paneIds, panes, panesOfParts, explorer]
  · simp [workbench, paneIds, panes, panesOfParts, explorer, document, side]

def pageLeads : List String :=
  ["requirement ", "judge ", "context ", "definitions of ", "uses of ", "search ", "keys"]

theorem lead_is_document (title : String) (h : pageLeads.any (fun lead => title.startsWith lead)) :
    documentRecords.any (fun lead => title.startsWith lead) = true := by
  simp only [pageLeads, documentRecords, List.any_cons, List.any_nil, Bool.or_false,
    Bool.or_eq_true] at *
  rcases h with h | h | h | h | h | h | h <;> simp [h]

/-- @proves REQ-SCREEN.buffer_goes_home -/
theorem buffer_goes_home (s : Screen) (path title : String) :
    let into := fun home => if home ∈ paneIds s.layout then home else s.focus
    destination (BufferKind.directory path) s = into explorer ∧
    destination (BufferKind.file path) s = into document ∧
    destination (BufferKind.review path) s = into document ∧
    destination (BufferKind.menu title) s = into side ∧
    (pageLeads.any (fun lead => title.startsWith lead) = true →
      destination (BufferKind.record title) s = into document) ∧
    (documentRecords.any (fun lead => title.startsWith lead) = false →
      destination (BufferKind.record title) s = into side) := by
  simp only [destination, homeOf, List.elem_iff, List.contains]
  refine ⟨trivial, trivial, trivial, trivial, ?_, ?_⟩
  · intro h
    rw [if_pos (lead_is_document title h)]
  · intro h
    simp [h]

mutual
  theorem splitAt_panes (pane fresh : String) (axis : Axis) : ∀ (l l' : Layout),
      splitAt pane fresh axis l = some l' →
      ∃ pre post b, panes l = pre ++ (pane, b) :: post ∧
        panes l' = pre ++ (pane, b) :: (fresh, b) :: post
    | Layout.pane id buffer, l', h => by
      simp only [splitAt] at h
      split at h
      · rename_i hid
        subst hid
        cases h
        exact ⟨[], [], buffer, by simp [panes], by simp [panes, panesOfParts]⟩
      · cases h
    | Layout.split axis' parts, l', h => by
      simp only [splitAt] at h
      split at h
      · cases h
      · rename_i changed hc
        cases h
        obtain ⟨pre, post, b, h1, h2⟩ := splitAtParts_panes pane fresh axis parts changed hc
        exact ⟨pre, post, b, by simpa [panes] using h1, by simpa [panes] using h2⟩
  theorem splitAtParts_panes (pane fresh : String) (axis : Axis) :
      ∀ (parts changed : List (Nat × Layout)),
      splitAtParts pane fresh axis parts = some changed →
      ∃ pre post b, panesOfParts parts = pre ++ (pane, b) :: post ∧
        panesOfParts changed = pre ++ (pane, b) :: (fresh, b) :: post
    | [], changed, h => by simp [splitAtParts] at h
    | (w, l) :: rest, changed, h => by
      simp only [splitAtParts] at h
      split at h
      · rename_i c hc
        cases h
        obtain ⟨pre, post, b, h1, h2⟩ := splitAt_panes pane fresh axis l c hc
        refine ⟨pre, post ++ panesOfParts rest, b, ?_, ?_⟩
        · simp [panesOfParts, h1]
        · simp [panesOfParts, h2]
      · split at h
        · rename_i c hc
          cases h
          obtain ⟨pre, post, b, h1, h2⟩ := splitAtParts_panes pane fresh axis rest c hc
          refine ⟨panes l ++ pre, post, b, ?_, ?_⟩
          · simp [panesOfParts, h1]
          · simp [panesOfParts, h2]
        · cases h
end

mutual
  theorem splitAt_found (pane fresh : String) (axis : Axis) : ∀ (l : Layout),
      pane ∈ paneIds l → splitAt pane fresh axis l ≠ none
    | Layout.pane id buffer, h => by
      simp [paneIds, panes] at h
      subst h
      simp [splitAt]
    | Layout.split axis' parts, h => by
      have hp : pane ∈ (panesOfParts parts).map Prod.fst := by simpa [paneIds, panes] using h
      have := splitAtParts_found pane fresh axis parts hp
      simp only [splitAt]
      split
      · contradiction
      · simp
  theorem splitAtParts_found (pane fresh : String) (axis : Axis) :
      ∀ (parts : List (Nat × Layout)),
      pane ∈ (panesOfParts parts).map Prod.fst → splitAtParts pane fresh axis parts ≠ none
    | [], h => by simp [panesOfParts] at h
    | (w, l) :: rest, h => by
      simp only [splitAtParts]
      split
      · simp
      · rename_i hl
        have hr : pane ∈ (panesOfParts rest).map Prod.fst := by
          simp only [panesOfParts, List.map_append, List.mem_append] at h
          rcases h with h | h
          · exact absurd hl (splitAt_found pane fresh axis l h)
          · exact h
        have := splitAtParts_found pane fresh axis rest hr
        split
        · simp
        · contradiction
end

/-- @proves REQ-SCREEN.split_keeps_the_buffer -/
theorem split_keeps_buffer (axis : Axis) (s : Screen) (h : s.focus ∈ paneIds s.layout) :
    ∃ fresh pre post b, panes s.layout = pre ++ (s.focus, b) :: post ∧
      panes (splitFocus axis s).layout = pre ++ (s.focus, b) :: (fresh, b) :: post := by
  unfold splitFocus
  simp only
  split
  · rename_i hn
    exact absurd hn (splitAt_found _ _ axis _ h)
  · rename_i l hl
    obtain ⟨pre, post, b, h1, h2⟩ := splitAt_panes _ _ axis _ _ hl
    exact ⟨_, pre, post, b, h1, h2⟩

theorem mem_panesOfParts (p : String × String) (parts : List (Nat × Layout)) :
    p ∈ panesOfParts parts ↔ ∃ q ∈ parts, p ∈ panes q.2 := by
  induction parts with
  | nil => simp [panesOfParts]
  | cons q rest ih =>
    obtain ⟨w, l⟩ := q
    simp [panesOfParts, ih]

theorem without_split_kept (buffer : String) (axis : Axis) (parts : List (Nat × Layout))
    (k : Layout)
    (hall : ∀ q ∈ withoutParts buffer parts, panes q.2 ≠ [] ∧ ∀ p ∈ panes q.2, p.2 ≠ buffer)
    (h : without buffer (Layout.split axis parts) = some k) :
    panes k ≠ [] ∧ ∀ p ∈ panes k, p.2 ≠ buffer := by
  simp only [without] at h
  split at h
  · cases h
  · rename_i w only hk
    cases h
    exact hall (w, k) (by simp [hk])
  · rename_i kept h1 h2
    cases h
    refine ⟨?_, ?_⟩
    · cases hL : withoutParts buffer parts with
      | nil => exact absurd hL h1
      | cons q rest =>
        obtain ⟨w, l⟩ := q
        obtain ⟨hq, _⟩ := hall (w, l) (by rw [hL]; simp)
        simp only [panes, panesOfParts]
        intro hnil
        exact hq (List.append_eq_nil.mp hnil).1
    · intro p hp
      simp only [panes] at hp
      obtain ⟨q, hq, hpq⟩ := (mem_panesOfParts p _).mp hp
      exact (hall q hq).2 p hpq

mutual
  theorem without_kept (buffer : String) : ∀ (l k : Layout), without buffer l = some k →
      panes k ≠ [] ∧ ∀ p ∈ panes k, p.2 ≠ buffer
    | Layout.pane id shown, k, h => by
      simp only [without] at h
      split at h
      · cases h
      · rename_i hs
        cases h
        simp [panes, hs]
    | Layout.split axis parts, k, h =>
      without_split_kept buffer axis parts k (withoutParts_kept buffer parts) h
  theorem withoutParts_kept (buffer : String) : ∀ (parts : List (Nat × Layout)),
      ∀ q ∈ withoutParts buffer parts, panes q.2 ≠ [] ∧ ∀ p ∈ panes q.2, p.2 ≠ buffer
    | [] => by simp [withoutParts]
    | (w, l) :: rest => by
      have hrest := withoutParts_kept buffer rest
      intro q hq
      simp only [withoutParts] at hq
      split at hq
      · exact hrest q hq
      · rename_i k hk
        simp only [List.mem_cons] at hq
        rcases hq with hq | hq
        · subst hq
          exact without_kept buffer l k hk
        · exact hrest q hq
end

theorem firstPane_placed (l : Layout) (h : panes l ≠ []) : firstPane (panes l) ∈ paneIds l := by
  unfold paneIds
  cases hp : panes l with
  | nil => exact absurd hp h
  | cons q rest => simp [firstPane]

/-- @proves REQ-SCREEN.close_collapses_the_pane -/
theorem close_collapses (buffer : String) (s : Screen) :
    (s.opened.filter (fun held => held.id ≠ buffer) = [] → closeBuffer buffer s = s) ∧
    (s.opened.filter (fun held => held.id ≠ buffer) ≠ [] →
      (closeBuffer buffer s).opened = s.opened.filter (fun held => held.id ≠ buffer) ∧
      (∀ held ∈ (closeBuffer buffer s).opened, held.id ≠ buffer) ∧
      panes (closeBuffer buffer s).layout ≠ [] ∧
      (∀ p ∈ panes (closeBuffer buffer s).layout, p.2 ≠ buffer) ∧
      (closeBuffer buffer s).focus ∈ paneIds (closeBuffer buffer s).layout) := by
  refine ⟨?_, ?_⟩
  · intro h
    unfold closeBuffer
    simp only
    rw [h]
  · intro h
    have keep : ∀ l : Layout, panes l ≠ [] →
        (if (paneIds l).contains s.focus then s.focus else firstPane (panes l)) ∈ paneIds l := by
      intro l hl
      split
      · rename_i hc
        simpa using hc
      · exact firstPane_placed l hl
    unfold closeBuffer
    simp only
    split
    · contradiction
    · rename_i first others hfo
      have hfirst : first.id ≠ buffer := by
        have : first ∈ s.opened.filter (fun held => held.id ≠ buffer) := by rw [hfo]; simp
        simpa using (List.mem_filter.mp this).2
      have hlayout : panes (match without buffer s.layout with
          | some kept => kept
          | none => Layout.pane s.focus first.id) ≠ [] ∧
          ∀ p ∈ panes (match without buffer s.layout with
            | some kept => kept
            | none => Layout.pane s.focus first.id), p.2 ≠ buffer := by
        split
        · rename_i k hk
          exact without_kept buffer s.layout k hk
        · simp [panes, hfirst]
      refine ⟨rfl, ?_, hlayout.1, hlayout.2, keep _ hlayout.1⟩
      intro held hh
      simpa using (List.mem_filter.mp hh).2

theorem weights_setBuffer (p b : String) (l : Layout) : weights (setBuffer p b l) = weights l := by
  cases l with
  | pane id shown =>
    simp only [setBuffer]
    split <;> rfl
  | split axis parts =>
    simp only [setBuffer, weights]
    induction parts with
    | nil => simp [setBufferParts]
    | cons q rest ih =>
      obtain ⟨w, _⟩ := q
      simp [setBufferParts, ih]

/-- @proves REQ-SCREEN.station_opens_a_buffer -/
theorem open_shows_in_focus (buffer : Buffer) (s : Screen) :
    panes (openBuffer buffer s).layout =
      (panes s.layout).map (fun q => if q.1 = s.focus then (q.1, buffer.id) else q) ∧
    weights (openBuffer buffer s).layout = weights s.layout ∧
    (openBuffer buffer s).focus = s.focus ∧
    buffer.id ∈ (openBuffer buffer s).opened.map (fun held => held.id) := by
  have shown : (openBuffer buffer s).layout = setBuffer s.focus buffer.id s.layout ∧
      (openBuffer buffer s).focus = s.focus ∧
      buffer.id ∈ (openBuffer buffer s).opened.map (fun held => held.id) := by
    unfold openBuffer showBuffer isOpened
    split
    · rename_i h
      simp only [h, if_true]
      simp_all
    · simp
  refine ⟨?_, ?_, shown.2.1, shown.2.2⟩
  · rw [shown.1, panes_setBuffer]
  · rw [shown.1, weights_setBuffer]

def OneDivider (before after : List Nat) : Prop :=
  after = before ∨
  (after.length = before.length ∧ ∃ i, i + 1 < before.length ∧
    (∀ j, j ≠ i → j ≠ i + 1 → after[j]? = before[j]?) ∧
    ∃ a b a' b', before[i]? = some a ∧ before[i + 1]? = some b ∧
      after[i]? = some a' ∧ after[i + 1]? = some b' ∧
      a' + b' = a + b ∧ 1 ≤ a' ∧ 1 ≤ b')

theorem shiftAt_divider (i : Nat) (amount : Int) (parts : List (Nat × Layout)) :
    OneDivider (parts.map (·.1)) ((shiftAt i amount parts).map (·.1)) := by
  unfold shiftAt
  split
  · rename_i here next h1 h2
    simp only
    split
    · rename_i hok
      simp only [Bool.and_eq_true, decide_eq_true_eq] at hok
      right
      have hlen : i + 1 < parts.length := (List.get?_eq_some.mp h2).1
      have hi : i < parts.length := by omega
      simp only [List.get?_eq_getElem?] at h1 h2
      refine ⟨by simp, i, by simpa using hlen, ?_, ?_⟩
      · intro j hj1 hj2
        simp [List.getElem?_set_ne (Ne.symm hj2), List.getElem?_set_ne (Ne.symm hj1)]
      · refine ⟨here.1, next.1, (Int.ofNat here.1 + amount).toNat,
          (Int.ofNat next.1 - amount).toNat, ?_, ?_, ?_, ?_, ?_, ?_, ?_⟩
        · simp [h1]
        · simp [h2]
        · simp [List.getElem?_set_ne (show i + 1 ≠ i by omega), List.getElem?_set_eq hi]
        · simp only [List.getElem?_map]
          rw [List.getElem?_set_eq (by simpa using hlen)]
          rfl
        · simp only [Int.ofNat_eq_coe] at hok ⊢
          have e1 := Int.toNat_of_nonneg (show 0 ≤ (here.1 : Int) + amount by omega)
          have e2 := Int.toNat_of_nonneg (show 0 ≤ (next.1 : Int) - amount by omega)
          omega
        · simp only [Int.ofNat_eq_coe] at hok ⊢
          have e1 := Int.toNat_of_nonneg (show 0 ≤ (here.1 : Int) + amount by omega)
          omega
        · simp only [Int.ofNat_eq_coe] at hok ⊢
          have e2 := Int.toNat_of_nonneg (show 0 ≤ (next.1 : Int) - amount by omega)
          omega
    · left; rfl
  · left; rfl

theorem shiftAt_refuses (i : Nat) (amount : Int) (parts : List (Nat × Layout))
    (here next : Nat × Layout) (h1 : parts.get? i = some here)
    (h2 : parts.get? (i + 1) = some next)
    (hbad : Int.ofNat here.1 + amount < 1 ∨ Int.ofNat next.1 - amount < 1) :
    shiftAt i amount parts = parts := by
  unfold shiftAt
  rw [h1, h2]
  simp only
  split
  · rename_i hok
    simp only [Bool.and_eq_true, decide_eq_true_eq] at hok
    omega
  · rfl

theorem resizeAtOnly_weights (pane : String) (amount : Int) (target : Nat) :
    ∀ (at_ : Nat) (parts : List (Nat × Layout)),
    (resizeAtOnly pane amount target at_ parts).map (·.1) = parts.map (·.1)
  | _, [] => by simp [resizeAtOnly]
  | at_, (w, l) :: rest => by
    simp only [resizeAtOnly, List.map_cons]
    rw [resizeAtOnly_weights pane amount target (at_ + 1) rest]

theorem resizeAtParts_weights (pane : String) (amount : Int) :
    ∀ (parts : List (Nat × Layout)),
    (resizeAtParts pane amount parts).map (·.1) = parts.map (·.1)
  | [] => by simp [resizeAtParts]
  | (w, l) :: rest => by
    simp only [resizeAtParts, List.map_cons]
    rw [resizeAtParts_weights pane amount rest]

theorem resizeAt_divider (pane : String) (amount : Int) (l : Layout) :
    OneDivider (weights l) (weights (resizeAt pane amount l)) := by
  cases l with
  | pane id buffer =>
    left
    simp [resizeAt]
  | split axis parts =>
    simp only [resizeAt]
    split
    · left; rfl
    · rename_i index _
      split
      · left
        simp only [weights]
        exact resizeAtOnly_weights pane amount index 0 parts
      · split
        · exact shiftAt_divider index amount parts
        · split
          · left
            simp only [weights]
            exact resizeAtParts_weights pane amount parts
          · exact shiftAt_divider (index - 1) (-amount) parts

/-- @proves REQ-SCREEN.resize_moves_one_divider
@proves REQ-SCREEN.resize_has_a_floor -/
theorem resize_moves_one_divider (amount : Int) (s : Screen) :
    OneDivider (weights s.layout) (weights (resizeFocus amount s).layout) ∧
    (resizeFocus amount s).focus = s.focus ∧ (resizeFocus amount s).opened = s.opened :=
  ⟨resizeAt_divider s.focus amount s.layout, rfl, rfl⟩

/-- @proves REQ-SCREEN.resize_has_a_floor -/
theorem resize_refused_below_floor (amount : Int) (s : Screen) (axis : Axis)
    (parts : List (Nat × Layout)) (index : Nat) (here next : Nat × Layout)
    (hlayout : s.layout = Layout.split axis parts)
    (hindex : indexHolding s.focus 0 parts = some index)
    (hflat : ∀ ps, here.2 ≠ Layout.split axis ps)
    (h1 : parts.get? index = some here) (h2 : parts.get? (index + 1) = some next)
    (hbad : Int.ofNat here.1 + amount < 1 ∨ Int.ofNat next.1 - amount < 1) :
    (resizeFocus amount s).layout = s.layout := by
  have hlen : index + 1 < parts.length := (List.get?_eq_some.mp h2).1
  unfold resizeFocus
  rw [hlayout]
  simp only [resizeAt, hindex, h1]
  split
  · rename_i hn
    obtain ⟨w, inner⟩ := here
    cases axis <;> cases inner with
    | pane _ _ => simp at hn
    | split a ps => cases a <;> first | exact absurd rfl (hflat ps) | simp at hn
  · rw [shiftAt_refuses index amount parts here next h1 h2 hbad]

/-- @proves REQ-SCREEN.station_produces_a_buffer -/
theorem station_produces (station : String) :
    (stationKind station).isSome = true ↔ station ∈ stationEntries.map (·.key) := by
  unfold stationKind
  simp only [stationEntries, List.map, List.mem_cons, List.not_mem_nil, or_false]
  repeat' split
  all_goals simp_all

theorem stripEntries_rows (opened : List Buffer) :
    ∀ (at_ : Nat) (buffers : List Buffer),
    (stripEntries opened at_ buffers).map (·.action) =
      buffers.map (fun buffer => some ("screen.show " ++ buffer.id)) ∧
    (stripEntries opened at_ buffers).map (·.description) = buffers.map (stripTitle opened)
  | _, [] => by simp [stripEntries]
  | at_, buffer :: rest => by
    obtain ⟨h1, h2⟩ := stripEntries_rows opened (at_ + 1) rest
    simp [stripEntries, h1, h2]

theorem markFocused_keeps (focused : Option String) :
    ∀ (buffers : List Buffer) (spans : List Span),
    (markFocused focused buffers spans).map (fun span => (span.start, span.stop, span.actions)) =
      spans.map (fun span => (span.start, span.stop, span.actions))
  | [], spans => by simp [markFocused]
  | _ :: _, [] => by simp [markFocused]
  | buffer :: rest, span :: spans => by
    simp only [markFocused, List.map_cons]
    rw [markFocused_keeps focused rest spans]
    split <;> rfl

/-- @proves REQ-SCREEN.strip_is_the_opened_set -/
theorem strip_is_opened (s : Screen) :
    (strip s).kind = BufferKind.menu "opened" ∧
    (strip s).text = (menuBuffer "opened" (stripEntries s.opened 1 s.opened)).text ∧
    (strip s).spans.map (fun span => (span.start, span.stop, span.actions)) =
      (menuBuffer "opened" (stripEntries s.opened 1 s.opened)).spans.map
        (fun span => (span.start, span.stop, span.actions)) ∧
    (stripEntries s.opened 1 s.opened).map (·.action) =
      s.opened.map (fun buffer => some ("screen.show " ++ buffer.id)) ∧
    (stripEntries s.opened 1 s.opened).map (·.description) = s.opened.map (stripTitle s.opened) := by
  refine ⟨rfl, rfl, ?_, (stripEntries_rows s.opened 1 s.opened).1,
    (stripEntries_rows s.opened 1 s.opened).2⟩
  exact markFocused_keeps _ _ _

def gap (from_ : Rect) (dir : Direction) (rect : Rect) : Nat :=
  match dir with
  | Direction.left => from_.left - (rect.left + rect.width)
  | Direction.right => rect.left - (from_.left + from_.width)
  | Direction.up => from_.top - (rect.top + rect.height)
  | Direction.down => rect.top - (from_.top + from_.height)

theorem nearer_spec (from_ : Rect) (dir : Direction) (a b : String × Rect) :
    (nearer from_ dir a b = a ∨ nearer from_ dir a b = b) ∧
    gap from_ dir (nearer from_ dir a b).2 ≤ gap from_ dir a.2 ∧
    gap from_ dir (nearer from_ dir a b).2 ≤ gap from_ dir b.2 := by
  cases dir <;> simp only [nearer, gap] <;> split <;> (try split) <;> (try split) <;>
    simp_all <;> omega

theorem pick_spec (from_ : Rect) (dir : Direction) :
    ∀ (cs : List (String × Rect)),
    (pick from_ dir cs = none → cs = []) ∧
    ∀ best, pick from_ dir cs = some best →
      best ∈ cs ∧ ∀ c ∈ cs, gap from_ dir best.2 ≤ gap from_ dir c.2
  | [] => by simp [pick]
  | c :: rest => by
    obtain ⟨hnone, hsome⟩ := pick_spec from_ dir rest
    refine ⟨?_, ?_⟩
    · intro h
      simp only [pick] at h
      split at h <;> simp at h
    · intro best h
      simp only [pick] at h
      split at h
      · rename_i hr
        cases h
        have := hnone hr
        subst this
        simp
      · rename_i b hb
        cases h
        obtain ⟨hmem, hle⟩ := hsome b hb
        obtain ⟨which, hc, hbest⟩ := nearer_spec from_ dir c b
        refine ⟨?_, ?_⟩
        · rcases which with w | w <;> rw [w] <;> simp [hmem]
        · intro x hx
          simp only [List.mem_cons] at hx
          rcases hx with hx | hx
          · subst hx; exact hc
          · exact Nat.le_trans hbest (hle x hx)

/-- @proves REQ-SCREEN.focus_follows_geometry -/
theorem focus_follows_geometry (rect : Rect) (s : Screen) (dir : Direction) :
    (rectOf s.focus (place rect s.layout) = none → focusStep rect s dir = s.focus) ∧
    ∀ from_, rectOf s.focus (place rect s.layout) = some from_ →
      ((place rect s.layout).filter (fun other =>
          other.1 ≠ s.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir) = [] →
        focusStep rect s dir = s.focus) ∧
      ∃ r, ((place rect s.layout).filter (fun other =>
          other.1 ≠ s.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir) ≠ [] →
        (focusStep rect s dir, r) ∈ (place rect s.layout).filter (fun other =>
          other.1 ≠ s.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir) ∧
        ∀ c ∈ (place rect s.layout).filter (fun other =>
            other.1 ≠ s.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir),
          gap from_ dir r ≤ gap from_ dir c.2) := by
  refine ⟨?_, ?_⟩
  · intro h
    unfold focusStep
    simp only [h]
  · intro from_ h
    unfold focusStep
    simp only [h]
    obtain ⟨hnone, hsome⟩ := pick_spec from_ dir ((place rect s.layout).filter (fun other =>
          other.1 ≠ s.focus && beyond from_ other.2 dir && overlaps from_ other.2 dir))
    split
    · rename_i hp
      refine ⟨fun _ => rfl, default, fun hne => absurd (hnone hp) hne⟩
    · rename_i landed hp
      obtain ⟨hmem, hle⟩ := hsome landed hp
      refine ⟨fun hnil => by rw [hnil] at hmem; simp at hmem, landed.2, fun _ => ⟨hmem, hle⟩⟩

theorem focusStep_placed (rect : Rect) (s : Screen) (dir : Direction) :
    focusStep rect s dir = s.focus ∨ focusStep rect s dir ∈ paneIds s.layout := by
  unfold focusStep
  simp only
  split
  · left; rfl
  · rename_i from_ _
    split
    · left; rfl
    · rename_i landed hp
      right
      obtain ⟨hmem, _⟩ := (pick_spec from_ dir _).2 landed hp
      rw [← place_ids s.layout rect]
      exact List.mem_map.mpr ⟨landed, (List.mem_filter.mp hmem).1, rfl⟩

/-- @proves REQ-SCREEN.one_arrangement_path -/
theorem key_and_pointer_agree (dir : Direction) (rect : Rect) (s : Screen) :
    arrange (Arrangement.focus dir) rect s =
      arrange (Arrangement.focusPane (focusStep rect s dir)) rect s ∧
    ∀ how r1 r2, (∀ d, how ≠ Arrangement.focus d) → arrange how r1 s = arrange how r2 s := by
  refine ⟨?_, ?_⟩
  · simp only [arrange]
    rcases focusStep_placed rect s dir with h | h
    · rw [h]
      split <;> rfl
    · simp [h]
  · intro how r1 r2 h
    cases how with
    | focus d => exact absurd rfl (h d)
    | _ => rfl

def goals : String := "record:proof"

def isLeanId (id : String) : Bool := id.startsWith "file:" && id.endsWith ".lean"

/-- @models REQ-LSP.goals_open_with_the_file -/
def opensGoals (kind : BufferKind) (opened : List String) : Bool :=
  match kind with
  | .file path => path.endsWith ".lean" && !opened.contains goals
  | _ => false

/-- @models REQ-LSP.goals_close_with_the_file -/
def closedWith (before after : List String) : List String :=
  let closed := before.filter (fun id => !after.contains id)
  if closed.contains goals then after.filter isLeanId
  else if closed.any isLeanId && !after.any isLeanId && after.contains goals then [goals]
  else []

/-- @proves REQ-LSP.goals_open_with_the_file -/
theorem goals_open_exactly_with_a_lean_file (kind : BufferKind) (opened : List String) :
    opensGoals kind opened = true ↔
      ∃ path, kind = .file path ∧ path.endsWith ".lean" = true ∧ goals ∉ opened := by
  cases kind <;> simp [opensGoals]

/-- @proves REQ-LSP.goals_close_with_the_file -/
theorem closing_the_goals_closes_every_lean_file (before after : List String)
    (closed : goals ∈ before) (gone : goals ∉ after) :
    closedWith before after = after.filter isLeanId := by
  have : (before.filter (fun id => !after.contains id)).contains goals = true := by
    simp [List.mem_filter, closed, gone]
  simp only [closedWith, this, if_true]

theorem no_lean_left (after : List String) (none : ∀ x ∈ after, isLeanId x = false) :
    after.any isLeanId = false := by
  cases h : after.any isLeanId
  · rfl
  · obtain ⟨x, hx, hl⟩ := List.any_eq_true.mp h
    simp [none x hx] at hl

/-- @proves REQ-LSP.goals_close_with_the_file -/
theorem closing_the_last_lean_file_closes_the_goals (before after : List String) (file : String)
    (kept : goals ∈ after) (was : file ∈ before) (gone : file ∉ after) (lean : isLeanId file = true)
    (none : ∀ x ∈ after, isLeanId x = false) :
    closedWith before after = [goals] := by
  have closedLean : ∃ x, x ∈ before ∧ x ∉ after ∧ isLeanId x = true := ⟨file, was, gone, lean⟩
  simp [closedWith, kept, closedLean, no_lean_left after none]

/-- @proves REQ-LSP.goals_close_with_the_file -/
theorem closing_other_buffers_closes_nothing (before after : List String)
    (stays : ∀ x ∈ before, x ∉ after → x ≠ goals ∧ isLeanId x = false) :
    closedWith before after = [] := by
  have notGoals : ¬(goals ∈ before ∧ goals ∉ after) := fun ⟨a, b⟩ => (stays _ a b).1 rfl
  have notLean : ¬∃ x, x ∈ before ∧ x ∉ after ∧ isLeanId x = true :=
    fun ⟨x, a, b, c⟩ => by simp [(stays x a b).2] at c
  simp [closedWith]
  rw [if_neg notGoals, if_neg (fun h => notLean h.1.1)]

end TraceLean.Screen
