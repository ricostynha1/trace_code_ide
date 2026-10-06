# Thermo — a tree to open

A small, ordinary project, traced. It exists so that TraceLean has something to
be opened on and something to show: a few directories, one empty file, one name
with an accent, and requirements that the code, the tests and a Lean model
claim.

- `reqs/REQ-THERMO.md` — the conversions, approved, every clause claimed
- `reqs/REQ-TABLE.md`  — a draft that refines it, with a clause nothing claims
- `src/celsius.rs`     — `@implements` each conversion
- `src/main.rs`        — the entry point
- `src/blank.rs`       — an empty file, because an empty buffer is a buffer
- `src/notes/`         — prose, including a filename that is not ASCII
- `tests/round_trip.rs` — `@tests`
- `specs/Thermo.lean`  — `@models` and `@proves`

Open the requirements station to see each requirement's evidence, a clause to
see every claim with a link to it. `crates/tui/tests/driving.rs` in the project
that ships this opens exactly this directory.
