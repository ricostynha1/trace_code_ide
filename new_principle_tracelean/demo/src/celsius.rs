//! Converting between the two scales people argue about, in whole degrees.

/// Celsius to Fahrenheit, rounded down.
///
/// @implements REQ-THERMO.to_fahrenheit
pub fn to_fahrenheit(degrees: i64) -> i64 {
    (degrees * 9).div_euclid(5) + 32
}

/// Fahrenheit to Celsius, rounded up.
///
/// @implements REQ-THERMO.to_celsius
pub fn to_celsius(degrees: i64) -> i64 {
    -((32 - degrees) * 5).div_euclid(9)
}

/// The one temperature where the two scales agree.
///
/// @implements REQ-THERMO.agreement
pub fn agreement() -> i64 {
    -40
}
