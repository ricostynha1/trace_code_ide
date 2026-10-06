---
id: REQ-THERMO
title: Converting temperatures
status: approved
decomposition: complete
clauses:
  to_fahrenheit: Converting from Celsius shall multiply by nine fifths and add thirty-two.
  to_celsius: Converting from Fahrenheit shall subtract thirty-two and multiply by five ninths.
  round_trip: Converting a temperature to the other scale and back shall give the temperature that went in.
  agreement: The two scales shall agree at minus forty.
---

# Converting temperatures

The two conversions in `src/celsius.rs`. Each clause is claimed where it is
done: implemented in Rust, tested in `tests/round_trip.rs`, modelled and proved
in `specs/Thermo.lean`. Open a clause to see every claim, with a link to it.
