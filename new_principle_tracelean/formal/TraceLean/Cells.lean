import Lean
import TraceLean.View

/-!
# What a terminal drew, cell by cell, and whether it is the theme's colour

Models `REQ-LOOK.roles_drawn_in_theme_colours`. The bytes a terminal received
become a grid of cells, each with its character and the style it was drawn in;
only the sequences a frontend here sends are understood (clearing the screen,
placing the cursor, weight, dimming, reverse video and 24-bit colour). Every
character of a buffer is then looked up in the cell it landed in, and one whose
cell is not in the look its role has -- the role of the first span covering it
-- is reported.
-/

namespace TraceLean.Cells

open Lean (ToJson FromJson)
open TraceLean.View

structure Cell where
  text : String
  fg : Option String
  bg : Option String
  bold : Bool
  dim : Bool
  reverse : Bool
  deriving Repr, Inhabited, BEq, ToJson, FromJson

def blank : Cell :=
  { text := " ", fg := none, bg := none, bold := false, dim := false, reverse := false }

/-- How a role is meant to look. A role with no look is drawn plain. -/
structure Look where
  role : Role
  fg : Option String
  bold : Bool
  deriving Repr, Inhabited, ToJson, FromJson

structure Miscoloured where
  line : Nat
  column : Nat
  text : String
  role : Role
  expected : Option String
  expectedBold : Bool
  /-- What the cell holds; `none` when the character fell off the grid. -/
  drawn : Option Cell
  deriving Repr, Inhabited, ToJson, FromJson

/-- A run of decimal digits, or the default for anything else. -/
def number (piece : String) (default : Nat) : Nat :=
  if piece.isEmpty || !piece.all Char.isDigit then default
  else piece.foldl (fun n c => n * 10 + (c.toNat - '0'.toNat)) 0

def hexChar (n : Nat) : Char :=
  if n < 10 then Char.ofNat ('0'.toNat + n) else Char.ofNat ('a'.toNat + n - 10)

def two (n : Nat) : String :=
  let m := min n 255
  String.mk [hexChar (m / 16), hexChar (m % 16)]

def hex (r g b : Nat) : String := "#" ++ two r ++ two g ++ two b

/-- The style a graphic rendition leaves, code by code. A colour that is not
complete ends the sequence; a palette index is skipped. -/
def render (style : Cell) : List Nat → Cell
  | [] => style
  | 0 :: rest => render { blank with text := style.text } rest
  | 1 :: rest => render { style with bold := true } rest
  | 2 :: rest => render { style with dim := true } rest
  | 22 :: rest => render { style with bold := false, dim := false } rest
  | 7 :: rest => render { style with reverse := true } rest
  | 27 :: rest => render { style with reverse := false } rest
  | 39 :: rest => render { style with fg := none } rest
  | 49 :: rest => render { style with bg := none } rest
  | 38 :: 2 :: r :: g :: b :: rest => render { style with fg := some (hex r g b) } rest
  | 48 :: 2 :: r :: g :: b :: rest => render { style with bg := some (hex r g b) } rest
  | 38 :: 5 :: _ :: rest => render style rest
  | 48 :: 5 :: _ :: rest => render style rest
  | 38 :: _ => style
  | 48 :: _ => style
  | _ :: rest => render style rest

structure State where
  cells : List (List Cell)
  style : Cell
  row : Nat
  column : Nat
  wraps : Bool

def fresh (rows columns : Nat) : List (List Cell) :=
  List.replicate rows (List.replicate columns blank)

/-- The next line, scrolling the screen up when the cursor is on the last. -/
def down (rows columns : Nat) (s : State) : State :=
  if s.row + 1 < rows then { s with row := s.row + 1 }
  else if rows > 0 then { s with cells := s.cells.drop 1 ++ [List.replicate columns blank] }
  else s

/-- A character drawn where the cursor is. Past the right edge it wraps to the
next line, or with wrapping off overwrites the last column; the cursor moves on
either way. -/
def put (rows columns : Nat) (s : State) (c : Char) : State :=
  let s :=
    if s.column ≥ columns then
      if s.wraps then { down rows columns s with column := 0 } else { s with column := columns - 1 }
    else s
  let cells :=
    if s.row < rows && s.column < columns then
      s.cells.set s.row ((s.cells.getD s.row []).set s.column { s.style with text := c.toString })
    else s.cells
  { s with cells := cells, column := s.column + 1 }

/-- The parameters of a control sequence, the letter ending it, and what
follows; no letter when the input ends first. -/
def control (params : List Char) : List Char → List Char × Option Char × List Char
  | [] => (params.reverse, none, [])
  | c :: rest => if c.isAlpha then (params.reverse, some c, rest) else control (c :: params) rest

def sequence (rows columns : Nat) (s : State) (params : String) : Option Char → State
  | some 'm' => { s with style := render s.style ((params.splitOn ";").map (number · 0)) }
  | some 'H' =>
    let pieces := params.splitOn ";"
    { s with row := min (max (number (pieces.getD 0 "") 1) 1) (max rows 1) - 1,
             column := min (max (number (pieces.getD 1 "") 1) 1) (max columns 1) - 1 }
  | some 'J' => if params == "2" then { s with cells := fresh rows columns } else s
  | some 'l' => if params == "?7" then { s with wraps := false } else s
  | some 'h' => if params == "?7" then { s with wraps := true } else s
  | _ => s

/-- What follows an escape (character 27): a control sequence, a character-set
designator and the byte it names, or one byte; and what is left after it. -/
def afterEscape (rows columns : Nat) (s : State) : List Char → State × List Char
  | '[' :: rest =>
    let (params, last, after) := control [] rest
    (sequence rows columns s (String.mk params) last, after)
  | c :: rest =>
    if c == '(' || c == ')' || c == '*' || c == '+' then (s, rest.drop 1) else (s, rest)
  | [] => (s, [])

/-- Each round consumes at least one character, so the input's length is
enough fuel. Escape and delete are named by their codes, 27 and 127. -/
def step (rows columns : Nat) : Nat → State → List Char → State
  | 0, s, _ => s
  | _, s, [] => s
  | fuel + 1, s, c :: rest =>
    if c.toNat == 27 then
      let (next, after) := afterEscape rows columns s rest
      step rows columns fuel next after
    else if c == '\r' then step rows columns fuel { s with column := 0 } rest
    else if c == '\n' then step rows columns fuel { down rows columns s with column := 0 } rest
    else if c < ' ' || c.toNat == 127 then step rows columns fuel s rest
    else step rows columns fuel (put rows columns s c) rest

/-- The cells a terminal of `rows` by `columns` shows after receiving
`painted`: wrapping past the right edge unless turned off, scrolling on a line
feed at the bottom, the cursor placed within the screen. A line feed starts
the next line at its first column. -/
def grid (painted : String) (rows columns : Nat) : List (List Cell) :=
  let chars := painted.toList
  let start : State := { cells := fresh rows columns, style := blank, row := 0, column := 0, wraps := true }
  (step rows columns (chars.length + 1) start chars).cells

def roleAt (spans : List Span) (offset : Nat) : Role :=
  match spans.find? (fun s => s.start ≤ offset && offset < s.stop) with
  | some s => s.role
  | none => .plain

def walk (spans : List Span) (cells : List (List Cell)) (looks : List Look) :
    List Char → Nat → Nat → Nat → List Miscoloured
  | [], _, _, _ => []
  | '\n' :: rest, offset, line, _ => walk spans cells looks rest (offset + 1) (line + 1) 0
  | c :: rest, offset, line, column =>
    let role := roleAt spans offset
    let (expected, expectedBold) :=
      match looks.find? (fun l => l.role == role) with
      | some l => (l.fg, l.bold)
      | none => (none, false)
    let drawn := (cells.get? line).bind (fun r => r.get? column)
    let right := match drawn with
      | some d => d.fg == expected && d.bold == expectedBold
      | none => false
    let here : List Miscoloured :=
      if right then [] else [{ line, column, text := c.toString, role, expected, expectedBold, drawn }]
    here ++ walk spans cells looks rest (offset + 1) line (column + 1)

/-- Every character of the buffer, drawn from the top-left of the cells, whose
cell is not in its role's look. -/
def miscoloured (buffer : Buffer) (cells : List (List Cell)) (looks : List Look) : List Miscoloured :=
  walk buffer.spans cells looks buffer.text.toList 0 0 0

/-- The two together: what a frontend that painted `painted` for `buffer`
drew in the wrong look.

@models REQ-LOOK.roles_drawn_in_theme_colours -/
def drawnWrong (buffer : Buffer) (painted : String) (rows columns : Nat) (looks : List Look) :
    List Miscoloured :=
  miscoloured buffer (grid painted rows columns) looks

/-- A control sequence: escape, `[`, its parameters and letter. -/
def csi (body : String) : String := String.mk [Char.ofNat 27] ++ "[" ++ body

/-- `ab`, its first character a heading. -/
def headed : Buffer :=
  { id := "b", kind := .record "b", text := "ab", spans := [⟨0, 1, .heading, []⟩] }

/-- A heading is bold red; plain has no look of its own. -/
def looks : List Look := [⟨.heading, some "#ff0000", true⟩]

/-- What is drawn is read back from the bytes the frontend sent: the heading in
bold red and the rest plain is right; the heading in the plain colour, or not
bold, is reported at its cell.

@proves REQ-LOOK.roles_drawn_in_theme_colours -/
theorem each_character_is_checked_where_it_landed :
    (drawnWrong headed (csi "1;38;2;255;0;0m" ++ "a" ++ csi "0m" ++ "b") 1 4 looks).length = 0 ∧
    ((drawnWrong headed ("a" ++ "b") 1 4 looks).map (fun m => (m.line, m.column))) = [(0, 0)] ∧
    ((drawnWrong headed (csi "38;2;255;0;0m" ++ "a" ++ csi "0m" ++ "b") 1 4 looks).map
      (fun m => (m.line, m.column))) = [(0, 0)] := by
  native_decide

end TraceLean.Cells
