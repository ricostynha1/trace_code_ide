import Lean
import TraceLean.View

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
  drawn : Option Cell
  deriving Repr, Inhabited, ToJson, FromJson

def number (piece : String) (default : Nat) : Nat :=
  if piece.isEmpty || !piece.all Char.isDigit then default
  else piece.foldl (fun n c => n * 10 + (c.toNat - '0'.toNat)) 0

def hexChar (n : Nat) : Char :=
  if n < 10 then Char.ofNat ('0'.toNat + n) else Char.ofNat ('a'.toNat + n - 10)

def two (n : Nat) : String :=
  let m := min n 255
  String.mk [hexChar (m / 16), hexChar (m % 16)]

def hex (r g b : Nat) : String := "#" ++ two r ++ two g ++ two b

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

def down (rows columns : Nat) (s : State) : State :=
  if s.row + 1 < rows then { s with row := s.row + 1 }
  else if rows > 0 then { s with cells := s.cells.drop 1 ++ [List.replicate columns blank] }
  else s

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

def afterEscape (rows columns : Nat) (s : State) : List Char → State × List Char
  | '[' :: rest =>
    let (params, last, after) := control [] rest
    (sequence rows columns s (String.mk params) last, after)
  | c :: rest =>
    if c == '(' || c == ')' || c == '*' || c == '+' then (s, rest.drop 1) else (s, rest)
  | [] => (s, [])

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

def miscoloured (buffer : Buffer) (cells : List (List Cell)) (looks : List Look) : List Miscoloured :=
  walk buffer.spans cells looks buffer.text.toList 0 0 0

/-- @models REQ-LOOK.roles_drawn_in_theme_colours -/
def drawnWrong (buffer : Buffer) (painted : String) (rows columns : Nat) (looks : List Look) :
    List Miscoloured :=
  miscoloured buffer (grid painted rows columns) looks

def looks : List Look := [⟨.heading, some "#ff0000", true⟩]

end TraceLean.Cells
