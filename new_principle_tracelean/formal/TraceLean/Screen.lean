import Lean
import TraceLean.View
import TraceLean.Produce

/-!
# What is opened, and what is shown

Models `REQ-SCREEN`. `REQ-VIEW` describes *a* buffer and `REQ-SHOW` says where
each one comes from; an editor shows several at once, and until this there was
no value that said which. The window could draw one thing, not because anybody
chose that but because nothing else was expressible.

A screen is the buffers opened, a layout placing some of them, and which pane
has focus. A pane has an identity of its own -- splitting leaves the same buffer
in both halves, so a focus naming a buffer could not say which half it meant.

Weights are natural numbers and shares are taken by running sums, so the parts
of a split add up to exactly the region. A fraction of the whole would give Lean,
Rust and TypeScript three answers at the last digit, and a differential suite
reporting rounding is one everybody learns to ignore.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.Screen

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Produce

/-! ## The value -/

/-- Which way a split divides its region. -/
inductive Axis where
  /-- Parts side by side; the region's width is shared. -/
  | across
  /-- Parts stacked; the region's height is shared. -/
  | down
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Where to move the focus. -/
inductive Direction where
  | left
  | right
  | up
  | down
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A region of the screen, in characters. -/
structure Rect where
  left : Nat
  top : Nat
  width : Nat
  height : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
Which buffers are on screen, and where.

A pane carries its own identity as well as the buffer it shows.
`REQ-SCREEN.split_keeps_the_buffer` puts one buffer in two panes, so a name that
was the buffer's could not tell them apart.
-/
inductive Layout where
  /-- One buffer, filling its region. -/
  | pane (id : String) (buffer : String)
  /-- A region divided between parts, each weighted. -/
  | split (axis : Axis) (parts : List (Nat × Layout))
  deriving Repr, Inhabited

/-! ### Encoding

`Layout` is recursive, so its encoding is written rather than derived. The shape
is the one serde produces for an externally tagged Rust enum with named fields,
because the Rust implementation is the other side of every comparison. -/

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

/--
Everything a session holds: the buffers opened, the layout placing some of them,
which pane has focus, and where the next pane's identity comes from.

`nextPane` is a counter rather than a clock or a random number, so that minting
an identity stays a function of the value (`ARCH-DETERMINISM`).

@models REQ-SCREEN.screen_is_a_value
-/
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

/-! ## Reading a layout -/

mutual
  /-- Every pane a layout holds, as an identity and the buffer it shows, left to
  right and top to bottom. -/
  def panes : Layout → List (String × String)
    | Layout.pane id buffer => [(id, buffer)]
    | Layout.split _ parts => panesOfParts parts

  private def panesOfParts : List (Nat × Layout) → List (String × String)
    | [] => []
    | (_, layout) :: rest => panes layout ++ panesOfParts rest
end

/-- The identities of a layout's panes. -/
def paneIds (layout : Layout) : List String :=
  (panes layout).map (fun pane => pane.1)

/-- The buffers a layout places. -/
def shownBuffers (layout : Layout) : List String :=
  (panes layout).map (fun pane => pane.2)

private def hasDuplicate : List String → Bool
  | [] => false
  | name :: rest => rest.contains name || hasDuplicate rest

/--
No two panes share an identity.

@models REQ-SCREEN.panes_are_distinct
-/
def distinctPanes (layout : Layout) : Bool :=
  !hasDuplicate (paneIds layout)

/--
The screen holds together: every buffer placed is opened, the focused pane is
one the layout places, and no two panes share an identity.

Stated as one function because the three are one question -- whether a frontend
can draw this value -- and answering them separately would let a caller check
two and ship the third.

@models REQ-SCREEN.every_pane_is_opened
@models REQ-SCREEN.focus_is_placed
-/
def coherent (screen : Screen) : Bool :=
  let openedIds := screen.opened.map (fun buffer => buffer.id)
  (shownBuffers screen.layout).all (fun buffer => openedIds.contains buffer)
    && (paneIds screen.layout).contains screen.focus
    && distinctPanes screen.layout

/-! ## Placing

Shares are taken by running sums: the boundary after part `i` is
`extent * (w₁ + … + wᵢ) / total`, and a part gets the difference between its
boundary and the one before it. The last boundary is `extent * total / total`,
which is `extent` exactly -- so the parts tile the region with no gap and no
overlap, whatever the weights.
-/

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

/--
How much of `extent` each weight gets.

Weights that are all zero share the region evenly rather than taking none of it:
a layout whose parts all weigh nothing is still a layout, and answering with
nothing would leave a frontend with a region and no pane to draw in it.
-/
def shares (extent : Nat) (weights : List Nat) : List Nat :=
  let total := sumOf weights
  if total = 0 then
    runningCuts extent weights.length 0 (weights.map (fun _ => 1))
  else
    runningCuts extent total 0 weights

/-! ### Parts that place nothing

A split may hold a part that draws nothing -- a split of no parts, or one whose
own parts are all like that. Giving such a part a share of the region leaves a
hole: it is handed extent and produces no rectangle, and the region is no longer
covered. So a part is weighted by what it will place.

Found by the law rather than by review: `place` agreed with its Rust twin on
every case, because both left the same hole. -/

/-- One for a part that will place a pane, zero for one that will not. -/
private def placingMask : List (Nat × Layout) → List Nat
  | [] => []
  | (_, layout) :: rest =>
    (if (panes layout).isEmpty then 0 else 1) :: placingMask rest

/-- Each part's own weight, or zero where the part will place nothing. -/
private def effectiveWeights : List (Nat × Layout) → List Nat
  | [] => []
  | (weight, layout) :: rest =>
    (if (panes layout).isEmpty then 0 else weight) :: effectiveWeights rest

/-- What to divide the region by.

The parts' weights, with a part that places nothing zeroed. When every part
that *does* place weighs nothing, the region is divided evenly between them --
which is `shares`' own rule, applied to the mask rather than to the weights, so
that a part placing nothing is still given nothing. -/
private def basisOf (parts : List (Nat × Layout)) : List Nat :=
  let weighted := effectiveWeights parts
  if sumOf weighted = 0 then placingMask parts else weighted

mutual
  /--
  Where each pane goes, given the region the layout fills.

  A layout that places any pane at all tiles its region: the shares of a split
  add up to the region's extent along its axis, each part is placed where the
  one before it ended, and a part that would place nothing is given nothing. A
  layout with no panes places nothing, and there is no tiling to speak of.

  @models REQ-SCREEN.layout_tiles_the_region
  -/
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

/-! ## Changing what is shown -/

private def isOpened (screen : Screen) (id : String) : Bool :=
  (screen.opened.map (fun buffer => buffer.id)).contains id

mutual
  /-- Put `buffer` in the pane named `pane`, leaving every other pane alone. -/
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

/--
Show an opened buffer in the focused pane.

A buffer that is not opened is not shown: the answer is the screen unchanged,
because showing something the session does not hold would put a pane in front of
a buffer nothing produced.
-/
def showBuffer (id : String) (screen : Screen) : Screen :=
  if isOpened screen id then
    { screen with layout := setBuffer screen.focus id screen.layout }
  else
    screen

/--
Open a buffer and show it.

A buffer already opened is *not* opened again and the one already held is kept,
so what it has accumulated survives being left and returned to. That is the
whole difference between a buffer and a panel that is rebuilt every time it
becomes visible.

@models REQ-SCREEN.opened_outlives_shown
@models REQ-SCREEN.station_opens_a_buffer
-/
def openBuffer (buffer : Buffer) (screen : Screen) : Screen :=
  if isOpened screen buffer.id then
    showBuffer buffer.id screen
  else
    showBuffer buffer.id { screen with opened := screen.opened ++ [buffer] }

/-! ## The workbench -/

/-- The pane that holds what you choose from: listings. -/
def explorer : String := "explorer"
/-- The pane that holds what you read and edit: files and reviews. -/
def document : String := "document"
/-- The pane that holds what you consult beside it: menus and records. -/
def side : String := "side"

/-- Add a buffer to the opened set unless one of its identity is already held. -/
private def hold (buffer : Buffer) (opened : List Buffer) : List Buffer :=
  if (opened.map (fun held => held.id)).contains buffer.id then opened else opened ++ [buffer]

/--
The screen a session opens on: three panes side by side -- the explorer, the
document and the side panel -- weighted one, two, one, with the focus on the
explorer.

The panes are named rather than minted, so that where a buffer belongs can be
said of a pane a person may since have resized, split beside or closed. A
minted identity is `pane` and a number, so it never collides with these.

@models REQ-SCREEN.workbench_has_three_places
-/
def workbench (listing : Buffer) (document_ : Buffer) (side_ : Buffer) : Screen :=
  Screen.mk
    (hold side_ (hold document_ (hold listing [])))
    (Layout.split Axis.across
      [(1, Layout.pane explorer listing.id),
       (2, Layout.pane document document_.id),
       (1, Layout.pane side side_.id)])
    explorer
    0

/-- Records read like a file — an opened requirement, a judge's prompt, lists of
places with their lines — named by how their titles start. -/
def documentRecords : List String :=
  ["requirement ", "judge ", "context ", "coverage ", "definitions of ", "uses of ", "search ", "keys"]

/-- The pane a buffer of this kind belongs in. -/
def homeOf : BufferKind → String
  | BufferKind.directory _ => explorer
  | BufferKind.file _ => document
  | BufferKind.review _ => document
  | BufferKind.menu _ => side
  -- A record read like a file is shown where files are.
  | BufferKind.record title =>
    if documentRecords.any (fun lead => title.startsWith lead) then document else side

/--
Where a buffer of this kind is shown: its home pane while the layout places one,
and the focused pane once it does not.

The fallback keeps a person's own arrangement theirs: close the side panel and a
station opens where you are, rather than the panel being brought back.

@models REQ-SCREEN.buffer_goes_home
-/
def destination (kind : BufferKind) (screen : Screen) : String :=
  if (paneIds screen.layout).contains (homeOf kind) then homeOf kind else screen.focus

/--
A buffer goes either to a pane the layout places, or where the focus already
is: it never sends the focus somewhere nothing is.

Part of `REQ-SCREEN.buffer_goes_home` and not the whole of it -- which pane a
kind belongs in is earned differentially -- so it is not claimed as a proof of
the clause.
-/
theorem destination_is_placed_or_stays (kind : BufferKind) (screen : Screen) :
    (paneIds screen.layout).contains (destination kind screen) = true ∨
      destination kind screen = screen.focus := by
  unfold destination
  split
  · left; assumption
  · right; rfl

/-! ## Splitting, closing, resizing -/

mutual
  /-- Replace the *first* pane named `pane` with a split of two, both showing
  what it showed, the new half taking the identity `fresh`.

  The first and not every one. Two panes may carry the same identity -- nothing
  in the type forbids it, which is why `panes_are_distinct` is a clause and not
  an invariant -- and splitting both would add two panes for one request and
  mint one identity for both new halves. Answering `none` when nothing matched
  is what stops the traversal at the first hit. -/
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

/-- The first `paneN` from `start` upward that nothing already placed is called.

`fuel` is one more than the number of panes placed, which is enough: among that
many consecutive candidates at most `fuel - 1` can be taken. -/
private def freeFrom (taken : List String) (start : Nat) : Nat → Nat
  | 0 => start
  | Nat.succ fuel =>
    if taken.contains ("pane" ++ toString start) then freeFrom taken (start + 1) fuel
    else start

/--
Divide the focused pane, leaving its buffer in both halves.

Neither half is empty, so a split never produces a region with nothing to draw
in it. The new pane takes its identity from the counter the screen carries, and
the focus stays where it was -- splitting is not a way of moving.

A focus on no pane splits nothing and mints no identity: a counter advanced by
a request that did nothing would leave a gap in the names for no reason.

@models REQ-SCREEN.split_keeps_the_buffer
-/
def splitFocus (axis : Axis) (screen : Screen) : Screen :=
  -- The counter alone is not enough to mint a name nobody has. `nextPane` is a
  -- field of a value anyone can build, so a screen with `pane0` placed and
  -- `nextPane := 0` is coherent by every stated invariant and one split away
  -- from two panes called `pane0` -- which `panes_are_distinct` forbids and
  -- which makes a pane unnameable, the thing pane identities exist for.
  --
  -- So the counter is advanced past whatever is already placed before it is
  -- used. Found by an independent review of this clause; the generator could
  -- not have found it, because it draws pane ids from an alphabet that never
  -- spells `paneN`.
  let taken := paneIds screen.layout
  let free := freeFrom taken screen.nextPane (taken.length + 1)
  match splitAt screen.focus ("pane" ++ toString free) axis screen.layout with
  | none => screen
  | some layout => { screen with layout := layout, nextPane := free + 1 }

mutual
  /-- Drop every pane showing `buffer`, collapsing a split left with one part.

  Answers `none` when nothing is left, which is how a caller learns that
  removing would have emptied the region. -/
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

/--
Close a buffer: drop it from the opened set, and give the region of every pane
showing it to the rest of the layout.

Two states are refused rather than entered. Closing the last opened buffer
answers unchanged, because a session with nothing opened has no layout to draw.
Closing one that fills every pane keeps the opened set's next buffer on screen,
because a region with no pane in it is not something a frontend can render.

@models REQ-SCREEN.close_collapses_the_pane
-/
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

/-! ### Resizing

A divider is named by the pane beside it: the shell knows which pane's edge a
pointer grabbed, and a keypress means the edge beside the focus. Weight moves
between the two parts either side and no third part is touched.
-/

private def holdsPane (pane : String) (layout : Layout) : Bool :=
  (paneIds layout).contains pane

/-- Move `amount` of weight from the part at `index + 1` to the part at `index`,
or the other way when it is negative -- or refuse, changing nothing, when
either side would end below one.

A refusal rather than a cut. A cut stopped at "what the giver can spare above
one", which is nothing for a part already at zero, and so left that part at
zero: the floor held only for parts that started on it. A drag arrives a
column at a time, so refusing the step that would cross the floor is where a
drag stops anyway. -/
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
  /-- Resize the divider beside the pane named `pane`.

  The divider *after* the part holding it, unless that part is last, in which
  case the divider before it -- so the last pane of a split can still be
  resized. -/
  def resizeAt (pane : String) (amount : Int) : Layout → Layout
    | Layout.pane id buffer => Layout.pane id buffer
    | Layout.split axis parts =>
      match indexHolding pane 0 parts with
      | none => Layout.split axis parts
      | some index =>
        if index + 1 < parts.length then
          Layout.split axis (shiftAt index amount parts)
        else if index = 0 then
          -- One part, or the pane is in the only part there is: nothing to move
          -- weight against, so descend and let an inner split answer.
          Layout.split axis (resizeAtParts pane amount parts)
        else
          Layout.split axis (shiftAt (index - 1) (-amount) parts)

  private def resizeAtParts (pane : String) (amount : Int) :
      List (Nat × Layout) → List (Nat × Layout)
    | [] => []
    | (weight, layout) :: rest =>
      (weight, resizeAt pane amount layout) :: resizeAtParts pane amount rest
end

/--
Grow or shrink the focused pane against its neighbour.

@models REQ-SCREEN.resize_moves_one_divider
@models REQ-SCREEN.resize_has_a_floor
-/
def resizeFocus (amount : Int) (screen : Screen) : Screen :=
  { screen with layout := resizeAt screen.focus amount screen.layout }

/-! ## Moving the focus -/

private def edgeOf (rect : Rect) : Direction → Nat
  | Direction.left => rect.left
  | Direction.right => rect.left + rect.width
  | Direction.up => rect.top
  | Direction.down => rect.top + rect.height

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

/-- Of two candidates, the one a move should land on: nearest in the direction
travelled, and when they are equally near, the earlier of the two in reading
order. -/
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

/--
The pane a move in a direction lands on.

The nearest pane wholly beyond the focused one in that direction and overlapping
it across the direction of travel. When there is none -- at the edge of the
screen, or with nothing alongside -- the focus stays where it is, because a move
that wrapped round would take a person somewhere they did not point at.

@models REQ-SCREEN.focus_follows_geometry
-/
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

/-! ## One way in

Every change to the arrangement goes through one function. A terminal resizes
with a key and a window resizes by dragging a divider; the easy thing is for
each frontend to work the new weights out itself, and then there are two
answers to what a resize does and only one is ever tested. A drag becomes an
amount before it arrives here, not after. -/

/-- A change to the arrangement, named rather than performed. -/
inductive Arrangement where
  /-- Divide the focused pane. -/
  | split (axis : Axis)
  /-- Close the buffer the focused pane shows. -/
  | close
  /-- Move the focus in a direction, which is what a key does. -/
  | focus (dir : Direction)
  /-- Focus a named pane, which is what a pointer does: clicking in a pane, or
  grabbing the divider on its edge. A pane the layout does not place is not
  focused, because a focus off the layout is one no frontend can draw. -/
  | focusPane (pane : String)
  /-- Grow the focused pane by this much, or shrink it when negative. -/
  | resize (amount : Int)
  /-- Show an already-opened buffer in the focused pane. -/
  | showBuffer (buffer : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The buffer the focused pane shows, if the focus is on a pane at all. -/
private def focusedBuffer (screen : Screen) : Option String :=
  match (panes screen.layout).find? (fun pane => pane.1 = screen.focus) with
  | none => none
  | some pane => some pane.2

/--
Make a change to the arrangement.

The region is an argument because moving the focus is a question about
geometry, and geometry is not in the screen: the same layout in a narrow
terminal and a wide window has the panes in different places.

@models REQ-SCREEN.one_arrangement_path
-/
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

/-! ## The strip and the stations -/

/-- The last part of a path: the name a tab shows. -/
def lastSegment (path : String) : String :=
  (path.splitOn "/").getLastD path

/-- What a buffer is called on its tab. The identity stays the buffer's id; this
is only what a person reads. -/
def titleOf (buffer : Buffer) : String :=
  match buffer.kind with
  | BufferKind.file path => lastSegment path
  | BufferKind.directory _ => "files"
  | BufferKind.review target => "review " ++ lastSegment target
  | BufferKind.menu title => title
  | BufferKind.record title => title

/-- The row of the buffer the focused pane shows is a heading: the active tab. -/
private def markFocused (focused : Option String) : List Buffer → List Span → List Span
  | buffer :: rest, span :: spans =>
    (if some buffer.id = focused then { span with role := Role.heading } else span)
      :: markFocused focused rest spans
  | _, spans => spans

/-- The folders a path sits in, outermost first. -/
def foldersOf (path : String) : List String :=
  (path.splitOn "/").dropLast

/-- The last `n` of a list. -/
def lastOf (n : Nat) (xs : List String) : List String :=
  xs.drop (xs.length - n)

/-- The fewest innermost folders of `path` that no other path's folders end
with: `tui/src` beside `desktop/src`. -/
def distinguishing (path : String) (others : List String) : String :=
  let mine := foldersOf path
  let theirs := others.map foldersOf
  let apart := fun n => theirs.all fun folders => lastOf n folders != lastOf n mine
  let n := ((List.range mine.length).map (· + 1)).find? apart |>.getD mine.length
  "/".intercalate (lastOf n mine)

/-- A tab's title among the others opened: a file whose name another opened
file shares says the folders that tell it apart, so two `main.rs` read
`main.rs · tui/src` and `main.rs · desktop/src`. -/
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

/--
The opened set, as a buffer.

A rendering of `opened` rather than a list kept beside it: a strip maintained
separately is a second answer to what the session holds, which is the failure
`REQ-VIEW` prevents one level up. Each row carries `screen.show <id>`: its text
is a number and a title, not the buffer's identity, so the action names the
buffer rather than leaving the shell to work it out from the row (which the
first review found the shell doing, around `dispatch`).

@models REQ-SCREEN.strip_is_the_opened_set
-/
def strip (screen : Screen) : Buffer :=
  let rows := menuBuffer "opened" (stripEntries screen.opened 1 screen.opened)
  { rows with spans := markFocused (focusedBuffer screen) screen.opened rows.spans }

/-- The five stations, in the order they are always in. The requirements are
the design's: a flat list of them beside it said nothing the graph does not.

Each row carries its own action rather than a shared one taking the row as a
target, so that a station is reachable from a bare keyboard as well as from a
pointer (see `TraceLean.Act`). -/
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
     action := some "screen.station.history" }]

/--
The stations, as a buffer.

A function of the screen that reads none of it, which is the clause: a station
list derived from the current project is unreachable exactly when no project is
open, and that is when the station that opens one is most needed.

@models REQ-SCREEN.stations_are_constant
-/
def stations (screen : Screen) : Buffer :=
  -- Taken and then ignored, deliberately. `stations_are_constant` is the claim
  -- that the answer does not depend on this argument, and a function that did
  -- not take it could not be compared against one that might.
  let _ := screen
  menuBuffer "stations" stationEntries

/--
What a station stands for: the kind of buffer pressing it produces.

One function rather than a case in each frontend, for the reason every producer
is in the core: a window that knew `requirements` meant the requirement index
and a terminal that did not would be two editors.

A name that is not a station's answers `none`. That is not a gap -- it is how
the dispatcher refuses an action nobody declared, and it is what makes the
claim below falsifiable rather than true by construction.

@models REQ-SCREEN.station_produces_a_buffer
-/
def stationKind (station : String) : Option BufferKind :=
  if station == "project" then some (BufferKind.directory ".")
  else if station == "design" then some (BufferKind.menu "design")
  else if station == "sandbox" then some (BufferKind.record "sandbox")
  else if station == "history" then some (BufferKind.record "history")
  else if station == "trace" then some (BufferKind.record "trace")
  else none

/-- Every station on the bar produces something.

A station that is drawn and does nothing is the worst of the two failures
available here: the screen says it is there and pressing it is silence.

@proves REQ-SCREEN.station_produces_a_buffer -/
theorem every_station_produces :
    stationEntries.all (fun entry => (stationKind entry.key).isSome) = true := by
  native_decide

/-! ## What is proved

Two clauses are laws rather than agreements, and are discharged here rather than
by a run. The rest are earned differentially -- see `.tracelean/drt.json`.
-/

/--
The stations do not depend on the screen.

@proves REQ-SCREEN.stations_are_constant
-/
theorem stations_constant (a b : Screen) : stations a = stations b := rfl

/--
Showing a buffer changes no buffer the session holds: it moves what a pane
points at, and produces nothing.

@proves REQ-SCREEN.opened_outlives_shown
-/
theorem show_keeps_opened (id : String) (screen : Screen) :
    (showBuffer id screen).opened = screen.opened := by
  unfold showBuffer
  split <;> rfl

/--
Opening a buffer already opened keeps the one already held, rather than
replacing it with the one offered.

@proves REQ-SCREEN.opened_outlives_shown
-/
theorem open_twice_holds_one (buffer : Buffer) (screen : Screen) :
    isOpened screen buffer.id = true →
    (openBuffer buffer screen).opened = screen.opened := by
  intro held
  unfold openBuffer
  simp [held, show_keeps_opened]

/-! ## Pinned

A specification says which answers are right, read from the clause rather than
from the function; it pins the model when the model meets it and no layout has
two right answers. Here because the model leans on a private helper. -/

/-- Whether a layout's panes are distinct, answered rightly: yes exactly when
no identity occurs twice among the identities of its panes.

@specifies REQ-SCREEN.panes_are_distinct -/
def PanesDistinct (layout : Layout) (answer : Bool) : Prop :=
  answer = true ↔ List.Nodup (paneIds layout)

private theorem no_duplicate_iff_nodup (names : List String) :
    hasDuplicate names = false ↔ List.Nodup names := by
  induction names
  case nil => simp [hasDuplicate]
  case cons name rest ih => simp [hasDuplicate, List.nodup_cons, ih]

/-- `distinctPanes` answers whether no two panes share an identity, and that
question has one answer.

@pins REQ-SCREEN.panes_are_distinct -/
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

/-- Whether a layout's panes are distinct is answered as its specification
says, for every layout.

@proves REQ-SCREEN.panes_are_distinct -/
theorem distinct_panes_answers (layout : Layout) : PanesDistinct layout (distinctPanes layout) :=
  panes_distinct_pinned.1 layout

end TraceLean.Screen
