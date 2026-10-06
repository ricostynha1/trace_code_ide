mod celsius;

/// @implements REQ-TABLE.every_sample
fn main() {
    for degrees in [-40.0, 0.0, 37.0, 100.0] {
        println!("{degrees}C is {}F", celsius::to_fahrenheit(degrees));
    }
}
