# Judging advice — REQ-COST

Produced by an assistant asked to read each clause against its model. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**: nothing
here is evidence until a person enters the verdict with `--judge … --verdict …
--by <name>`, and the record is then theirs.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-COST.<clause>`.

| Clause | Advice |
|---|---|
| `cost_from_usage` | agrees |
| `price_is_per_model` | agrees |
| `cache_priced_apart` | agrees, with a note |
| `estimate_is_labelled` | agrees, with a note |

## `cache_priced_apart`

*Note — the model shows the rates are separate, not that they are right.*
`amountOf` multiplies each of the four counts by its own rate, which is what
the clause asks. What no model here can say is whether the numbers in
`.tracelean/prices.json` are the prices anyone actually charges. That file is a
fact about the outside world, and the only check on it is a person reading a
price list. Worth knowing before judging, because a run that agreed perfectly
would agree just as perfectly about a table that was a year out of date.

## `estimate_is_labelled`

*Note — the proof is concrete, deliberately.* `an_estimate_says_so` evaluates
four spends rather than quantifying over all of them. A universal proof about
this string would be a proof about `String.startsWith`, which is a fact about
Lean's string representation and not about what the clause says; the four cases
are the shapes the line can take. The universal form of the claim is asked of
the implementation instead, over every generated spend, in
`generation_reaches_unpriced_models_and_every_kind_of_token`.

*Note — "never as an amount billed" is checked as an absence.* The clause has
two halves. That the line says `estimated` is checked directly. That it is
never rendered as a bill is checked by a driving test looking at a real screen
for the words a bill would use, which is the strongest thing available for a
claim of the form "and nothing does the other thing".

## Independent review — 2026-09-20

Second opinion, produced by a reviewer who did not write these models. Advice,
not a record (`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `cost_from_usage` | agrees |
| `price_is_per_model` | agrees |
| `cache_priced_apart` | agrees |
| `estimate_is_labelled` | agrees |

No drift. Four notes a person should read before entering a verdict.

### `cost_from_usage` — note

True by signature. `spendOf : List Price → List Usage → Spend` is pure, so "and
from nothing else" holds on every input there is. No run can falsify it and no
differential test can either. The clause buys an architectural guarantee, not a
checked behaviour.

Also: `spendOf` takes `List Usage`, not a transcript. Nothing bound to this
clause says those records came from one; that join is `readTranscript`'s
`usage` field, which models `REQ-TRANSCRIPT`.

### `price_is_per_model` — note

*The judge prompt shows half the clause.* The clause has two halves — look up
by the model the usage names, and report an unpriced model rather than pricing
it at zero. Only the first is in `priceOf`. The second is in `accumulate`, a
private unannotated declaration reached through `spendOf`. `spendOf` carries
`@models REQ-COST.price_is_per_model` as well, but `material::model_of` takes
the first declaration in `(file, line)` order, so `--judge` prints `priceOf`
alone. A person judging from the prompt judges the lookup and never sees the
honesty half.

That also weakens invalidation: the recorded `model_hash` is `priceOf`'s. If
`accumulate` were rewritten to price an unknown model at zero and name nothing,
the hash would not move and the judgement would survive the change it exists to
guard.

*A false unpriced-model signal is reachable.* `usageIn` defaults `model` to
`"unknown"` and returns a `Usage` for any record carrying a `usage` key,
whatever its shape (see the `unknown_preserved` drift in the REQ-TRANSCRIPT
review). Evaluated: `spendOf [opus] (readTranscript "{\"usage\":\"n/a\"}\n").usage`
= `{ amount := 0, unpriced := ["unknown"] }`. The estimate line then reads
`estimated 0.000000 (unpriced: unknown)` for a transcript that reported no
usage at all. The clause's honesty signal fires on nothing. The fault is in
`Transcript.lean`, not here, but it surfaces on this line.

*Named, not counted.* `noting` keeps the model string and discards the counts.
The body says "the amount says what it does not include"; the model names the
models, not the tokens.

### `cache_priced_apart` — note

Agrees, and no input can make it not agree. The separateness is in the shape of
`Price` — four fields — not in anything `amountOf` computes. A differential run
can find arithmetic divergence, never a collapsed rate. Trivially satisfied by
construction, which is fine so long as it is not read as a behaviour that was
checked. `Price` itself carries no annotation.

*Truncation is per record, not per total.* `amountOf` divides each record's sum
by 1000000, so the loss accumulates: a thousand records can discard up to a
thousand millionths. The comment's "less than a millionth of a unit per line"
is true per line, not per estimate. Deterministic either way, so
`ARCH-DETERMINISM` is unharmed; the figure is simply low, always in the same
direction.

*Wider than the words.* The clause names three kinds; `amountOf` prices four.
Harmless, but output being priced apart is not something the clause asked for.

The earlier note about `.tracelean/prices.json` stands and is the larger risk.

### `estimate_is_labelled` — note

The concrete proof is weaker than the file's own defence of it.
`an_estimate_says_so` checks `startsWith "estimated "` against a function whose
body is `"estimated " ++ money ++ tail`. The property proved is insensitive to
everything the four cases vary — amount, unpriced list, both — so the same
proof passes over one case or a hundred. Keeping it is right; reading it as
"four shapes were checked" is not. Nothing proves anything about `padded`, and
its `6 - digits.length` is Nat subtraction that saturates, correct here only
because `amount % 1000000 < 1000000`.

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `price_is_per_model` | agrees, with a note |

### `price_is_per_model` — note

Both halves are in the models taken together: `priceOf` looks up by the model
the usage names, and `spendOf` (through the private `accumulate`) names a model
with no price in `unpriced` instead of adding zero. The 2026-09-20 binding note
still applies: `--judge` prints `priceOf` alone and the recorded hash is
`priceOf`'s, so a change to `accumulate` would not invalidate the judgement. The
false `unknown` model from a non-object `usage` is fixed in `Transcript.lean`
(`usageIn` now requires an object).
