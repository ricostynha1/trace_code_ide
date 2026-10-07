use thermo::{agreement, describe, to_celsius, to_fahrenheit};

/// @tests REQ-THERMO.round_trip
/// @tests REQ-THERMO.to_celsius
#[test]
fn converting_back_gives_what_went_in() {
    for degrees in [-40, -1, 0, 1, 37, 100] {
        assert_eq!(to_celsius(to_fahrenheit(degrees)), degrees);
    }
}

/// @tests REQ-THERMO.agreement
#[test]
fn the_scales_meet_at_minus_forty() {
    assert_eq!(to_fahrenheit(agreement()), agreement());
}

/// Only the cold end: run `tracelean-trace . --coverage` and the other two
/// branches of `describe` show as lines no test runs.
///
/// @tests REQ-TABLE.every_sample
#[test]
fn below_zero_is_ice() {
    assert_eq!(describe(-5), "ice");
}
