# Progress

Living status. Structure is stable; the checklists move.

## Where the port is

| Stage | State |
|---|---|
| Documentation, methodology, decision log | written, hash-linked, current |
| Requirement set (43 documents) | written |
| Stage 0 — Rust conformance runner (`REQ-DRT-RUST`) | implemented, proved end to end |
| Stage 1 — trace kernel | implemented and modelled |
| Lean models | 35 modules under `formal/TraceLean/` |
| History, sandbox observation, surfaces | implemented and modelled |
| The representation both frontends render (`REQ-VIEW`) | modelled, implemented, differentially tested |
| Producers and dispatch (`REQ-SHOW`, `REQ-ACT`) | modelled, implemented, differentially tested |
| What is opened and what is shown (`REQ-SCREEN`) | modelled, implemented, differentially tested, and drawn by both frontends |
| The four stations (`REQ-SHOW`, `REQ-COST`) | all four open their own buffer; project, requirements, design and sandbox have producers |
| Frontends — terminal, window, page | built, conformance-checked on both rungs |
| Evidence is earned, not annotated (`REQ-EVID`, `REQ-LOCK`) | 235 records in the lock — 150 `L3` from the model↔implementation bond, 85 `L4` from the proof bond |
| The editor driven end to end (`REQ-DRIVE`) | modelled, differentially tested, and driven on a real pseudo-terminal over `demo/` |
| Model namespacing (`docs/08-ideas.md` §0) | done — every module under its own namespace, no name means two things |
| One shared runner per side (§1) | done — 135 generated packages became 2; a full differential run is 86s through `tools/differential-all.sh`, from about 1364s |
| Stage 2 — self-application fixpoint | the tree checks itself; `Imprecise 1` is the only finding |

Every clause is answered one of three ways: modelled and differentially tested,
proved, or `@structural` with a reason (ADR-0012). Nothing is `Unbound`,
`Unmodeled`, `Untested` or `Unimplemented`. `Imprecise 1` is the only finding.

## What is left

- **A full differential run does not saturate the machine through cargo.**
  `cargo test` runs test binaries one at a time, so the parallelism is bounded
  by the tests in one suite — 487% of sixteen cores. `tools/differential-all.sh`
  runs the same binaries concurrently and reaches 1150%. Nothing here can lift
  the cargo bound.
- **`Imprecise 1`.** `Command.lean` is the one file tree-sitter-lean4 0.3.0
  still cannot parse cleanly, so annotations in or after the unparsed region are
  capped at L1 (ADR-0008). There is no newer published grammar; this is not
  closable from here without changing what the definitions say.
- **L3 reaches 75 of 106 bindings.** `coverage::level` grants it for agreement
  *and* a met floor **the binding declared**. 75 bindings now declare `floors`,
  and their generation tests report counts against those names through
  `support::covered`; the other 31 have no declared floors, so `verdict` answers
  `Undeclared` and they stay at L1. A situation is a predicate over generated
  values and cannot move into JSON, so the binding names it and the suite
  reports how often the name was reached — that split is why the two halves are
  composed through `.tracelean/pending/` rather than asserted in one place.
- **The requirement↔model bond has five requirements' worth of advice and no
  judgements.** `judge::record` produces the record and
  `tracelean-trace --judge` exports the prompt and accepts a person's decision;
  the advice for `REQ-EVID`, `REQ-SCREEN`, `REQ-VIEW`, `REQ-SHOW` and
  `REQ-COST` is written down in `.tracelean/judging/`. Nobody has entered a
  decision. That rung is a person's.
- **One clause is reported as drift and nobody has ruled on it.**
  `REQ-SHOW.graph_from_refinement` promises that a requirement refining
  something appears under it; the walk is bounded from the roots, so a
  requirement no root reaches appears nowhere. The bound is necessary —
  `refines` is data and a cycle in it is representable — but a clause the
  implementation is allowed to fail is not a clause. A rewording is suggested in
  `.tracelean/judging/REQ-SHOW.md`; accepting it is a person's, and it gives up
  a guarantee, which is why it is not an edit.
- **The price table is a fact about the outside world.** Nothing here can check
  that `.tracelean/prices.json` holds the prices anyone charges. A differential
  run agrees just as perfectly about a table a year out of date, and the
  estimate would be confidently wrong and look right. The only check is a person
  reading a price list.
- **Nothing machine-checkable says a glyph means its name.** 🗑 where the buffer
  says `project` is conformant: its accessible name is the text, and the
  harness reads the text. Aptness is a judgement about how a thing looks, which
  is the L2 rung; the advice in `.tracelean/judging/REQ-VIEW.md` records it
  rather than letting it pass unnoticed.
- **Strength: 1 pinned, 127 open.** `pins_assurance` in `formal/TraceLean/Pinned.lean`
  is proved and the kernel confirmed it. `obligationSource` generates the rest
  and leaves them in `sorry`.
- **The window has been opened once.** It draws the listing the terminal draws,
  which is `one_representation` holding in the medium a person actually looks
  at. The L2 rung is still a person's judgement of how it *looks*, and nobody
  has entered one.
- **`menu_entries` is a second which-key.** `crates/editor` computes a mode's
  menu from the keymap's bindings directly rather than through
  `keymap::which_key`, so the menu the editor shows omits the `Escape` row the
  bar includes. Both are computed from the keymap, so neither is stale — but
  `REQ-MYTH.whichkey_is_a_query` asks for one query, and this is two.

## Resolved open questions

- **Does anchor resolution need tree-sitter in the model?** No. Anchor identity
  and staleness are modelled (`Anchor.lean`, `Hash.lean`); symbol extraction is
  axiomatised, with the law that equal normalised bodies hash equally. Precision
  is per anchor rather than per file (ADR-0011), so one unparsed region no
  longer caps a whole file.
- **Coverage floors for laws.** Designed and built: a binding states floors, a
  run is judged against them, and *vacuous* is a named verdict distinct from
  *short* (`REQ-DRT-COVER`, [04-coverage.md](04-coverage.md)). Every differential
  suite judges its own generation through `drt::coverage` rather than through a
  row of assertions.
- **Effects in Lean.** Axiomatised: opaque constants with laws written once over
  a witness (ADR-0002, `Effects.lean`). The model describes what a run must
  satisfy; nothing in `formal/` launches anything.
