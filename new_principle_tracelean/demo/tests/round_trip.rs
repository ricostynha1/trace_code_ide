/// @tests REQ-THERMO.round_trip
/// @tests REQ-THERMO.to_celsius
#[test]
fn converting_back_gives_what_went_in() {
    for degrees in [-40.0, 0.0, 37.0] {
        let there_and_back = to_celsius(to_fahrenheit(degrees));
        assert!((there_and_back - degrees).abs() < 1e-9);
    }
}

/// @tests REQ-THERMO.agreement
#[test]
fn the_scales_meet_at_minus_forty() {
    assert_eq!(to_fahrenheit(agreement()), agreement());
}
