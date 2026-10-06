# ROLLOVER — new_principle_tracelean

*Last updated 2026-09-20*

## Goal
Port TraceLean into this tree under TraceLean's own methodology: requirements with
clauses, Lean models, Rust implementations, differential testing. Self-hosting.

## Done

**Docs** (`docs/`): methodology, bootstrap ladder, scope cuts, doc-sync design,
progress. Decision log `docs/decisions/ADR-0001..0006`.

**Requirements** (`reqs/`, 36 documents): 6 `ARCH-*` architectural roots +
30 `REQ-*` feature requirements across `drt/ trace/ history/ agent/ surface/`.
All parse under the stage-0 tool; every `refines:` resolves; no cycles, no
duplicate ids, no frontmatter problems.

**Scope cuts applied**: in-house agent runtime, tool registry/selector, provider
clients, context retention, cost model, MCP, ACP, shell sandbox, surgical_edit —
all dropped. External sandboxed-agent observation is the only agent path
(`ARCH-NO-DRIVING`, `REQ-OBS`). Judge is a human, no network code (ADR-0003).

**Stage-0 driver** `tools/stage0-trace` — runs the *existing* tracelean kernel
over this tree. Build: `cd tools/stage0-trace && cargo build --release`.
Run: `./tools/stage0-trace/target/release/stage0-trace .` (pass project root, not `..`).

**REQ-DRT-RUST implemented and proved end to end.** This was the blocker: stage-0
refuses non-Python implementations (`tracelean/core/src/drt/bind.rs:265`), so
nothing in a Rust project could reach L3.
- `crates/core/src/drt/signature.rs` — parses Rust `fn` parameter names. 9 tests.
- `crates/core/src/drt/config.rs` — `Binding` / `CallSpec`, qualified ops.
- `crates/core/src/drt/rust_runner.rs` — resolves bindings, derives module paths
  from file paths, generates + writes the runner crate. 6 tests.
- `crates/core/src/drt/protocol.rs` — `Case` / `Reply` / `RunnerError`.
- `crates/core/tests/rust_runner_end_to_end.rs` — generates a runner against this
  crate, compiles it, speaks the protocol. **Passes.**

Status: 15 unit tests + 1 end-to-end pass. 29 annotation links resolve to real
symbols under stage 0.

### 2026-09-16 21:40 — first vertical slice closed

The methodology now runs end to end on itself.

- `formal/` — Lean package (Lean 4.12, no mathlib). `TraceLean/Evidence.lean`
  models `REQ-EVID`: `Level`, `Bond`, `bondLevel`, `assurance`, `chain`.
  Two theorems proved by the kernel: `assurance_le_bond` (the aggregate never
  exceeds any bond) and `assurance_mem_chain` (the result is always a level
  something established — an average could not be).
- `crates/core/src/evidence.rs` — the Rust implementation, wire-compatible with
  Lean's derived JSON encoding.
- `crates/core/src/drt/{schema,gen,lean_runner,run}.rs` — declared schema
  grammar, deterministic generator with edge sampling and shrinking, Lean runner
  generation, and the run loop that asks both sides and reduces divergences.
- `.tracelean/drt.json` — the first real binding.
- `crates/core/tests/differential_evidence.rs` — **2000 cases, seed 7, zero
  divergences.** A negative test binds the wrong function on purpose, finds the
  disagreement and shrinks it to `{"records": []}`.

`REQ-EVID.weakest_link` is the first clause in this tree with L3 evidence and a
proved model behind it.

### 2026-09-16 21:45 — trace kernel ported; the two kernels agree

The ported kernel now reads this tree. Stage-2 comparison is **passing**.

- `trace/hash.rs` — normalised body hashing. Comments excluded (so editing an
  annotation cannot invalidate its own evidence), whitespace inside literals
  preserved.
- `trace/annotation.rs` — the grammar. Pure, total, no regex: every `@word`
  yields a directive or a named problem.
- `trace/anchor.rs` — tree-sitter scanning for Rust and Lean 4, symbol paths,
  region balance.
- `trace/requirement.rs` — frontmatter, refinement DAG, cycle detection.
- `trace/record.rs` — evidence records and invalidation (`REQ-STALE`).
- `trace/index.rs` — the scanner shell.
- `trace/checker.rs` — 12 named finding kinds, small blocking set.
- `trace/rollup.rs` — `Figure { value, exact }`, so a lower bound cannot be
  rendered bare.
- `trace/lockfile.rs` — deterministic serialisation, version-gated parse.
- `crates/core/src/bin/tracelean-trace.rs` — the stage-2 driver.

**Fixpoint: 144 links, both kernels, same set.** Two spellings differ and the
port is right about both — see ADR-0007. A `crates/core/tests/fixpoint.rs`
test declares exactly those two and fails on any new divergence.

Honest state of this tree by its own checker: 250 findings, **none blocking** —
207 unmodelled clauses, 39 untested, 4 unbound. The 4 unbound are `REQ-EVID`
clauses that have a model and an implementation and no `@drt` binding yet, which
is exactly the finding the checker exists to surface.

86 unit tests, 3 integration suites.

### 2026-09-16 21:55 — history ported, modelled and differentially tested

- `history/command.rs` — the command algebra. Six variants, witnesses carried,
  refusal rather than partial application.
- `history/tree.rs` — branching history, `jump_to` via nearest common ancestor,
  pure preview.
- `history/provenance.rs` — which edit wrote the text at a position, by
  inverting recorded edits. Renames followed; deletions never claimed as an
  origin.
- `history/persistence.rs` — append-only log, checkpoint equivalence, truncated
  records replayed to the last complete entry.
- `formal/TraceLean/Command.lean` — the model. Four theorems proved:
  inverse-involution for insert / delete / rename, and that a batch's inverse
  runs its members in the opposite order.
- `crates/core/tests/differential_command.rs` — **6000 cases, model against
  implementation, agreeing** on both the round-trip law and on applying.

**The differential test earned its keep immediately.** It found a real modelling
error and shrank it to a two-element case: the model's workspace was a list of
pairs, which admits two entries for one path, while the implementation's is a
map, which cannot. The model was wrong — a model that admits states the thing it
models cannot reach makes claims about situations that do not exist. Fixed in
`Workspace.canon` rather than by restricting the generator, which would have
hidden the gap instead of closing it.

State by this tree's own checker: 254 findings, none blocking. 123 tests.

### 2026-09-16 22:10 — every requirement area now has an implementation

- `observe/policy.rs` — sandbox path classification, total over four answers.
  Modelled in `formal/TraceLean/Policy.lean` with a theorem that protected is
  never mirrored. **2000 cases agreeing.**
- `observe/mirror.rs` — tree diff to commands, and self-write suppression.
  Modelled in `formal/TraceLean/Mirror.lean`. **2000 cases agreeing.**
- `observe/transcript.rs` — partial-record holding, unrecognised records kept.
- `surface/keymap.rs` — the modal keymap as a total state machine, with
  load-time validation: undefined modes and actions, parent cycles, unreachable
  modes and actions.
- `surface/lsp.rs` — UTF-8/16/32 position round-tripping, workspace-edit
  lowering back-to-front, overlap refused, "no server" never rendered as an
  empty result.
- `judge.rs` — the human judge. Prompt exported, never sent; no network code.
- `trace/doclink.rs` — **your hash-linked documentation.** Implemented, and this
  project's own four narrative documents are stamped and checked: 9 doc links,
  all current, with a test that a moved hash puts a document in review.
- `trace/strength.rs` — the `@pins` obligation, generated ending in `sorry`.

**A real defect found and fixed in the scanner.** The Lean grammar available
(`tree-sitter-lean4` 0.3.0) cannot read `mutual`, and the scanner was reporting
such files as precisely anchored anyway — so annotations after the error
silently anchored to the whole file *at full confidence*. Now an error node
marks the scan imprecise, caps those links at L1, and reports it under its own
non-blocking finding kind. See ADR-0008. Three model files currently report as
imprecise; their differential evidence is unaffected.

Six differential suites pass end to end: evidence, command round-trip, command
apply, policy, mirror, and the generated-runner proof. Fixpoint against stage 0
passes with three declared divergences (ADR-0007).

175 unit tests, 8 integration suites.

### 2026-09-16 22:30 — nine differential bindings; the binding file is now checked

Models and bindings added for the keymap, staleness, position encoding and
roll-up. **Nine bindings, all agreeing**, each 2000–3000 cases:

| Clause | Cases |
|---|---|
| `REQ-EVID.weakest_link` | 2000 |
| `REQ-CMD.round_trip` / `.total_or_refused` | 3000 each |
| `REQ-SBX.classification_total` | 2000 |
| `REQ-MIRROR.apply_reproduces` | 2000 |
| `REQ-MYTH.totality` | 3000 |
| `REQ-STALE.change_invalidates` | 3000 |
| `REQ-LSP.encoding_round_trip` | 2000 |
| `REQ-ROLLUP.min_not_mean` | 2000 |

`.tracelean/drt.json` now lists every binding, and
`crates/core/tests/bindings_are_real.rs` checks the configuration against the
annotations in both directions.

**It caught real dishonesty on its first run.** Twelve clauses carried `@drt`
annotations with no binding behind them — claiming differential testing that did
not exist. All twelve removed; they are the queue for the next models
(`REQ-ANNOT.totality`, `REQ-UNDO.jump_equivalence`, `REQ-PROV.mapping_inside`,
`REQ-PERSIST.replay_exact`, `REQ-SELFWRITE.own_writes_ignored`,
`REQ-TRANSCRIPT.partial_line_held`, `REQ-DOCLINK.hash_moves_review`,
`REQ-LOCK.deterministic_bytes`, `REQ-CMD.batch_reverses`,
`REQ-LSP.edits_ordered`, `REQ-JUDGE.caps_at_judgement`,
`REQ-STRENGTH.obligation_generated`).

**A protocol defect fixed.** A runner answering `null` — which is what a model
returning `none` does — was parsed as carrying neither an output nor an error,
so a legitimate answer looked like a runner that could not speak. `Reply` is now
read by key presence rather than by a derived `Option`.

`README.md` written.

178 unit tests, 10 integration suites.

### 2026-09-16 23:15 — Unbound is zero

Every clause in this tree that has both a model and an implementation is now
differentially checked. `Unbound` went 14 → 26 → **0**.

New models: `DocLink.lean`, `Transcript.lean`, `Judge.lean`; self-writes and
`mutationsOf` added to `Mirror.lean`; edit lowering added to `Lsp.lean`.

**21 bindings covering 42 clauses**, all agreeing — 20 differential suites, each
1000–3000 cases. New ones: command inverses (batch order), document state and
what blocks, self-write suppression and expiry, transcript reading, edit
lowering, judgement recording, judgement lapse, backend ceilings, the evidence
chain, and the mutation list itself.

**One call may check several clauses.** `.tracelean/drt.json` entries take
`also_checks`; `bindings_are_real` holds those clauses to the same rule as the
primary one. See ADR-0009.

**A defect in the binding mechanism, found by the arity check.** Resolution
matches a symbol inside a file, and both `doclink.rs` and `record.rs` already
had a *method* of the name a new free function wanted. Resolution took the
method. `signature::declarations` now counts, and `resolve` refuses an ambiguous
name instead of guessing — ADR-0009.

Three smaller things: `read_chunks`, `mutationsOf` and similar wrappers exist so
the generator can produce the input shape that matters (a transcript with a
half-written last line; a workspace with no duplicate paths) — the function
under test is unchanged. Encodings for `Detail`, `Key`, `Evidence`, `Material`,
`Judgement` and judge `Outcome` moved to external tagging and camelCase to match
Lean's derived JSON. `tracelean-trace --show <Kind>` lists one finding kind in
full.

`README.md` written.

179 unit tests, 13 integration suites, 20 differential comparisons.

### 2026-09-17 01:30 — the annotation grammar, the undo tree, and architecture checked from source

**`Annotation.lean`** models the grammar that every link in the project goes
through: `@word` tokens, roles, qualifiers, attributes, identifier splitting.
3000 generated comment bodies, joined out of fragments chosen to be the cases
nobody writes down — a bare `@`, `a@example.com`, `@implementsREQ-X`, an
unterminated quote, an identifier starting with a digit. Agreement first run
after one encoding fix.

**`Tree.lean`** models the undo tree. The test drives a *script* — push, undo,
redo, jump — and then asks every node the same question two ways: travel to it
via the nearest common ancestor, and replay its ancestry from the base. Both
answers go in the report, so one run checks model against implementation and
the two definitions against each other.

`Step` split into `Push` and `Move { movement }` on both sides. With a flat
four-way choice, three quarters of every script navigated a tree that was never
built — 42% of generated histories had no node at all. The split is also the
truer model: recording work and travelling over it are different things.

**`crates/core/tests/architecture.rs`** — the `ARCH-*` clauses that are about
what the code *does not* do, checked by reading the tree rather than asserted in
a comment: no model or network call, nothing that launches an agent outside the
differential toolchain, no clock and no unseeded randomness. Four tests; the
last one states the single exception (fixture directories named from the process
id) instead of hiding it.

**`Unbound` is zero and stays zero.** 23 bindings, 47 clauses, 22 differential
suites.

`crates/core/src/wire.rs` holds the one shared wire shape — a `BTreeMap` as
sorted pairs — now used by `Workspace.files` and `RawAnnotation.attrs`.

180 unit tests, 16 integration suites.

### 2026-09-17 01:50 — provenance, replay, lockfile order, spec strength, the checker

Five more subsystems modelled and bound. **31 bindings covering 72 clauses**,
every one agreeing.

| Model | What the run checks |
|---|---|
| `Provenance.lean` | Which recorded edit wrote the text at a position, over generated histories — including across batches and renames |
| `Persistence.lean` | Replay, checkpoint equivalence, and a log that ends mid-entry, in one report |
| `Lockfile.lean` | The order links are written in — total through every field, so the file is a function of the tree and not of the walk |
| `Strength.lean` | What a proof is worth: open, attempted, nondeterministic, and never `pinned` from links alone |
| `Checker.lean` | Exactly one chain finding per clause; what each kind means; unsound qualifiers |

Three functional cores were pulled out of shells to make this possible, and each
is a better shape than what it replaced: `obligations_from` over links rather
than over an index, `coverage_kinds` and `qualifier_kinds` over roles and
qualifiers rather than inline in `check`, `from_script` shared by the tree and
provenance suites.

`Unmodeled` 170 → 141. `Untested` 54 → 40. `Unbound` 0.

**Generator tuning is most of the work in a new suite.** Four of the five
needed their alphabets narrowed before the interesting cases occurred at all —
wide string spaces mean paths never collide, so every command is refused and
every generated history is empty. Each narrowing is commented where it is, with
what it would otherwise have missed.

### 2026-09-17 02:15 — hashing, the refinement graph, the keymap, and the generator itself

**41 bindings covering 88 clauses.** `Unmodeled` 141 → 125.

| Model | What the run checks |
|---|---|
| `Hash.lean` | Whitespace normalisation, comment exclusion, and FNV-1a reimplemented in Lean — the digest agrees bit for bit |
| `Refinement.lean` | The depth-first cycle search, including *which* cycle is reported, and dangling parents kept distinct from cycles |
| `Keymap.lean` (extended) | Validation — undefined modes and actions, parent cycles, unreachable modes and actions — and the which-key bar |
| `Generator.lean` | **The case generator itself**: the xorshift64\* stream, edge sampling, string generation |

The generator one is the bootstrap turning on itself. Every other suite's cases
come out of `gen::Rng`, and an evidence record saying "seed 7, twelve million
cases" is auditable only while seed 7 still means those cases. The stream is now
checked against an independent implementation over seeds across the whole 64-bit
range, including 0 and `u64::MAX`.

Two things the work changed for the better rather than around:

- `keymap::validate` sorted its findings by their own `Debug` rendering, which
  made the order an accident of a formatting implementation. `Problem` now
  derives `Ord` and the report is sorted by it.
- `hash::normalize` sliced protected ranges without bounds-checking them. They
  come from the scanner and are always valid, but a scanner bug would have
  become a panic in the function everything else depends on. Clamped now.

Lean encodes `UInt64` as a JSON *string*; the implementation writes a number.
The generator model therefore takes and returns `Nat` and converts at the
boundary — the arithmetic inside is still 64-bit and wrapping.

### 02:45 — anchors name one thing, and precision stopped being a file property

`properties.rs` was the first thing in the tree to ask whether the *anchor*
machinery itself holds up, and it found two defects (ADR-0011):

- A file could hold two declarations with the same symbol path — `impl Foo`
  next to `struct Foo` in Rust, and `def Kind.progress` collapsing to `Kind` in
  Lean. Both fixed in `anchor.rs`: `impl` is a scope rather than a declaration,
  and Lean names are read from the source instead of the grammar's first
  identifier token.
- Precision was a file-wide flag, so one unreadable tactic block capped every
  annotation in the file at L1. It is now per anchor: the declaration must have
  parsed, and no unparsed region may *begin* between the comment and it.
  Capped annotations went from 124 to 58.

Also: annotation parse problems were collected and never reported, so a
misspelt `@implments` disappeared silently. `index::build` now surfaces them,
and `properties.rs` pins that.

Findings now: `Imprecise 15`, `Unmodeled 117`, `Untested 0`. 19 property tests,
all suites green.

### 03:00 — the axiomatised-effect slice, finally built

ADR-0002 promised `formal/TraceLean/Effects.lean` in the first hours of the port
and it did not exist. It does now, and it is the pattern the whole effectful
half of the system rests on:

- **`formal/TraceLean/Effects.lean`** — every axiom in the project, in one file.
  `sandboxCopy`, `probeContainment`, `observedWrites`, `readTranscript` are
  opaque constants; each law is written **once**, as a total function over a
  witness (`copyViolations`, `capabilityViolations`, `escapeViolations`, and
  `runViolations` over all three), and the axiom says the real operation
  produces witnesses that satisfy it. `readTranscript` deliberately carries no
  axiom, and the file says why — a `String → String` already cannot consume.
- **`crates/core/src/observe/effects.rs`** — the same functions in Rust.
- **`crates/core/src/observe/workspace.rs`** — the thin shell that actually
  copies a tree, probes `PATH` for a containment mechanism (without running
  it), and records a witness. Its tests copy a real directory and check the
  laws hold of what happened, which is `ARCH-EFFECT-LAW.law_checked` against
  reality rather than against a description.
- **`crates/core/tests/differential_effects.rs`** — model against
  implementation over generated witnesses, 2000 cases at seed 53.

It found a defect immediately: `copy_violations` walked `before.keys()` then
`after.keys()`, so the order of reported changes depended on which snapshot was
taken first. Now a `BTreeSet` union, matching the model.

`also_checks` now accepts qualified names (`REQ-SBX.real_tree_untouched`), so
one call can establish clauses of several requirements — the copy law is
simultaneously about observation and about containment.

New structural checks in `architecture.rs`: filesystem access is confined to a
named list of shells and those shells stay branch-thin; all axioms live in one
file and every law is a callable function; findings are named vocabularies, not
bools; decision modules are public.

Findings now: `Imprecise 15`, `Unmodeled 112`, `Untested 0`, `Unbound 0`.

### 03:31 — protocol, requirement documents, staleness sweep, doc declarations

Four more vertical slices, each requirement → Lean model → Rust → differential
test, and each found something:

- **`Protocol.lean`** (`REQ-DRT-PROTO`). Reading a runner's line is now a total
  function `hear(expected, line) → Heard` with named failure reasons, and
  `duplicateOps` for dispatch collisions. **The harness never checked that a
  runner echoed the case it was asked** — a runner one reply behind would have
  been read as answering the current case. `run.rs` now routes through `hear`.
- **`Requirement.lean`** (`REQ-REQDOC`). The frontmatter grammar, modelled.
  `FrontmatterProblem` gained a named `kind`, and `ParseOutcome::NotARequirement`
  now carries its problems — **markdown that was malformed *and* not a
  requirement was silently skipped**, which is the case where silence hides why
  an expected requirement is missing.
- **`Staleness.lean`** (`REQ-STALE`). `sweep` partitions records with nothing
  rewritten and nothing dropped (proved: the two sides account for every record
  that went in), and `revalidated` says what replaces a stale record — always
  drawn from what the backend produced.
- **`DocLink.lean`** (`REQ-DOCLINK`). `declaredIn` models the `describes:` /
  `described_hash:` grammar, `recordedFrontmatter` what a person re-records.
  **`recorded_of` resolved a repeated target to the last entry while `state`
  resolved it to the first**, so one function would call a document current and
  the other would write a different hash for it.

Generator note worth keeping: for line-oriented parsers, put *whole documents*
in the example alphabet. Drawing a fence, an identifier and a closing fence as
three independent lines in the right order happens about once in two hundred
draws, which leaves everything past the fence untested.

Findings now: `Imprecise 17`, `Unmodeled 95`, everything else zero.

### 03:51 — `@structural`, and the lockfile's version stamp

**A fourth qualifier, `@structural(reason=...)`** — ADR-0012. A third of this
project's clauses are not about a value: *nothing here calls a model*, *a link
exists only where somebody wrote one*, *the index equals the bootstrap tool's*.
There is no `f : Input → Output` to write in Lean and no second implementation
to compare against, so the checker was demanding something that cannot exist.

A structural clause requires a `@tests` and nothing else — no model, no binding,
and no `@implements` either, because there is nothing to point one at. It stays
in the coverage denominator, which an exemption would not, and caps at L2. The
`reason=` is required; `qualifier_kinds` reports a structural claim without one
as unsound. `REQ-ANNOT.qualifiers` and a new `REQ-CHECK.structural_is_not_exempt`
record it; `Checker.lean` models it and the existing proof still goes through.

Applied to 21 clauses across `ARCH-NO-DRIVING`, `ARCH-SELFHOST`,
`ARCH-CORE-SHELL`, `ARCH-EFFECT-LAW`, `ARCH-HONEST`, `ARCH-DETERMINISM` and
`REQ-LOCK`, each with a test that reads the repository. **Every ARCH clause now
has either a model or a structural test.**

Also this pass: `REQ-LOCK.version_stamped` and `evidence_preserved` modelled and
differentially tested; five new structural tests in `architecture.rs`; and one
requirement text corrected — `ARCH-SELFHOST.no_convention` said a link "shall
survive any renaming of files or directories", which is not what the system does
or should do. A `link_hash` names its target, file included, so that evidence
earned for a function in one file is not inherited by the same text in another.
The clause now says what it meant: a link is never *inferred* from a filename,
directory or naming convention.

Findings now: `Imprecise 17`, `Unmodeled 64` — down from 117 at 02:15, and all
64 are feature clauses rather than architectural ones.

### 04:01 — judge prompt, LSP registry, and the generated runners

- **`Judge.lean::prompt`** (`REQ-JUDGE`). The prompt is the one place a model's
  opinion enters this system at all, so it is modelled and compared like
  anything else — a prompt that quietly stopped presenting the divergence would
  make every judgement after it worth less with nothing reporting a change.
  `no_call` is structural.
- **`Lsp.lean::presentText` / `encodingFor`** (`REQ-LSP`). One server per
  language, the position encoding read off the running one, and an unregistered
  language *named* rather than answered with a default. `ServerState` and
  `Display` moved from internal to external tagging to match the model's derived
  encoding; one assertion in `properties.rs` followed.
- **`runners_are_generated.rs`** — 13 clauses of `REQ-DRT-RUST`, `REQ-DRT-LEAN`
  and `REQ-DRT-BIND` checked by generating into a scratch directory and reading
  what came out: only cache-directory files written, a path dependency in the
  manifest, `lean_exe` in the lakefile, a tampered runner regenerated, and a
  closed key vocabulary in `drt.json` so a binding cannot grow an adapter.

Findings now: `Imprecise 17`, `Unmodeled 44`.

### 04:15 — anchors, agreement, and coverage designed at last

- **`Anchor.lean`** (`REQ-ANCHOR`). `anchorIdent` — no line number anywhere in
  an identity — and `anchorCeiling`, which caps everything claimed through an
  unparsed file at L1. `AnchorKind` moved to external tagging.
- **`Protocol.lean::agree` / `drtLevel`** (`REQ-DRT`). Both sides refusing an
  input is *agreement*; treating it as a failed run makes every partial function
  untestable. `run.rs::agree` now projects onto the modelled decision.
- **`REQ-DRT-COVER` designed and built** — it had been `draft` since the start
  and was the last real design gap. A binding declares a floor (situations its
  runs must reach, and how often); `drt::coverage::verdict` judges a run against
  it and `coverage::level` says what it is worth. Four verdicts, and the two
  that matter are separate: **vacuous** (no case reached the situation — the law
  was never asked its question) against **short** (reached, not often enough).
  They call for different work, and one "coverage failed" would hide which.
  `level(agreed, verdict)` is L3 only for an agreeing run that met its floor.
  Modelled in `Coverage.lean`, differentially tested, documented in
  `docs/04-coverage.md`, and `differential_effects.rs` now judges its counts
  through it rather than through a row of assertions.

Findings now: `Imprecise 17`, `Unmodeled 35`. All 11 documents current.

### 06:45 — records, mirror, and the last unmodelled clause

Six more models, and the finding list is now one line long.

- **`Record.lean` / `record.rs`** (`REQ-RECORD`). `sweep` decides which stored
  records a link's move invalidates, `revalidated` says which came back, and
  `reproducibility` grades an evidence record by whether its inputs are still
  there: **reproducible**, **incomplete** (inputs named, one missing), or
  **missingInput** (never recorded). A record nobody can re-run is not evidence,
  and the two ways of failing that call for different repairs.
- **`Rollup.lean::fileCoverage`** — unclaimed files stay in the denominator.
  A percentage that only counts what someone already annotated always reads 100%.
- **`Mirror.lean` / `mirror.rs`** (`REQ-MIRROR.binary_handled`). `FileState` is
  `absent | text | opaque`; `changeAt` mirrors text and only ever *reports* an
  opaque file, with `an_opaque_file_is_never_mirrored` proving it. `snapshot`
  now goes through `survey`, which records a path it cannot read as text instead
  of dropping it silently — the old behaviour made a binary write invisible.
- **`Strength.lean::obligationSource`** — the generated obligation always ends
  in `sorry`, and a model with no theorems gets the honestly unprovable `True`
  specification rather than something quietly closable.
- **DRT found three real divergences this round**: report order in
  `copy_violations` (chained key iterators, fixed with a `BTreeSet` union),
  `recorded_of` taking the last match where `state` takes the first, and a
  `region { end }` field that Lean spelled `stop`.
- **`REQ-DRT-GEN.shrink_minimal` and `REQ-DRT-RUST.build_error_explained`** —
  the last two unmodelled clauses, both closed as `@structural` in
  `crates/core/tests/runners_are_generated.rs`. Neither is a function from data
  to data: the first is a property of the shrink *loop* (every reduction step is
  strictly smaller, so it can neither stop early nor run forever), the second
  needs a compiler to produce the message it is about.

Findings now: `Imprecise 17`, everything else `0`. All 11 documents current.
The 17 are Lean files tree-sitter-lean4 0.3.0 cannot parse cleanly (ADR-0008);
there is no newer grammar to move to.

### 07:05 — a shipped keymap, and every suite judged by the coverage module

- **`assets/keymap.json`** — the keymap the project ships, seven modes and
  twenty-four actions, rather than the fixtures `REQ-MYTH` was resting on.
  `keymap::ACTIONS` is the registry it is validated against and
  `keymap::load(text, actions)` is the way in: unreadable text and a readable
  keymap that does not hold together are different failures, and both are
  reported at load rather than on the key that would have triggered them.
  `crates/core/tests/shipped_keymap.rs` presses every letter, digit and a few
  non-ASCII keys against every mode, walks escape back to the root from each,
  and checks that an edited keymap naming an action nobody has is refused.
- **Every differential suite now judges its own generation through
  `drt::coverage`**, via `crates/core/tests/support/mod.rs` (`floors_met`,
  `each_occurred`). The rows of `assert!(count > n)` were coverage floors
  written by hand: each suite reported a shortfall its own way, and none could
  tell a situation the generator never reached from one it reached too rarely.
  The set-membership checks (`seen.contains(...)`) became counters, so "never
  generated" is now the modelled `Vacuous` verdict.
- **`docs/progress.md` rewritten** — it still said the Lean models had not been
  started. `docs/02-scope.md` and `README.md` gained the shipped keymap, the
  per-anchor precision rule, `@structural`, and the coverage floor in what L3
  means.

Findings unchanged: `Imprecise 17`, everything else `0`. All 11 documents current.

### 07:34 — the stage-2 fixpoint, and an anchor that pointed at nothing

Running the whole `--ignored` set for the first time since the anchor work
failed `fixpoint.rs`, and it was right to.

- **A bare `end` was closing the enclosing namespace.** `collect` popped a scope
  on any `end` node, and Lean writes a bare `end` to close a `mutual` block or an
  anonymous section. Every declaration after such a block lost its namespace, so
  `TraceLean.origin` in `Provenance.lean` was anchored as plain `origin` — an
  anchor for a symbol that does not exist under that name. Now only an `end` that
  *names* a scope closes one, and `end A.B` closes both. Recorded in ADR-0008.
- **The rest of the disagreement was the port being right.** Stage 0 stops at the
  type or namespace containing a member, so every method of a type shares one
  anchor; the port names the member (`State::blocks`, `Kind.progress`). The
  member's name cannot be recovered from the type's, so `fixpoint.rs` now
  *matches* a stage-0 anchor against a port anchor that extends it, rather than
  rewriting one spelling into the other, and asserts the rule is still live.
  Lean anchors are compared by the declaration's own name within its file,
  because stage 0 drops namespaces entirely. Both declared in ADR-0007.

`cargo test --test fixpoint -- --ignored` passes.

**And a flake that was a real defect.** Differential suites failed at random
with lake's `could not acquire an exclusive configuration lock`. Each runner has
its own scratch package, but all of them depend on the one lake package in
`formal/`, and cargo runs the suites in parallel. `tests/harness/mod.rs` now
waits for the other build and retries rather than reporting the collision as a
runner that would not build.

### 07:50 — looking at what the grammar cannot read

`tracelean-trace --unparsed <file>` prints the regions of a file the grammar
could not parse, so the `Imprecise 17` can be looked at instead of guessed
about. What it shows settles the question: the regions are ordinary Lean —
subscripted hypothesis names (`h₁`), `∈` in a theorem statement, `<;>` in a
tactic block, an identifier escaped as `«theorem»` because the field is named
after a keyword. `Evidence.lean`'s one capped annotation sits above
`assurance records ∈ chain records`, and the only way to lift it is to stop
saying that. The models stay as they are and the claims stay capped; recorded in
ADR-0008.

### 08:02 — green, end to end

One clean run of everything, after the lake retry:

- `cargo test -p tracelean-core` — **281 passed**, 0 failed.
- `cargo test -p tracelean-core -- --ignored` — **69 passed**, 0 failed, every
  differential suite and the stage-2 fixpoint included.
- `tracelean-trace .` — 36 requirements, 1271 links, 11 documents current,
  findings `Imprecise 17` and nothing else, not blocking.

The port is a proper project by its own definition: every clause is modelled and
differentially tested, proved, or `@structural` with a reason, and the two
kernels agree about this tree modulo the fixes declared in ADR-0007.

### 10:58 — the models rewritten into the grammar's subset, and REQ-VIEW

Two things, both asked for.

**`Imprecise 17` is now `Imprecise 1`.** The claim that the cap could not be
lifted was wrong: every construct tree-sitter-lean4 0.3.0 cannot read has an
equivalent spelling it can, and the equivalent says the same thing. Sixteen
model files were rewritten into that subset — subscripted names, `∈`, `⊕`, `let
rec`, `mutual`-free walks, bit operators by name, `match` where an `if` would
bind a `let` in both branches, hoisted lambdas, `«theorem»`/`«by»`/`«case»`
renamed on both sides of the wire. The full table is in ADR-0008, and each
rewrite carries a comment saying which limit it is written around.

`Command.lean` is the one file left, and deliberately: `apply` and `inverse`
recurse into a batch's members, which Lean 4.12 accepts only with `List.attach`
and `decreasing_by`. Fuel needs a computable size the derived `SizeOf` does not
give, `partial` would not reduce so the round-trip theorems could not be stated,
and `mutual` is equally unreadable. That is a change to the definition rather
than to its spelling, so the file stays capped and ADR-0008 says why.

`tracelean-trace --unparsed <file>` prints the regions, and now also names a
declaration that failed to parse inside a comment or a literal, which the byte
ranges do not reach.

**`REQ-VIEW` is built** (the user approved the requirement mid-run). One
representation both frontends render: a buffer is an identity, a kind and text;
spans over that text say what a region *is* and what can be done there, named
with the actions the keymap dispatches. `View.lean` and `surface/view.rs` carry
`faults`, `actionsAt`, `plainText`, `directoryBuffer`, `delta`/`applyDelta`, and
`conformance` — the last is how a frontend is tested: it answers with what it
drew (`Rendering { lines, offered }`) and `conformance` says what it got wrong.
That is a pure function of two values, so the same check runs against any
frontend that can answer over the DRT protocol. Six differential suites, all
agreeing; `docs/05-view.md` and ADR-0013 record the decision.

### 11:15 — frontends checked, first by what they say and then by what they paint

The question was whether the *actual* frontends can be tested, not just the
representation. Two rungs now exist, and they are different checks.

**Rung 1 — `drt::frontend::check`.** A frontend is a process that reads a buffer
over the DRT protocol and answers with `Rendering { lines, offered }`;
`conformance` says what it got wrong. Nothing in it knows what a terminal or a
web view is, so the same suite runs against either.
`crates/core/src/bin/tracelean-render-text.rs` is the reference frontend, and it
draws wrongly on demand — `--embroider`, `--invent`, `--drop`, `--refuse` — so
the suite has negative controls. Without those it would pass against a checker
that always said yes.

**Rung 2 — `drt::frontend::capture`.** Rung 1 believes the frontend's own
account. `--paint` makes the reference frontend write the bytes a terminal would
receive and say nothing about them; the harness strips the escapes and checks
the text. `--two-faced` is the control that justifies the rung: it reports the
buffer's own text and paints something else, passes rung 1 and fails rung 2.
What a screen cannot carry is what is *offered*, so the two rungs are
complementary rather than one superseding the other. New clause
`REQ-VIEW.screen_is_readable` (11 clauses now), `docs/05-view.md` restamped and
rewritten around the three rungs, the third being how it *looks* — judged by a
person, L2.

`nothing_writes_towards_the_tool` had to be narrowed: it flagged the frontend
reading its *own* stdin, which is being spoken to rather than speaking. The
needles now name giving another process a stdin, not owning one.

### 12:40 — producers, dispatch, and a terminal frontend that is checked

Phases A, B and C of the plan below are done; D is not.

**A — producers (`REQ-SHOW`, 9 clauses).** `formal/TraceLean/Produce.lean` and
`crates/core/src/surface/produce.rs`: `tidy`, `actionsFor`, `fileBuffer`,
`diffLines`/`reviewBuffer`, `menuBuffer`, `recordBuffer`. Eight differential
suites, all agreeing on the first run. The review diff is common-prefix and
common-suffix with everything between removed then added — no heuristics, so the
same two texts always give the same answer.

**B — dispatch (`REQ-ACT`, 7 clauses).** `formal/TraceLean/Act.lean` and
`crates/core/src/surface/act.rs`: `Focus`, `Intent`, `dispatch`, `acts`. Two
differential suites. `a_key_and_a_button_reach_the_same_dispatch` walks the
shipped keymap and checks, for every dispatching binding, that the menu row the
core produces carries the same action the key does and that both resolve to the
same intent — which is `one_path` as a test rather than a claim.

**C — the terminal frontend.** `crates/tui`, crossterm, offline. `draw.rs` is
pure and is the only place the frontend decides what the screen says;
`editor.rs` holds the state and performs intents; `main.rs` is the loop plus
`--protocol` and `--paint`. `crates/tui/tests/conformance.rs` runs both rungs of
`drt::frontend` against the real binary over core-produced buffers.

Two findings the checker raised were real annotation gaps and are closed:
`directoryBuffer` now also models `REQ-SHOW.listing_from_entries` (with its own
binding, because `also_checks` names clauses of the same requirement only), and
`REQ-ACT.dispatch_is_pure` gained a `@tests`.

A stale field name from the earlier `«theorem»` rewrite was found by the full
`--ignored` run: `Strength::{Pinned,Attempted}` carried `theorem` where the model
emits `theoremName`. Renamed to `theorem_name`; both suites pass.

### 14:20 — the TypeScript frontend, checked against the model, and a window

Phase D. The web frontend is a second implementation of the functions that read
a buffer, and it is checked the way a second implementation has to be.

**`REQ-DRT-TS` (6 clauses) and `drt/ts_runner.rs`.** A generated Node process
speaking the same line protocol as the Lean and Rust runners.
`Binding.also_implemented_by` (new) lets one model be the oracle for several
implementations; the binding-shape test holds every one of them to the same
vocabulary, and `config::LANGUAGES` closes the list at the two that have
generators. `tsParameters` is modelled in `Signature.lean` and differentially
tested, so argument order still comes from the implementation's own signature.

**`web/`.** `view.ts` holds `plainText`, `actionsAt`, `declaredActions` and
`conformance`; `render.ts` is the only place the page decides what it says;
`app.ts` builds DOM from it and `frontend.ts` answers the harness from it.
Three differential suites compare the TypeScript with the **Lean model** — not
with the Rust, because two implementations agree perfectly when both are wrong —
and all three agreed on the first run, over cases including text outside the
basic plane. Both conformance rungs run against `node web/src/frontend.ts`, and
`--embroider`/`--invent` prove the harness has teeth against it.

**`crates/editor`.** The editor state moved out of the TUI into its own crate,
because the window drives it too. A terminal and a window each performing
intents their own way would be two answers to what the editor does.

**`crates/desktop`.** Tauri 2, three commands (`shown`, `press`, `act`) and no
decisions. webkit2gtk-4.1 is present, so it builds offline; the icon is
generated by a script rather than pulled from an image toolchain.

**`web/build.mjs`.** Node's own `stripTypeScriptTypes` turns `web/src/*.ts` into
`web/dist/*.js` and rewrites `./x.ts` imports to `./x.js`. No bundler, no
package manager, no lockfile — and nothing between the source that was checked
and the file the browser loads.

**Two things the suites caught, both real.** The stage-2 fixpoint found that
stage 0 was reading `@implements` comments in `web/*.ts` as links while this
kernel, having no TypeScript grammar, was not — so those files now say in prose
what they realise, and the claims live in the binding file and the suites where
they can be placed (ADR-0008's rejected alternative is exactly this). And a
workspace-wide `--ignored` run raced two lake builds against the shared `formal/`
package; `harness::LeanBuild` now serialises them with a lock file rather than
retrying past a corrupted build directory.

Five structural `REQ-DRT-TS` clauses are closed by
`crates/core/tests/ts_runner_is_generated.rs`, including a live check that an
implementation which throws answers for that case and the run continues.

### 16:05 — windowing, editing, and every report answering

The editor is usable rather than demonstrable.

**`REQ-SHOW.window_is_a_buffer` (new clause).** `window buffer start count` in
`Produce.lean` and `surface/produce.rs`, differentially tested. Scrolling is a
core transformation that clips and re-tidies the spans, so a frontend still
draws everything it is given and cannot be shown a buffer it invented a view
of. `a_window_carries_the_spans_that_fall_in_it` and `a_window_is_total`.

**Text editing.** An `Insert` mode in `assets/keymap.json` (`i` enters, Escape
leaves), and in `crates/editor`: `insert`, `delete_back`, `step`, `step_line`,
`line_and_column`, `follow_cursor`, `visible`, `cursor_in`, `key`, `typed`.
Typing goes through `Command::Insert` and `Command::Delete` on the history tree,
so undo and provenance are never special cases. Key handling lives in the shared
crate, which is what stops the two frontends drifting.

**Both frontends redrawn.** `crates/tui` paints `editor.visible(height)` with a
real cursor; `crates/desktop` exposes `shown(height)`, `cursor(height)` and
`press(key)`; `web/app.ts` gained a height, a caret, a resize handler and the
editor's key names. All three delegate to `editor.key`.

**Every report answers.** `lockfile::{path_in, read, write}` and
`drt::config::{path_in, read}` are the two shells that were missing. The editor
now computes `evidence`, `lock`, `rollup`, `stale`, `drt bindings` and
`observed` from the tree, and `drt run`/`shrink`/`coverage` name the command
rather than pretending — the editor starts no processes. A clause's level comes
from the committed lock and never from the tree, so a project without one reads
L1 everywhere and says so. `crates/editor/tests/reports.rs` (8 tests) holds
every report an action can reach to answering something.

**Watching is wired, and starts nothing.** `observe.start` reads the working
tree and derives the difference through `mirror::mutations`; `observe.accept`
pushes those commands through the history tree; `observe.reject` drops them and
leaves the tree alone. No agent is spawned anywhere — `ARCH-NO-DRIVING` holds.

**`tracelean-trace --lock`** writes the committed index, carrying existing
evidence through unchanged. This project now has its own: 40 requirements, 1566
links, 0 evidence records — which is the honest state, because no backend has
written one yet.

**The whole suite, including the differential ones:** 54 suites, 432 tests, 0
failures. `tracelean-trace .` over 197 files: 18 documents current, `Imprecise
1`, not blocking.

**Two usability defects the new tests found, both real.** Typing did nothing at
all: `Insert` had `Normal` as its parent, and the model's rule is that an
unbound key in a submode returns to the root — correct for a mistyped leader,
fatal for text. `Insert` is now parentless and binds Escape itself, and
`shipped_keymap` states the invariant it actually meant (*a mode with a parent
never strands you*) rather than *only the root passes keys through*. And the
leader menu replaced the buffer, so `Space f o` opened the menu row the cursor
had landed on instead of the file; the menu now lives in `editor.menu` beside the
buffer, and all three frontends draw it there.

### 17:30 — evidence is earned rather than annotated

Until now nothing in the project had ever produced an evidence record. The whole
ladder was modelled, differentially tested, and unused: `judge::record` could
make an L2 record and nothing stored it, and nothing anywhere constructed a
`Detail::Drt` or a `Detail::Proof`. Every clause read L1 because every claim was
still a claim.

**`trace::earn`** (pure) turns a backend's result into a record: the level comes
from `coverage::level` and never from the backend, the inputs it depended on are
read out of the index so a change underneath makes it stale, and the record is
keyed to the `@drt` or `@proves` link so retargeting the annotation invalidates
it. `merge` gives one record per requirement-clause-bond slot, so running twice
replaces an answer instead of stacking two.

**`trace::store`** (shell) is `.tracelean/evidence/`, one file per slot — a
directory rather than a document because thirty suites appending to one file is
a race, and the record that loses it is evidence that silently never existed.

**`lockfile::collected`** folds held, earned and swept in that order. Sweeping
last is what lets a fresh record replace a stale one; sweeping first would throw
away the record the new run was written to replace and lose both. Dropped
records come back named, because "went stale" and "never earned" look identical
in a shorter lock file.

**`proofs_are_earned`** builds the models and writes a record for every theorem
the kernel accepted — reading the `declaration uses 'sorry'` warnings and holding
back the declaration that owns each warned line, since recording L4 for an
unproved theorem is the one failure that would make the ladder a lie. It wrote
**88 records into 77 slots**, and `--lock` now carries them.

**What this exposed: L3 is unreachable as the project stands.** `coverage::level`
grants it for agreement *and* a met floor **the binding declared** — and none of
the 86 bindings declares one. The floors are written in the generation tests
instead, because a situation is a Rust predicate over generated values and could
not simply move into JSON. So `verdict` answers `Undeclared` for every binding.
Closing it is `REQ-DRT-COVER.floor_stated`: the binding names its situations and
their floors, and the run counts those named situations as it goes, so one run
establishes agreement and coverage together instead of two sibling tests each
establishing half. The reasoning is recorded at `support::WHY_NO_L3_RECORDS`.

**README.** `reqs/README.md` indexes all forty requirements by area with clause
counts; the top-level README links it, links the four surface requirements
inline, documents how a level is earned, and its reading order now includes
06/07 and progress.

### 2026-09-17 — L3 is reachable, and 35 bindings reach it

The entry above ended by saying L3 was unreachable because no binding declared
coverage floors. It does now.

**`support::agreed` / `support::covered`.** L3 needs two facts that this
project's suites establish in two different processes: the runs agreed, and the
generator reached the situations the binding declared. Neither test can see the
other's result, so each writes its half to `.tracelean/pending/<op>.json` and
whichever lands second composes the record. Composition is order-independent —
there is no "first" test.

`agreed` takes the `DrtResult` rather than the facts, so a suite cannot report a
run other than the one it did: op, seed, case count and divergence all come from
the run. The agreement half is a map keyed by seed, not a flag, because
`also_implemented_by` binds a second frontend to the same model and both are run
against it; a record written after one of them agreed would say a clause is
checked when half of it is. Composition therefore waits for one agreeing run per
declared implementation and names the lowest seed. (This was a real bug in the
first design, found by an adversarial read of the flag version.)

`covered` refuses a count against a situation the binding never declared — that
is a counting error, not extra credit.

**Every differential suite migrated.** All 21 suites now call `support::agreed`
instead of a hand-rolled `assert!(result.agreed(), …)`, and every
`generation_reaches_…` test that has a binding calls `support::covered` instead
of `floors_met`/`each_occurred`. **34 of 86 bindings declare `floors`** in
`.tracelean/drt.json`.

Two suites' counts were split across two ops, because one loop was exercising
two bindings: `differential_produce.rs` (`tidy` → `well_formed_by_construction`,
`diff_lines` → `review_from_change`) and `differential_view.rs` (`faults` →
`text_is_the_content`, `delta` → `changes_are_deltas`). Reporting both against
one op would have claimed a law was exercised where it never ran.
`differential_act.rs` was deliberately **not** migrated — its coverage call is
`@structural REQ-ACT.one_path` with no binding behind it.

**Strength.** `formal/TraceLean/Pinned.lean` proves `pins_assurance`, the
`REQ-EVID.weakest_link` specification-strength obligation, and the kernel
confirmed it: 1 pinned, 127 open.

**Judging.** `tracelean-trace --judge REQ.clause` prints the prompt; with
`--verdict/--by/--note` it records the person's decision and attributes it to
them. `.tracelean/judging/REQ-EVID.md` holds a tool's *advice* for REQ-EVID
(6 agrees, 1 drift on `bonds_separate`, 1 note on `ladder`) — advice is not
evidence (ADR-0003), so nothing has been entered as a judgement.

**Two usability defects found and fixed**, both by new `crates/editor/tests/`:
typing was completely broken (`Insert` had a parent, so `keymap::step` answered
`Leave{root}` for every letter — `Insert` is now parentless and binds Escape
itself), and the leader menu replaced the buffer instead of sitting beside it
(`editor.menu` now sits beside it; all three frontends draw it).

**Ideas** are written down in [docs/08-ideas.md](docs/08-ideas.md), including
the measured performance finding: a full `--include-ignored` run is ~1364s
across 55 suites, and **68 `harness::lean_runner` sites mean 68 separate lake
builds of the same package, serialised behind one lock**. `main_lean` already
dispatches on `op` over a list of entries, so one shared runner is tractable and
is the highest-value performance change available.

### 2026-09-17 — one Lean runner, and floors on two thirds of the bindings

**Coverage floors: 20 → 54 of 86 bindings.** Sixteen new `generation_reaches_…`
tests across rollup, evidence, judge, lockfile, command, hash, doclink, produce,
view and keymap, each stating its law directly against the implementation as
well as counting situations — inverting a batch equals its members inverted in
reverse order; a claim about an unscanned file never moves the coverage figure;
every producer's output has no faults.

The floors are chosen from what the law needs, not from what the generator
happens to give. Two of them were not met, and both were fixed by moving the
generator rather than the number:

- **`REQ-EVID.record_reproducible` was `Vacuous`.** The generator drew input
  names from `["model", "code"]`, but `required_inputs` asks for `requirement`,
  `implementation` and `toolchain`. *No* generated record could ever be
  reproducible, so the agreement on that binding was about the failing branch
  only. Alphabet corrected to the names actually required, list lengthened to 8
  so carrying both is ordinary rather than lucky.
- **`REQ-SHOW.file_from_text`** and **`producer_is_pure`** were `Short` on two
  situations by 20–30%; those floors were genuinely optimistic and were lowered.

**A kernel disagreement, found by `fixpoint`.** Stage 0 read a *backticked*
`@proves REQ-EVID.weakest_link` in a doc comment in `formal/TraceLean/Pinned.lean`
as a real annotation; the port did not. Fixed by not writing a directive-shaped
token in prose — a directive in prose is a directive as far as a scanner is
concerned, and the file now says so.

**One Lean runner instead of sixty-eight.** `lean_runner::main_lean` now takes a
list of imports (sorted, deduplicated, so the same set always produces the same
bytes). `harness::shared_lean_runner` generates one package carrying an entry
for every binding — read from `.tracelean/drt.json`, which already declares each
binding's import, function and arguments — at `target/tracelean-drt-lean`, and
generates *and* builds it under one `LeanBuild` guard, because the package is
shared between processes. `write_if_changed` makes every later process's build a
no-op.

A call that is not a declared binding's own call still builds its own package:
`differential_evidence.rs` binds the wrong function deliberately, and a shared
runner answering it with the right one would turn a test of the harness into a
test of nothing.

### 2026-09-20 — the screen: several buffers at once

The surface could show one buffer, because `shown` answered with one and there
was no value that said otherwise. `REQ-SCREEN` is that value: buffers opened, a
layout placing some of them, which pane has focus, and the two menus a session
always has. Planned in [docs/10-screen.md](docs/10-screen.md), which was written
first and checked against what the surface is meant to be.

- `reqs/surface/REQ-SCREEN.md`, 14 clauses, refining `REQ-VIEW` and `REQ-SHOW`.
- `formal/TraceLean/Screen.lean` — `Layout` is recursive, so its JSON encoding
  is hand-written to match serde's externally tagged form. Three theorems.
- `crates/core/src/surface/screen.rs` — 13 unit tests.
- `crates/core/tests/differential_screen.rs` — ten bindings, **2,000 cases each,
  no divergence**, plus a generation test per binding.

All 14 clauses now carry `L3`; `opened_outlives_shown` and
`stations_are_constant` also carry `L4`. The advice for the judging bond is in
`.tracelean/judging/REQ-SCREEN.md`, and it reports two clauses as drift.

**A pane has its own identity.** The first draft had focus name a buffer, and
`split_keeps_the_buffer` puts one buffer in two panes — so focus could not say
which half it meant. Panes carry ids minted from a counter the screen holds.

**What the comparison could not find.** All ten differential runs passed first
time. Asserting the *laws* over the same generated cases then failed three:
`place` left a hole wherever a part placed nothing (both sides had the same
hole, so they agreed); `split` hit every pane sharing a name rather than one;
and `focus_follows_geometry` has no answer under duplicate names, which is a
precondition and is now recorded as one. Two implementations written from the
same misunderstanding agree perfectly — the law is the part that does not.

### 2026-09-20 — both frontends draw the screen

`Space w` opens the screen menu: `v`/`s` split, `q` closes, `hjkl` move the
focus, `+`/`-` resize, `o` shows the buffer under the cursor, `t` opens the
station under it.

- **`Intent::Arrange`** carries a named change; `Screen.arrange` is the one
  function that performs it. That is `one_arrangement_path`, a sixteenth clause,
  added because a key and a pointer would otherwise grow separate arithmetic —
  and the window's `grab` command proves the point, sending a drag measured in
  characters into the same function `+` reaches.
- **`Editor` holds a `Screen`.** `screen.opened` is the only store of buffers;
  `buffer()` reads the focused pane's out of it. Each pane keeps its own cursor
  and first line, parked when the focus leaves it.
- **`Editor::laid_out(region)`** hands over each pane with its buffer *whole*.
  A frontend reserves its own chrome — a terminal spends a column on a divider,
  a window spends none — and windows the buffer to what is left through the
  core's `window`.
- **The terminal** composes panes into one grid of `(character, role)` cells, so
  colour survives composition. **The window** positions each pane in `ch` and
  line heights, the units the editor laid it out in.

**The join nothing else crosses.** Every part of the arrangement is modelled and
differentially tested, and all of it would still hold of a frontend that laid
two panes out and drew one. So `crates/tui/tests/driving.rs` presses
`Space w v` into a real pseudo-terminal and reads the divider off the screen,
and a second test presses `Space w q` and checks it is gone. Both were watched
failing with the divider drawing removed.

### 2026-09-20 — the bars, and what a station found

Both bars are on the screen from the first frame: the stations, then what is
opened. `Space w t` opens the stations (`p`/`s`/`r`/`d`); `Space w b` shows the
opened set as a buffer, which is how `Space w o` reaches a target from a bare
keyboard.

- **A station is an action of its own** (`screen.station.project` and three
  more) rather than one action taking the name as a target. A target comes from
  what the cursor is on, so a single `screen.station` would have been reachable
  only once a station row was under the cursor — which is only true after you
  have reached the stations, which is what the action was for.
- **`focus_in(buffer, offset)`** moved out of `Editor`: a click on a bar is
  resolved against the bar's own buffer. Against the pane it would have done
  something, just not the thing clicked.
- **The window** draws each bar as a row of items with an emblem *beside* each
  station's name. Beside and not instead: the page still contains the buffer's
  text, so no clause has to move. The iconic version is phase D, which is where
  it belongs because that is the one needing the rule changed.

**What the station found.** The requirements station opened and showed the whole
tree: `produce` matched `BufferKind::Directory { .. }` and threw the path away,
so every directory ever asked for gave the same listing. `listing_from_entries`
held of the function that was called — it was the caller that never varied, and
nothing noticed while only one directory was ever asked for. Now
`listing_of(path)` lists what is under the path it was given.

**Still empty:** `design` and `sandbox` open a record with no producer behind
it. Phases E and F.

**A limit written down rather than discovered.** `drt::schema` has no recursive
form, so a layout's schema is nested by hand three deep. Layouts deeper than
that are never generated and no floor here claims otherwise; the fix belongs to
`REQ-DRT-SCHEMA`.

### 2026-09-20 — the rule changed, and the bar is emblems

`REQ-VIEW` gained `presentation_may_be_symbolic`; `screen_is_readable` now says
*accessible* text. The window paints 📁 🧪 📋 🕸 and no words. The terminal
paints the words, which is `rendering_is_total`: a frontend without glyphs draws
the text rather than nothing.

The line moved from **painted** to **named**. A region may be painted as
anything so long as it carries the buffer's own text as its accessible name, and
a screen is read by those names — the same string a screen reader announces, so
the check and the person get one guarantee instead of two.
`Presented { painted, name }`, `accessible`, `presentedConformance`;
`a_symbol_is_read_as_its_name` is the clause as a theorem (repaint every region
with one glyph, no verdict moves).

- **The emblem is keyed by action, not by role.** The plan said role → symbol.
  A role cannot tell four stations apart — they are one role — so all four
  would have got the same glyph. The action is what distinguishes them and it is
  already declared, dispatched and governed. The previous bar keyed its emblem
  off `piece.text.split(" ")[0]` under a comment claiming otherwise, which was
  the frontend pattern-matching text it recognised: the very thing the design
  rejected.
- **`shown(buffer)` is one decision with two consumers.** The page builds its
  DOM from it; `screenText` is it joined. Painting one thing and reporting
  another would mean disagreeing with a value computed once.
- **The control is `--mislabel`**, which names each painted region after its own
  glyph. The capture harness fails it. Without that the permission would be
  unfalsifiable.
- **The generator was fixed, not the floor.** Drawing `painted` and `name`
  independently from wide alphabets made them almost never equal, so the
  ordinary case — a region with no emblem — appeared 33 times in 3,000. Small
  alphabets and `max_len: Some(0)` lifted it to 352.

**Still a person's:** whether a glyph *means* its name. `🗑` for `project` is
conformant. That is L2, and `.tracelean/judging/REQ-VIEW.md` says so.

### 2026-09-20 — every station holds something, and the harness was stale

Phases E and F. `Unmodeled` is gone: `stationKind` is the one function saying
what a station stands for, `Act` dispatches through it, and
`every_station_produces` is the theorem that every row on the bar leads
somewhere.

- **`Role::Level` carries its grade.** It was one role for any level, so a
  frontend had to read `L3` out of the text to colour it — a frontend parsing
  the buffer, which is the one thing `REQ-VIEW` forbids. Both frontends now
  paint the four apart from the same value.
- **Two producers, two `REQ-SHOW` clauses.** `index_from_requirements` (the
  requirement set, one row each, carrying the level reached) and
  `graph_from_refinement` (the same set, indented from the roots, bounded
  because `refines` is data and a cycle in it is representable).
- **`REQ-COST`, four clauses, all `L3`** and `estimate_is_labelled` also `L4`.
  Naturals throughout — prices per million tokens, amounts in millionths —
  because a floating-point total makes two implementations disagree in the last
  digit for reasons that have nothing to do with pricing. An unknown model is
  named *unpriced*, never priced at zero.
- **The transcript reader learned to count.** A tool's accounting records carry
  no text, so every one of them was landing in `unrecognised`. `Read` now
  carries `usage`. There is still no field a command could come out of.
- **`observe::watch`** is the new shell: two reads, no decisions.

**What the payload broke.** A role is an object in the page's type now, and
`===` on an object compares identity — so the obvious comparison cuts every
character of a graded span into its own region, draws exactly the same text,
and passes every check that existed. `frontend.ts --regions` answers with how
many regions each line was cut into; one test asks for two adjacent spans of one
grade and requires one region. Watched failing with the comparison broken.

**The harness had a stale link.** The shared Lean and Rust differential runners
are built once and reused across fifty-eight processes, guarded by a mark of
what they were built from — and the mark held the *bindings*, not the sources.
Editing a model and running the suites compared a new implementation against a
runner built from the old one: not an error, a disagreement. Worse on the
implementation side, where two stale runners that both fail to read a changed
shape *agree*, because two runners failing identically is agreement — so a
suite went green while neither side had been asked the question. Found the day
`Role` gained a payload. The mark now carries a digest of every source the
runner would be built from.

A smaller one fell out of it: `shrink_divergence` numbered its cases from
`u64::MAX / 2`, which a JavaScript runner cannot echo — doubles lose integers
above 2^53 — so every divergence against the TypeScript side came back as a
protocol error instead of a counterexample. It counts from 2^50 now.

## Next

1. **`Imprecise 1`** — `Command.lean`, not closable without changing what the
   definitions say (ADR-0008).
2. **31 of 106 bindings still declare no floors**, so they stay at L1. Each needs a
   `generation_reaches_…` test that names its situations.
3. **`REQ-DRT-COVER`'s own text needs a person's decision** — not edited here,
   because requirement text is the top of the ladder and rewriting it changes
   what everything below claims to satisfy. Three things about it:
   - Its frontmatter says `status: approved`; its body says it is `draft`.
   - Its body says `law_coverage` and `vacuous_named` are open work.
     `vacuous_named` is now built — `Verdict::Vacuous` is a named verdict and it
     fired for real, on `REQ-EVID.record_reproducible`.
   - Its last paragraph is **still correct and still unaddressed**: a floor
     stated per *binding* is insufficient, because "a protected path is never
     written" is satisfied perfectly by a run in which no case named a protected
     path. `support::covered` states floors per binding. Stating them per law,
     over that law's precondition, is the design that is not yet done.
4. **The Rust side has the same problem the Lean side just lost**: 67
   `cargo build --release` runs of generated crates that differ only in which
   function they call.
5. **`drt::config::Binding` describes the implementation side only.** The
   `model` key exists in `.tracelean/drt.json` and has no field to land in, so
   `harness::declared_models` parses it as raw JSON and nothing in Rust checks
   its shape.
6. **127 strength obligations are `open`.**
7. **Nothing has been judged.** `--judge` is ready; a person is not.
8. **The desktop window has not been opened by a person.** It builds, its
   commands are the editor's, and the page it loads is conformance-checked —
   but no one has looked at it, which is exactly the L2 rung the ladder
   reserves for a person.
9. **`REQ-SHOW.graph_from_refinement` is reported as drift** and nobody has
   ruled on it. The clause promises that a requirement refining something
   appears under it; the walk is bounded from the roots, so a requirement no
   root reaches appears nowhere. A rewording is suggested in
   `.tracelean/judging/REQ-SHOW.md`. It gives up a guarantee, which is why it
   is a person's and not an edit.
10. **The price table is a fact about the outside world.** Nothing here can
    check that `.tracelean/prices.json` holds the prices anyone charges.
11. **The project map, the Lean infoview and a terminal panel** are still not
    restored — see the table in [docs/10-screen.md](docs/10-screen.md), which
    says why for each.

### Superseded plan — the four phases

**Building the editor itself.** The representation exists and is checkable; what
is missing is everything that produces it, everything that acts on it, and both
frontends. Four phases, each usable before the next starts.

**A — producers (`REQ-SHOW`, written, not built).** Nothing in the core turns
real state into a buffer: `directoryBuffer` is the only producer and it takes its
entries as an argument. Needed, in `formal/TraceLean/Produce.lean` and
`crates/core/src/surface/produce.rs`:

- `tidy` — clamp, drop backwards, sort, drop overlaps. One normaliser used by
  every producer, which is what makes `well_formed_by_construction` true for
  *generated* states rather than for the ones somebody tried.
- `actionsFor : Role → List String` — the role-to-affordance policy, one place.
- `fileBuffer path text marks` (marks come from the parser, in Rust).
- `reviewBuffer target before after` — a line diff by common prefix and suffix;
  removed then added, no heuristics, deterministic.
- `menuBuffer title entries`, `recordBuffer title events`.

**B — dispatch (`REQ-ACT`, written, not built).** The keymap turns a key into an
action *name* and a span carries action names; nothing turns a name into
anything. Needed: `Intent` = `show | edit Command | travel | observe | refuse`,
and `dispatch (action) (focus) : Intent` where `Focus` is the buffer, the offset
and the text of the span under it — that is what makes `file.open` mean a path.
Same call from a key and from a button, which is `one_path`.

**C — the terminal frontend.** A new shell crate (`crates/tui`), crossterm, which
resolves offline. Raw mode, key → keymap → dispatch → producer → `Buffer` →
paint. It must also answer headlessly (`--protocol`, `--paint`) so
`drt::frontend::{check, capture}` run against the real binary, not a fixture.

**D — the TypeScript frontend + Tauri.** The DRT protocol is language-agnostic
(a runner is a `cmd`), so the TS renderer becomes a *third runner* checked
against the same Lean model: `drt/ts_runner.rs` alongside `rust_runner.rs`, and
`Binding.implementation` grows to a list so one clause can name both a Rust and
a TypeScript implementation. Node 22, `tsc`/`vite`/`vitest` and the `tauri`
crates are all present offline.

Also still open, unchanged:

- **`Imprecise 1`** — `Command.lean` only, not closable without changing what
  the definitions say (ADR-0008).
- **Strength obligations are all `open`.** `obligationSource` generates the
  question and leaves it in `sorry`; nobody has answered one yet.

### Superseded plan

1. **Sandbox observation** (`REQ-SBX`, `REQ-MIRROR`, `REQ-SELFWRITE`,
   `REQ-TRANSCRIPT`) — the one agent-facing feature that survives the scope cut.
   The tree diff is pure, so it is a direct DRT target.
2. **`REQ-MYTH`** — a pure total state machine; the laws are what fix it.
3. **Lean models for the trace kernel** — 201 clauses unmodelled; the pure ones
   (annotation grammar, staleness, rollup) are modellable now.
4. **`REQ-DOCLINK` and `REQ-STRENGTH`** implementations.

### Superseded plan

1. **History** (`REQ-CMD`, `REQ-UNDO`, `REQ-PROV`, `REQ-PERSIST`). Self-contained,
   and the command round-trip law is the best remaining DRT target.
2. **Lean models for the trace kernel.** 207 clauses are unmodelled; the pure
   ones (annotation grammar, staleness, rollup, evidence) are modellable now.
3. **Sandbox observation** (`REQ-SBX`, `REQ-MIRROR`, `REQ-SELFWRITE`).
4. **Surfaces** (`REQ-MYTH` first — it is a pure total state machine).

### Older plan (superseded by the entries above)

1. **Trace kernel.** `REQ-ANNOT` (annotation grammar), `REQ-ANCHOR` (anchors and
   body hashing), `REQ-REQDOC` (frontmatter, refinement DAG). These make the tree
   self-describing and are the path to stage 2.
2. **`REQ-STALE`** — invalidation. Pure, and the soundness core.
3. **`REQ-CHECK` + `REQ-ROLLUP`** — findings and honest percentages.
4. **`REQ-LOCK`** — the committed index, then the stage-2 fixpoint comparison.
5. Then history (`REQ-CMD` round-trip law is the next best DRT target), then
   sandbox observation, then Myth / LSP / judge.

## Gotchas

- Parallel Lean runner builds contend on the one lake package in `formal/`.
  `tests/harness/mod.rs` retries on `configuration lock`; if a new build path
  appears, it needs the same treatment.
- `formal/.tracelean-build.lock` is held by whichever suite is building the
  shared Lean package. A run killed mid-build leaves it behind, and every later
  suite then waits out the full five-minute bound. `LeanBuild::acquire` now
  writes its pid into the lock and reclaims one whose process is gone; if a run
  ever stalls on "has been running for over 60 seconds" with no `lake` process
  alive, delete that file.


- Clause keys may contain only `[A-Za-z0-9_]`. A hyphen silently truncates the
  clause at the hyphen — `REQ-X.a-b` resolves to clause `a`. Use underscores.
- Generated runner: deserialize arguments with `serde_json::from_str` over
  `&RawValue` slices, **not** `from_value`. `from_value` requires
  `DeserializeOwned` and breaks every `&str` parameter (ADR-0006).
- In `String::from("...")` literals in the generator, `{{` is *literal* braces,
  not an escape — only `format!` escapes them. Cost one build failure.
- Annotations attach to the *next declaration*. A `//!` module-level annotation
  in a test file anchors to whatever function comes first, not the module.
- `tools/stage0-trace` is excluded from the workspace on purpose; it pulls the
  whole stage-0 dependency tree (~2.5 min cold build). Nothing in `crates/` may
  depend on stage 0.

## Key paths

| What | Where |
|---|---|
| Methodology | `docs/00-methodology.md` |
| Bootstrap ladder | `docs/01-bootstrap-ladder.md` |
| Scope cuts | `docs/02-scope.md` |
| Doc-sync (hash-linked docs) | `docs/03-doc-sync.md`, `reqs/trace/REQ-DOCLINK.md` |
| Decisions | `docs/decisions/ADR-00{01..13}-*.md` |
| Requirements | `reqs/{arch,drt,trace,history,agent,surface}/` |
| Runner generators | `crates/core/src/drt/{rust,lean,ts}_runner.rs` |
| Signature parser | `crates/core/src/drt/signature.rs` |
| End-to-end proof | `crates/core/tests/rust_runner_end_to_end.rs` |
| Editor state, shared by both frontends | `crates/editor/src/lib.rs` |
| Editor behaviour under keys | `crates/editor/tests/{editing,reports}.rs` |
| Producers and dispatch | `crates/core/src/surface/{produce,act}.rs` |
| What is opened and what is shown | `crates/core/src/surface/screen.rs`, `formal/TraceLean/Screen.lean` |
| The plan for the surface | `docs/10-screen.md` |
| Terminal, window, page | `crates/tui/`, `crates/desktop/`, `web/` |
| The keymap, as data | `assets/keymap.json` |

## Commands

```
cargo test --workspace                                         # everything fast
cargo test --workspace -- --ignored                            # the differential suites
cargo run -p tracelean-core --bin tracelean-trace -- .          # trace this tree
cargo run -p tracelean-core --bin tracelean-trace -- . --lock   # write .tracelean/trace.lock
node web/build.mjs && cargo run -p tracelean-desktop            # the window
cargo run -p tracelean-tui                                      # the terminal
./tools/stage0-trace/target/release/stage0-trace .              # the stage-0 driver
```
