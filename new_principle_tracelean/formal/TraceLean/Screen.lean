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
     action := some "screen.station.definitions" },
   { key := "proof", description := "The goals Lean reports at the cursor",
     action := some "screen.station.proof" }]

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
  else if station == "proof" then some (BufferKind.record "proof")
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

end TraceLean.Screen
