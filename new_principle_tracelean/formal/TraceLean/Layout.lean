import TraceLean.Produce

namespace TraceLean.Layout

open TraceLean.View
open TraceLean.Produce

abbrev Piece := String × Role × List String

structure Lines where
  text : String
  spans : List Span
  width : Nat

def start (width : Nat) : Lines := { text := "", spans := [], width := max width 12 }

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

def cutAt (rest : List Char) (room : Nat) : Nat :=
  let window := rest.take (room + 1)
  let spaces := (List.range window.length).filter (fun i => window.getD i 'x' == ' ')
  match spaces.getLast? with
  | some space => if space ≥ room / 2 then space + 1 else room
  | none => room

def rowsOf (room : Nat) : Nat → List Char → List String
  | 0, _ => []
  | fuel + 1, rest =>
    if rest.length ≤ room then [String.mk rest]
    else
      let cut := cutAt rest room
      (String.mk (rest.take cut)).trimRight :: rowsOf room fuel (rest.drop cut)

def breakWords (text : String) (room : Nat) : List String :=
  (text.splitOn "\n").bind fun paragraph => rowsOf room (paragraph.length + 1) paragraph.toList

def wrapped (out : Lines) (indent text : String) (role : Role) (actions : List String) : Lines :=
  let room := max (out.width - indent.length) 8
  if text.isEmpty then line out [(indent, Role.plain, [])]
  else (breakWords text room).foldl (fun o row => line o [(indent, Role.plain, []), (row, role, actions)]) out

def finish (out : Lines) (title : String) : Buffer :=
  { id := "record:" ++ title, kind := .record title, text := out.text,
    spans := tidy out.text.length out.spans }

def padRight (text : String) (width : Nat) : String :=
  text ++ String.mk (List.replicate (width - text.length) ' ')

end TraceLean.Layout
