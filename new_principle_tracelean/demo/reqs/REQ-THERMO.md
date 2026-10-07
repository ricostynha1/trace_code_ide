---
id: REQ-THERMO
title: Converting temperatures
status: approved
decomposition: complete
clauses:
  to_fahrenheit: Converting whole degrees Celsius shall multiply by nine fifths, round down and add thirty-two.
  to_celsius: Converting whole degrees Fahrenheit shall subtract thirty-two, multiply by five ninths and round up.
  round_trip: Converting whole degrees Celsius to Fahrenheit and back shall give the degrees that went in.
  agreement: The two scales shall agree at minus forty.
---

# Converting temperatures

The two conversions in `src/celsius.rs`. Each clause is claimed where it is
done: implemented in Rust, tested in `tests/round_trip.rs`, modelled and proved
in `specs/Thermo.lean`. Open a clause to see every claim, with a link to it.
