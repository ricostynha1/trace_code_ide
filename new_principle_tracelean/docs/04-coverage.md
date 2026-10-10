---
describes: [REQ-DRT-COVER, ARCH-HONEST]
described_hash:
  REQ-DRT-COVER: 05e3ddef88851419
  ARCH-HONEST: fb288c153afd69f8
---

# Coverage

A differential run that finds no disagreement says nothing on its own. It says
something once you know which situations its cases reached. A law about
deletions, checked over two thousand cases none of which deleted anything, is
satisfied and vacuous, and reporting that as evidence is the most comfortable
lie this system could tell.

## The shape

A binding declares a **floor**: situations its runs must reach, and how often.
After a run, the suite counts how often each was reached and
`drt::coverage::verdict` judges one against the other.

```
floors   [{situation: "deletes a file", atLeast: 20}, …]
observed [{situation: "deletes a file", reached: 3},  …]
         → Short { situation, reached: 3, atLeast: 20 }
```

Four verdicts. `Met`, `Undeclared` — no floor stated, which is not the same as
meeting a floor of zero — and the two that matter:

- **`Vacuous`** — no case reached the situation at all. The law was never asked
  its question. The fix is the generator's alphabet.
- **`Short`** — the situation was reached, just not often enough. The fix is the
  case count or the weighting.

They are separate because they call for different work, and a single "coverage
failed" would hide which.

## Classes, lines and waivers

Named situations are added on top of what the types already say. Every run
counts how many of its cases reached each **class** of its arguments
(`drt::classes`: zero or positive, empty or not, each `Option` and enum case,
inside every field), and `support::agreed` refuses L3 to an agreeing run that
left a class unreached. Lines are the other half: `coverage::line_reach` names
each executable line of an implementing item by its text, for a run measured
under coverage.

The target is all of them. A binding's `waive` lists classes or lines that
cannot be reached, each with a `reason`; a waiver without one excuses nothing,
and one that excuses nothing the run missed is reported (`unused_waivers`), so
waivers cannot pile up.

## What it is worth

`coverage::level(agreed, verdict)` is `L3` only for an agreeing run that met its
floor. Everything else is `L1`. A run that agreed but did not reach its floor
has not shown what the floor exists to make it show.

## Counting

The counting is the caller's, because only the caller knows what a situation is.
A situation is a predicate over generated values, so no JSON could hold it: the
binding holds the *name* and the floor, and the suite reports how often the name
was reached. A count against a name the binding never declared is refused — it
is a counting error, not extra credit.

## The two halves

L3 needs two facts established in two places. The differential test knows the
runs agreed; the `generation_reaches_…` test knows which situations were
reached. Neither can see the other's result, and they are usually two processes.

So each reports its half — `support::agreed(&result)` and
`support::covered(op, counts)` — into `.tracelean/pending/<op>.json`, and
whichever lands second composes the record. Composition is order-independent:
there is no first test.

The agreement half is keyed by seed rather than a flag, because
`also_implemented_by` binds a second frontend to the same model and both are run
against it. A record written after one of them agreed would say the clause is
checked when half of it is, so composition waits for one agreeing run per
declared implementation and names the lowest seed of them.
