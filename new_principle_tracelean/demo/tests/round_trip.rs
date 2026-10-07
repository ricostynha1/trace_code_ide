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
