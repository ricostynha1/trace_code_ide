# Requirements

Forty-one documents, 253 clauses. Identity is the frontmatter `id`, never the path —
a document may move without any annotation changing.

A requirement is *complete* when its author claims the clauses exhaust it. An
*open* decomposition can never read as finished: its coverage is rendered as a
lower bound (`≥`), because nobody has said what the denominator is.

## Architecture — the constraints everything else refines

| | Clauses | |
|---|---|---|
| [ARCH-SELFHOST](arch/ARCH-SELFHOST.md) | 5 | TraceLean is a proper project by its own definition |
| [ARCH-CORE-SHELL](arch/ARCH-CORE-SHELL.md) | 4 | Decision and effect are separate |
| [ARCH-DETERMINISM](arch/ARCH-DETERMINISM.md) | 5 | Derived artefacts are deterministic |
| [ARCH-EFFECT-LAW](arch/ARCH-EFFECT-LAW.md) | 5 | Effects are modelled by their laws |
| [ARCH-HONEST](arch/ARCH-HONEST.md) | 6 | Unknown is reported as unknown |
| [ARCH-NO-DRIVING](arch/ARCH-NO-DRIVING.md) | 5 | TraceLean observes agents and never drives them |

All six are open decompositions on purpose: an architectural constraint is not
something anyone can claim to have exhausted.

## Traceability — what a claim is and what it is worth

| | Clauses | |
|---|---|---|
| [REQ-REQDOC](trace/REQ-REQDOC.md) | 8 | Requirement documents and the refinement graph |
| [REQ-ANNOT](trace/REQ-ANNOT.md) | 7 | Annotation grammar |
| [REQ-ANCHOR](trace/REQ-ANCHOR.md) | 6 | Anchors and body hashing |
| [REQ-EVID](trace/REQ-EVID.md) | 8 | Evidence algebra — the L1–L4 ladder, three bonds, aggregated by minimum |
| [REQ-STALE](trace/REQ-STALE.md) | 6 | Staleness and invalidation |
| [REQ-CHECK](trace/REQ-CHECK.md) | 8 | Findings |
| [REQ-ROLLUP](trace/REQ-ROLLUP.md) | 6 | Roll-up and coverage reporting |
| [REQ-LOCK](trace/REQ-LOCK.md) | 5 | The committed index |
| [REQ-STRENGTH](trace/REQ-STRENGTH.md) | 6 | Spec strength |
| [REQ-DOCLINK](trace/REQ-DOCLINK.md) | 8 | Documentation is linked and goes stale |

## Differential testing — how a model and an implementation are compared

| | Clauses | |
|---|---|---|
| [REQ-DRT](drt/REQ-DRT.md) | 5 | Differential testing (open: falsification is never finished) |
| [REQ-DRT-BIND](drt/REQ-DRT-BIND.md) | 4 | Bindings describe a call and nothing more |
| [REQ-DRT-PROTO](drt/REQ-DRT-PROTO.md) | 6 | The conformance protocol |
| [REQ-DRT-SCHEMA](drt/REQ-DRT-SCHEMA.md) | 4 | Declared input schemas |
| [REQ-DRT-GEN](drt/REQ-DRT-GEN.md) | 6 | Case generation and shrinking |
| [REQ-DRT-COVER](drt/REQ-DRT-COVER.md) | 4 | Coverage qualifies a passing run |
| [REQ-DRT-LEAN](drt/REQ-DRT-LEAN.md) | 5 | Lean model runner |
| [REQ-DRT-RUST](drt/REQ-DRT-RUST.md) | 7 | Rust conformance runner |
| [REQ-DRT-TS](drt/REQ-DRT-TS.md) | 6 | TypeScript conformance runner |

## History — every change, with an inverse

| | Clauses | |
|---|---|---|
| [REQ-CMD](history/REQ-CMD.md) | 6 | Command algebra |
| [REQ-UNDO](history/REQ-UNDO.md) | 5 | History is a tree |
| [REQ-PERSIST](history/REQ-PERSIST.md) | 5 | Persistence and replay |
| [REQ-PROV](history/REQ-PROV.md) | 6 | Provenance of a position |

## Observation — what an outside agent left behind

| | Clauses | |
|---|---|---|
| [REQ-OBS](agent/REQ-OBS.md) | 5 | Observing an external agent |
| [REQ-SBX](agent/REQ-SBX.md) | 6 | Sandbox workspace and path policy |
| [REQ-MIRROR](agent/REQ-MIRROR.md) | 6 | Mirroring a workspace into the editor |
| [REQ-SELFWRITE](agent/REQ-SELFWRITE.md) | 5 | Self-write suppression |
| [REQ-TRANSCRIPT](agent/REQ-TRANSCRIPT.md) | 5 | Reading an external tool's transcript (draft) |
| [REQ-COST](agent/REQ-COST.md) | 4 | What a sandboxed agent's usage is estimated to have cost |

## Surface — key to action to intent to buffer to screen

| | Clauses | |
|---|---|---|
| [REQ-MYTH](surface/REQ-MYTH.md) | 10 | The modal keymap |
| [REQ-ACT](surface/REQ-ACT.md) | 7 | An action resolves to an intent, and nothing acts on a name |
| [REQ-SHOW](surface/REQ-SHOW.md) | 13 | The core produces every buffer, from the state it is given |
| [REQ-VIEW](surface/REQ-VIEW.md) | 12 | One representation, rendered by every frontend |
| [REQ-SCREEN](surface/REQ-SCREEN.md) | 16 | What is opened, and what is shown |
| [REQ-JUDGE](surface/REQ-JUDGE.md) | 8 | The human judge |
| [REQ-LSP](surface/REQ-LSP.md) | 7 | Language server integration |
| [REQ-DRIVE](surface/REQ-DRIVE.md) | 6 | The editor is driven, not just its parts |

## Reading one

The frontmatter carries the identity, the refinement edges and the clauses; the
body says why the clauses are what they are. A clause is referred to as
`REQ-X.clause_key`, and that is the string an annotation writes:

```rust
/// @implements REQ-ACT.one_path
```

What each role means, and how a claim becomes evidence:
[docs/00-methodology.md](../docs/00-methodology.md).
