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

/// What water is at a temperature in Celsius.
///
/// @implements REQ-TABLE.every_sample
pub fn describe(degrees: i64) -> &'static str {
    if degrees <= 0 {
        "ice"
    } else if degrees < 100 {
        "water"
    } else {
        "steam"
    }
}

/// The one temperature where the two scales agree.
///
/// @implements REQ-THERMO.agreement
pub fn agreement() -> i64 {
    -40
}
