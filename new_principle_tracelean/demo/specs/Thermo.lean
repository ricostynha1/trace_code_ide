/-! The conversions over whole degrees. Fahrenheit rounds down and Celsius
rounds up, so a Celsius temperature survives the trip there and back.

Each clause has a specification, a predicate saying which answers are right,
and a model, the function the Rust is tested against. A specification *pins*
its model when the model meets it and no input has two right answers. -/

namespace Thermo

/-- `f` is `c` degrees Celsius in Fahrenheit, rounded down.

@specifies REQ-THERMO.to_fahrenheit -/
def ToFahrenheit (c f : Int) : Prop :=
  5 * (f - 32) ≤ 9 * c ∧ 9 * c < 5 * (f - 32) + 5

/-- @models REQ-THERMO.to_fahrenheit -/
def toFahrenheit (c : Int) : Int := c * 9 / 5 + 32

/-- `c` is `f` degrees Fahrenheit in Celsius, rounded up.

@specifies REQ-THERMO.to_celsius -/
def ToCelsius (f c : Int) : Prop :=
  9 * c - 9 < 5 * (f - 32) ∧ 5 * (f - 32) ≤ 9 * c

/-- @models REQ-THERMO.to_celsius -/
def toCelsius (f : Int) : Int := -((32 - f) * 5 / 9)

/-- The model meets its specification, and nothing else does.

@pins REQ-THERMO.to_fahrenheit -/
theorem to_fahrenheit_pinned :
    (∀ x, ToFahrenheit x (toFahrenheit x)) ∧
    (∀ x y1 y2, ToFahrenheit x y1 → ToFahrenheit x y2 → y1 = y2) := by
  unfold ToFahrenheit toFahrenheit
  constructor
  · intro x
    omega
  · intro x y1 y2 h1 h2
    omega

/-- @pins REQ-THERMO.to_celsius -/
theorem to_celsius_pinned :
    (∀ x, ToCelsius x (toCelsius x)) ∧
    (∀ x y1 y2, ToCelsius x y1 → ToCelsius x y2 → y1 = y2) := by
  unfold ToCelsius toCelsius
  constructor
  · intro x
    omega
  · intro x y1 y2 h1 h2
    omega

/-- @proves REQ-THERMO.agreement -/
theorem agree_at_minus_forty : toFahrenheit (-40) = -40 := by
  decide

/-- Every Celsius temperature, not only the ones tried.

@proves REQ-THERMO.round_trip -/
theorem round_trip (c : Int) : toCelsius (toFahrenheit c) = c := by
  unfold toCelsius toFahrenheit
  omega

end Thermo
