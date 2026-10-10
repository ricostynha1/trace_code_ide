import TraceLean.Produce

/-!
# Panels laid out a line at a time

The panels that are laid out rather than parsed -- the sandbox station, an
opened requirement -- are built a line at a time, each piece with its role and
actions, long texts wrapped to the width the pane has. This is that builder,
shared by their models as `Lines` is shared by their implementations.
-/

namespace TraceLean.Layout

open TraceLean.View
open TraceLean.Produce

/-- A piece of a line: its text, its role, and what it does. -/
abbrev Piece := String × Role × List String

structure Lines where
  text : String
  spans : List Span
  width : Nat

/-- No line is narrower than twelve characters. -/
def start (width : Nat) : Lines := { text := "", spans := [], width := max width 12 }

/-- One line of pieces; a piece with a role or an action is a span. -/
def line (out : Lines) (pieces : List Piece) : Lines :=
  let begun := if out.text.isEmpty then out.text else out.text ++ "\n"
  let done := pieces.foldl (fun (acc : String × List Span) (p : Piece) =>
    let at_ := acc.1.length
    let spans :=
      if p.2.1 != Role.plain || !p.2.2.isEmpty then
        acc.2 ++ [{ start := at_, stop := at_ + p.1.length, role := p.2.1, actions := p.2.2 }]
      else acc.2
    (acc.1 ++ p.1, spans)) (begun, out.spans)
  { out with text := done.1, spans := done.2 }

def blank (out : Lines) : Lines := line out []

/-- Where a row too long for `room` is cut: after its last space within
`room + 1` characters if that is in the row's second half, else at `room`. -/
def cutAt (rest : List Char) (room : Nat) : Nat :=
  let window := rest.take (room + 1)
  let spaces := (List.range window.length).filter (fun i => window.getD i 'x' == ' ')
  match spaces.getLast? with
  | some space => if space ≥ room / 2 then space + 1 else room
  | none => room

/-- One paragraph's rows; each takes at least one character. -/
def rowsOf (room : Nat) : Nat → List Char → List String
  | 0, _ => []
  | fuel + 1, rest =>
    if rest.length ≤ room then [String.mk rest]
    else
      let cut := cutAt rest room
      (String.mk (rest.take cut)).trimRight :: rowsOf room fuel (rest.drop cut)

/-- Rows of at most `room` characters; a line break in the text starts a row. -/
def breakWords (text : String) (room : Nat) : List String :=
  (text.splitOn "\n").bind fun paragraph => rowsOf room (paragraph.length + 1) paragraph.toList

/-- A long text wrapped to the width, every row with the same role and
actions; an empty one is a line of its indent alone. -/
def wrapped (out : Lines) (indent text : String) (role : Role) (actions : List String) : Lines :=
  let room := max (out.width - indent.length) 8
  if text.isEmpty then line out [(indent, Role.plain, [])]
  else (breakWords text room).foldl (fun o row => line o [(indent, Role.plain, []), (row, role, actions)]) out

/-- The lines as a record titled `title`. -/
def finish (out : Lines) (title : String) : Buffer :=
  { id := "record:" ++ title, kind := .record title, text := out.text,
    spans := tidy out.text.length out.spans }

/-- `text` padded with spaces to `width` characters, as `{:<width}` pads. -/
def padRight (text : String) (width : Nat) : String :=
  text ++ String.mk (List.replicate (width - text.length) ' ')

end TraceLean.Layout
