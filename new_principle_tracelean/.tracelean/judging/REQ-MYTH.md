# Judging advice — REQ-MYTH

Produced by claude-review, simulating the human approver. Each verdict below was
also entered with `--judge … --verdict … --by "claude-review (simulated human)"`;
an `agrees` is a record, a `drift` or `unmodelable` is recorded as no evidence.

Material assembled by `tracelean-trace . --judge REQ-MYTH.<clause>` and
`--context … --parts requirement,models`, read against `formal/TraceLean/Keymap.lean`.

| Clause | Verdict |
|---|---|
| `totality` | agrees, with a note |
| `escape_pops_one` | drift |
| `escape_terminates` | drift |
| `modes_defined` | drift |

## `totality` — note

`step` is total and has no "nothing happens" outcome. Two behaviours a person
should accept knowingly: an unbound non-Escape key in a non-root mode is
consumed as `.leave root` (visible, but the key itself is not passed on), and in
a mode that does not exist every key, Escape included, is `passThrough`, so
`nextMode` keeps the machine in that mode for good. Not swallowing, but a dead
end that only `validate` prevents.

## `escape_pops_one` — drift

`step`'s last branch: an unbound key that is not Escape, in a mode with a
parent, returns `.leave keymap.root`. With root `A`, `B.parent = A`,
`C.parent = B`, pressing an unbound `x` in `C` leaves to `A` — two levels.
The model's own outcome is `leave`, so by its vocabulary this is leaving, and it
does not move one level. Either the clause names Escape only ("Escape shall
move exactly one level") or the fallback leaves to the parent.

## `escape_terminates` — drift

`validate` reports a parent *cycle* only. Two keymaps it passes clean where
repeated Escape never reaches the root:

- root `A` binds `b` to enter `B`; `B.parent = none`. `B` is reachable, has no
  cycle, and in `B` Escape is `passThrough` forever.
- `B.parent = "Ghost"`, no mode `Ghost`. `parentCycleFrom` sees `none` from the
  lookup and answers `false`; Escape from `B` lands in `Ghost`, where every key
  passes through.

The clause needs "every non-root mode's parent chain ends at the root", which is
a check `parentCycleFrom` could make by requiring the chain to end on
`keymap.root` rather than on any `none`.

## `modes_defined` — drift

`bindingProblem` checks that each `enter` target exists. A mode's `parent` is
also a transition target — `step` returns `.leave parent` and `nextMode` moves
there — and nothing checks it exists (second example above). Only half the
transition targets are covered.

Side observation, not this clause: `reachableFrom` spends one unit of fuel per
dequeued name, including names of modes that do not exist, with budget
`modes.length + 2`. Enough dangling `enter` targets ahead of a real mode exhaust
the fuel before that mode's children are explored, giving a spurious
`unreachableMode` next to the genuine `undefinedMode` reports.
