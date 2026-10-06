/-! The conversions over whole degrees, checked at temperatures where every
division is exact. -/

namespace Thermo

/-- @models REQ-THERMO.to_fahrenheit -/
def toFahrenheit (c : Int) : Int := c * 9 / 5 + 32

/-- @models REQ-THERMO.to_celsius -/
def toCelsius (f : Int) : Int := (f - 32) * 5 / 9

/-- @proves REQ-THERMO.agreement -/
theorem agree_at_minus_forty : toFahrenheit (-40) = -40 := by
  decide

/-- @proves REQ-THERMO.round_trip -/
theorem round_trip_at_boiling : toCelsius (toFahrenheit 100) = 100 := by
  decide

end Thermo
