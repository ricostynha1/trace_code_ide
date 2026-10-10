import Lean

/-!
# Contrast between the colours a theme draws together

Models `REQ-LOOK.contrast_sufficient`: every pair of colours the theme declares
as drawn together has at least the contrast ratio its kind needs, by WCAG's
relative luminance, computed from the theme itself. A pair below its minimum is
a finding unless the theme waives it with a reason; a waiver on a pair that
meets its minimum is a finding too, so waivers do not outlive their cause.
-/

namespace TraceLean.Contrast

open Lean (ToJson FromJson)

/-- Two colours drawn together: the keys of the text's and the background's
colours, and the least ratio allowed, in hundredths (450 for text). -/
structure Pair where
  text : String
  on : String
  least : Nat
  waived : Option String
  deriving Repr, Inhabited, ToJson, FromJson

structure Finding where
  text : String
  on : String
  ratio : Option Nat
  least : Nat
  problem : String
  deriving Repr, Inhabited, ToJson, FromJson

def hexDigit (c : Char) : Option Nat :=
  if '0' ≤ c && c ≤ '9' then some (c.toNat - '0'.toNat)
  else if 'a' ≤ c && c ≤ 'f' then some (c.toNat - 'a'.toNat + 10)
  else if 'A' ≤ c && c ≤ 'F' then some (c.toNat - 'A'.toNat + 10)
  else none

def byte (hi lo : Char) : Option Nat :=
  match hexDigit hi, hexDigit lo with
  | some a, some b => some (a * 16 + b)
  | _, _ => none

/-- `#rrggbb` as its three channels; anything else is no colour. -/
def channels (colour : String) : Option (Nat × Nat × Nat) :=
  match colour.toList with
  | ['#', r1, r2, g1, g2, b1, b2] =>
    match byte r1 r2, byte g1 g2, byte b1 b2 with
    | some r, some g, some b => some (r, g, b)
    | _, _, _ => none
  | _ => none

def linear (n : Nat) : Float :=
  let c := Float.ofNat n / 255.0
  if c ≤ 0.04045 then c / 12.92 else Float.pow ((c + 0.055) / 1.055) 2.4

def luminance (rgb : Nat × Nat × Nat) : Float :=
  0.2126 * linear rgb.1 + 0.7152 * linear rgb.2.1 + 0.0722 * linear rgb.2.2

/-- The contrast ratio of two colours in hundredths, rounded down. -/
def ratio (a b : String) : Option Nat :=
  match channels a, channels b with
  | some x, some y =>
    let lx := luminance x
    let ly := luminance y
    let hi := if lx < ly then ly else lx
    let lo := if lx < ly then lx else ly
    some ((((hi + 0.05) / (lo + 0.05)) * 100.0).floor.toUInt64.toNat)
  | _, _ => none

def colourOf (colours : List (String × String)) (key : String) : Option String :=
  (colours.find? (·.1 == key)).map (·.2)

def judge (colours : List (String × String)) (pair : Pair) : List Finding :=
  let found := fun (r : Option Nat) (problem : String) =>
    [({ text := pair.text, on := pair.on, ratio := r, least := pair.least, problem := problem } : Finding)]
  match colourOf colours pair.text, colourOf colours pair.on with
  | some a, some b =>
    match ratio a b with
    | none => found none "not a colour"
    | some r =>
      if r < pair.least then (if pair.waived.isSome then [] else found (some r) "below its minimum")
      else if pair.waived.isSome then found (some r) "waiver unused"
      else []
  | _, _ => found none "unknown colour"

/-- Everything wrong with the pairs a theme declares, in their order.

@models REQ-LOOK.contrast_sufficient -/
def findings (pairs : List Pair) (colours : List (String × String)) : List Finding :=
  pairs.bind (judge colours)

end TraceLean.Contrast
