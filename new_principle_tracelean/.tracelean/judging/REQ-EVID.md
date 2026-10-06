# Judging advice — REQ-EVID

Produced by an assistant asked to read each clause against its model. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**: nothing
here is evidence until a person enters the verdict with `--judge … --verdict …
--by <name>`, and the record is then theirs.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-EVID.<clause>`.

| Clause | Advice |
|---|---|
| `weakest_link` | agrees |
| `monotone` | agrees |
| `absent_is_lowest` | agrees |
| `bonds_separate` | **drift** — see below |
| `ladder` | agrees, with a note |
| `chain_rendered` | agrees |
| `record_reproducible` | agrees |
| `judgement_caps` | agrees |

## agrees

**`weakest_link`** — "the minimum over its bonds, never an average". `assurance`
folds `Level.min` from `L4` over `allBonds`, which is the minimum and cannot be
anything else: `Pinned.lean::pins_assurance` now proves the two theorems about it
admit no other function.

**`monotone`** — "adding a record shall never lower an assurance". Not visible in
the definition's shape; it is a property, proved as
`adding_a_record_never_lowers_assurance`. It holds because `bondLevel` takes a
`max` over its records, so one more record can only raise a bond, and `assurance`
is a `min` over values that only rose. A property clause realised by a theorem
rather than by the definition is the normal case here, not a gap.

**`absent_is_lowest`** — `bondLevel` folds from `Level.L1` and the fold is a
no-op for records of other bonds, so a bond nothing records contributes `L1`.
Exactly the clause.

**`chain_rendered`** — `chain = allBonds.map (bondLevel records)` is the per-bond
chain in a fixed order. The clause asks for presentability, which this is.

**`record_reproducible`** — the clause has two halves and they are carried by two
things: "shall not be expressible without it" by the datatype, whose variants
have no optional fields, and "shall carry what is needed" by `missingField`,
which rejects the empty string the type cannot. Together they answer it.

**`judgement_caps`** — `Detail.ceiling` maps `.judge` to `.L2`. Direct.

## drift

**`bonds_separate`** — "the three bonds shall be graded **independently**".

The model anchored to this clause is the `Bond` inductive. A datatype with three
constructors says the bonds exist and are distinct; it does not say each is
graded from its own records. That content is in `bondLevel`, whose filter
`if r.bond = b` is precisely what stops one bond's records reaching another's
level.

So as the pair stands, the model does not say what the clause says. The fix is an
annotation, not a theorem: add `@models REQ-EVID.bonds_separate` to `bondLevel`.
Recording `drift` is what causes that to be looked at; recording `agrees` would
leave a clause pointing at a declaration that does not carry it.

## agrees, with a note

**`ladder`** — "totally ordered, annotation below judgement below differential
testing below proof".

The constructor order is the ladder and the order itself is `toNat` with the `LE`
instance beside it, so the pair does say what the clause says. Worth knowing:
until today the project had proved reflexivity and transitivity but **not**
antisymmetry, so "totally ordered" was not fully established. It is now —
`Pinned.lean::Level.le_antisymm` — and comparability follows from `Nat`. Both
deserve `@proves REQ-EVID.ladder` so the clause's evidence matches what is
actually proved about it.

## Recording these

```bash
cargo run -p tracelean-core --bin tracelean-trace -- . \
  --judge REQ-EVID.weakest_link --verdict agrees --by "<your name>"
```

`drift` records no evidence and is reported; `unmodelable` produces a proposal
and changes nothing. Each record names the requirement hash and the model hash it
was about, so it expires the moment either moves.
