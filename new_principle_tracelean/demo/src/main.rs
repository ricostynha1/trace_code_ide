mod celsius;

/// @implements REQ-TABLE.every_sample
fn main() {
    for degrees in [-40, 0, 37, 100] {
        println!("{degrees}C is {}F, {}", celsius::to_fahrenheit(degrees), celsius::describe(degrees));
    }
}
