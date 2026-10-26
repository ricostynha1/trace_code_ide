import Lean

namespace TraceLean.Contrast

open Lean (ToJson FromJson)

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

/-- @models REQ-LOOK.contrast_sufficient -/
def findings (pairs : List Pair) (colours : List (String × String)) : List Finding :=
  pairs.bind (judge colours)

theorem judge_silent_iff (colours : List (String × String)) (pair : Pair) :
    judge colours pair = [] ↔
      ∃ a b r, colourOf colours pair.text = some a ∧ colourOf colours pair.on = some b ∧
        ratio a b = some r ∧
        ((pair.least ≤ r ∧ pair.waived = none) ∨ (r < pair.least ∧ pair.waived.isSome = true)) := by
  unfold judge
  cases colourOf colours pair.text <;> cases colourOf colours pair.on <;> simp
  rename_i a b
  cases ratio a b <;> simp
  rename_i r
  cases pair.waived <;> by_cases lt : r < pair.least <;> simp [lt] <;> omega

/-- @proves REQ-LOOK.contrast_sufficient -/
theorem no_finding_means_every_pair_is_legible (pairs : List Pair)
    (colours : List (String × String)) :
    findings pairs colours = [] ↔
      ∀ pair ∈ pairs, ∃ a b r, colourOf colours pair.text = some a ∧
        colourOf colours pair.on = some b ∧ ratio a b = some r ∧
        ((pair.least ≤ r ∧ pair.waived = none) ∨ (r < pair.least ∧ pair.waived.isSome = true)) := by
  unfold findings
  simp only [List.bind_eq_nil, judge_silent_iff]

end TraceLean.Contrast
