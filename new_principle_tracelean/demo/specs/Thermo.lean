/-! The conversions over whole degrees. Fahrenheit rounds down and Celsius
rounds up, so a Celsius temperature survives the trip there and back.

Each clause has a specification, a predicate saying which answers are right,
and a model, the function the Rust is tested against. The proved statements
*pin* the model when every function meeting them is the model. -/

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

/-- The model meets its specification.

@proves REQ-THERMO.to_fahrenheit -/
theorem to_fahrenheit_meets_its_specification (c : Int) : ToFahrenheit c (toFahrenheit c) := by
  unfold ToFahrenheit toFahrenheit
  omega

/-- Nothing else meets it: every function that does is the model.

@pins REQ-THERMO.to_fahrenheit -/
theorem to_fahrenheit_pinned :
    ∀ g : Int → Int, (∀ c, ToFahrenheit c (g c)) → g = toFahrenheit := by
  intro g h
  funext c
  have a := h c
  have b := to_fahrenheit_meets_its_specification c
  unfold ToFahrenheit at a b
  omega

/-- @proves REQ-THERMO.to_celsius -/
theorem to_celsius_meets_its_specification (f : Int) : ToCelsius f (toCelsius f) := by
  unfold ToCelsius toCelsius
  omega

/-- @pins REQ-THERMO.to_celsius -/
theorem to_celsius_pinned :
    ∀ g : Int → Int, (∀ f, ToCelsius f (g f)) → g = toCelsius := by
  intro g h
  funext f
  have a := h f
  have b := to_celsius_meets_its_specification f
  unfold ToCelsius at a b
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
