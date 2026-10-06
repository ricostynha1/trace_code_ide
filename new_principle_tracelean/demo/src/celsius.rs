//! Converting between the two scales people argue about.

/// Celsius to Fahrenheit.
///
/// @implements REQ-THERMO.to_fahrenheit
pub fn to_fahrenheit(degrees: f64) -> f64 {
    degrees * 9.0 / 5.0 + 32.0
}

/// Fahrenheit to Celsius.
///
/// @implements REQ-THERMO.to_celsius
pub fn to_celsius(degrees: f64) -> f64 {
    (degrees - 32.0) * 5.0 / 9.0
}

/// The one temperature where the two scales agree.
///
/// @implements REQ-THERMO.agreement
pub fn agreement() -> f64 {
    -40.0
}
