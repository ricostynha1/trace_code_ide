# Working state — traceability implementation (weekend run)

Live status file. Read top-to-bottom; last section is what is currently true.

## What is being built
The design in `docs/formal_traceability_and_lsp_plan.md`: requirements linked to code by
comment annotations (no filenames), an executable Lean reference model, differential
testing binding model↔code, an LLM judge binding requirement↔model, an evidence ladder,
and a checker.

## Topics and status

| # | Topic | Plan doc | Status |
|---|---|---|---|
| T1 | Traceability core (annotations, anchors, requirements, checker, lockfile) | `docs/plan_t1_traceability_core.md` | **done** — builds, 64 new tests pass |
| T2 | Differential testing (protocol, model runner, adapters, generators) | `docs/plan_t2_differential_testing.md` | **done** — 36 tests pass |
| T3 | Requirement↔model judge | `docs/plan_t3_judge.md` | **done** — 34 tests pass |
| T4 | Hierarchy, roll-up, zoom, UI | `docs/plan_t4_hierarchy_zoom_ui.md` | **done** — panel, treemap, gutter chips, commit trend |
| T5 | LSP + Myth | `docs/plan_t5_lsp_myth.md` | **done** — client, providers, mode stack, edit lowering. Live diagnostics deferred by design |

## Log

### 2026-09-11
- Design phase completed earlier in the session; two documents produced
  (`formal_traceability_and_lsp_plan.md`, `verification_backends_considered.md`).
- `ROLLOVER_traceability.md` written for zero-context handoff.
- T1 plan written and sent to two reviewers.

### T1 — traceability core: DONE

**What now works.** A requirement is any `.md` anywhere carrying an `id:` in frontmatter
(the older `# REQ-01: Title` + `Status:` form still parses). Code, models, tests and
harnesses link to it by comment annotations — `@models`, `@implements`, `@tests`, `@drt`,
`@proves`, qualified by `@partial` and `@exempt`. Every filename and directory convention
is gone.

**Files added** (`tracelean/core/src/trace/`):

| File | What it does |
|---|---|
| `annotation.rs` | Finds annotations in tree-sitter comment nodes; roles, clauses, attributes, `begin`/`@end` regions, qualifiers |
| `anchor.rs` | Resolves each annotation to a symbol path by walking the syntax tree; hashes the normalized body |
| `requirement.rs` | Frontmatter parsing (hand-rolled, no YAML dependency) with a legacy-format fallback |
| `evidence.rs` | Per-bond evidence records, levels L1–L4, weakest-link assurance |
| `checker.rs` | 19 named finding classes, project policy, CI gate rules |
| `lockfile.rs` | Deterministic `.tracelean/trace.lock.json`; evidence preserved across scans |
| `hash.rs` | Two hashes: `body_hash` (staleness) and `link_hash` (evidence validity) |
| `mod.rs` | `TraceIndex`, the scan pipeline, and the views the UI needs |

**Files changed**
- `core/src/trace_graph/scan.rs` — `link_by_convention` replaced by `link_from_annotations`;
  `test_to_source_path` and `integration_test_to_req_id` deleted outright.
- `core/src/requirements.rs` — `list_requirements` now walks the whole tree via the trace
  index; `has_spec` means "something carries an `@models` annotation", not "a file named
  after it exists in `specs/`".
- `core/src/service.rs` — `trace_scan`, `trace_findings`, `trace_requirement`, `trace_overview`.
- `gui_backend/src/ipc/trace.rs` + `lib.rs` — the four commands above exposed to the frontend.
- `core/Cargo.toml` — added `sha2 = "0.10"` (already in `Cargo.lock` transitively, so no new
  compile cost).
- Root `.gitignore` — `.tracelean/*` with negations, so `trace.lock.json`, `trace_policy.json`
  and `drt.json` are versioned while runtime state stays ignored.

**Design decisions worth knowing** (all recorded in the plan doc's two revision sections)
- Anchors walk the tree-sitter tree directly rather than using `parser::SymbolTable`, because
  that table only extracts top-level items — a method inside an `impl` would otherwise get no
  anchor at all.
- Comment node kinds are matched exactly, and matched nodes are not descended into: Rust nests
  `doc_comment` inside `line_comment`, so a `contains("comment")` walk counts `///` annotations
  two or three times.
- Body hashing strips comment ranges (so editing an annotation cannot invalidate its own
  evidence) but preserves whitespace inside string literals (so `"a  b"` ≠ `"a b"`).
- Evidence carries *all* its input hashes, not one anchor hash — otherwise editing the model
  alone would leave differential-testing evidence looking valid.
- `@partial`/`@exempt` became qualifiers on a claim rather than roles of their own, and
  `@refines` was dropped: the refinement DAG comes from frontmatter, because code should not be
  able to reshape the requirement hierarchy.

**Tests**: `tracelean/tests/unit/test_trace.rs`, 47 tests, plus a fixture project at
`tracelean/tests/fixtures/trace_project/`. `cargo test -p tracelean-tests` → 176 pass.

**Pre-existing failures, unrelated to this work**: `ai::model_catalog::tests::
enrich_surfaces_ranking_for_a_region_prefixed_bedrock_claude_model` and
`region_prefixed_claude_entries_carry_the_same_ranking_as_their_bare_sibling` fail on a clean
checkout too (verified by stashing this work and re-running).

**Not done in T1, by design**: no differential testing, no judge, no UI changes — the
Requirements panel still renders the old list. T4 rewires it.

### T2 — differential testing: DONE (backend)

**What now works.** A requirement clause can be bound to a Lean model runner and an
implementation runner in `.tracelean/drt.json`; TraceLean generates cases from a declared
input schema, sends each to both sides over a line-delimited JSON protocol, compares the
replies, shrinks any disagreement to a minimal input, and records an evidence entry.

**Files added** (`tracelean/core/src/drt/`):

| File | What it does |
|---|---|
| `protocol.rs` | `Case`/`Reply`, the long-lived `Runner` subprocess, timeouts, restarts, stderr capture, and the comparison rules |
| `schema.rs` | The declared input grammar and constructor/variant accounting for coverage |
| `gen.rs` | Deterministic generation (inline xorshift PRNG) and structural shrinking |
| `run.rs` | Orchestration, divergence triage, coverage floor, evidence mapping, seed corpus |
| `config.rs` | `.tracelean/drt.json`, binding lookup, adapter scaffolding for Rust and Python |
| `lean_runner.rs` | Lakefile + runner module generation, `lake build`, toolchain detection |

**Decisions that changed from the original plan, and why**
- **No `--emit-schema` from Lean.** Lean has no runtime type reflection; extracting a schema
  needs a `MetaM` metaprogram and is undecidable with type parameters. The input shape is
  *declared* in `drt.json` over a whitelisted grammar instead.
- **The derived JSON encoding is pinned in tests.** Lean's `deriving ToJson` writes a nullary
  constructor as a bare string (`"weakPassword"`), a one-field constructor as `{"C": v}`, and
  omits a `x?` field that is `none`. The comparator treats absent as null and compares parsed
  values, never strings.
- **Numbers are canonicalized before comparison** rather than enabling serde_json's
  `arbitrary_precision` crate-wide, which would change `Number` handling for every other user
  of the crate in this workspace.
- **No proptest for generation.** Its shrinker is typed and its regression files fight the
  "seed → identical case list" guarantee an evidence record depends on. Generation and
  shrinking are hand-rolled with an inline PRNG; proptest stays in unit tests.
- **No Lean source instrumentation for branch coverage.** Rewriting `match`/`if` in the user's
  model would break `termination_by`, derived instances and proofs — and would change the
  anchor hash of the very thing being measured. v1 ships input-constructor and output-variant
  coverage computed on the Rust side, which needs no instrumentation.
- **A divergence is triaged, never auto-filed.** It can be an implementation bug, a model bug
  or an adapter bug; `Triage` records which, and the default is `Untriaged`.

**Tests**: `tracelean/tests/unit/test_drt.rs`, 36 tests. The Lean side is stood in for by small
Python runners, which exercises the real subprocess path — spawning, framing, flushing,
timeouts, restarts, crashes — without needing a Lean toolchain. The headline test plants a
classic off-by-one (`> 8` against the model's `>= 8`) and asserts that every reported
divergence shrinks to an exactly-8-character password.

**T2 gap now closed.** `service::drt_generate_runner` derives the whole model runner from the
`@models` annotations: it reads each anchored `def`, extracts its name and binder names,
resolves the Lean module from the file path, finds the nearest lakefile to `require`, emits the
dispatch (arguments read out of the input object *by name*, so a mismatch between the declared
schema and the model's binders is a readable error rather than silent nonsense), and builds it
when a toolchain is present. A declaration it cannot parse is refused outright rather than
guessed at — a wrong guess would compile and answer the wrong question.

### T3 — the requirement ↔ model judge: DONE (backend)

**What now works.** A clause and its `@models` Lean source are sent to a model with a
versioned prompt; the reply is parsed strictly; a drift claim is then **executed against the
compiled model** and discarded if the model does not actually behave as claimed.

**Files added** (`tracelean/core/src/judge/`):

| File | What it does |
|---|---|
| `prompt.rs` | The versioned prompt (`judge/v1`), rendering, and fencing of untrusted text |
| `verdict.rs` | Verdict/witness types, fence-tolerant JSON extraction, strict validation |
| `witness.rs` | Runs the witness through the model runner and confirms or falsifies the claim |
| `calibration.rs` | Seven built-in fixtures across the mutation classes, and the scoring gate |
| `mod.rs` | Orchestration, repair turn, budget check, evidence mapping, provider construction |

**The rules that make an LLM verdict usable here, all enforced in code rather than in the
prompt**
- A drift verdict **without a witness is rejected at parse time**. An unverifiable assertion
  never reaches the evidence store.
- A witness must carry `model_produces_json`, not just prose, because prose cannot gate an
  error-severity finding. Comparison uses the same structural JSON equality as differential
  testing.
- A witness whose claim about the model is **falsified is discarded**, retried once, and after
  a second falsification recorded as `judge-unreliable` — a fact about the judge, never a
  requirement finding.
- Without a model runner the verdict is **degraded**: recorded, but incapable of producing a
  drift finding.
- `Agrees` writes **L2 and nothing more**; absence of a witness is not proof of absence, so it
  can never promote a link to conformant or proved. Every other verdict writes L1.
- The repair turn **appends the parse error**; retrying an identical prompt at temperature 0
  would reproduce the same unusable text.
- Requirement text and Lean source are fenced with a fence longer than any backtick run they
  contain, and labelled as data — a requirement cannot instruct the judge.
- The spend cap is checked **before** the call, because the provider layer does not enforce it
  (the cap lives in the agent loop, which the judge does not run inside).

**Calibration**: the suite's gate is deliberately asymmetric — **zero false `agrees` on
deliberately mutated pairs**, while false alarms are reported but do not fail. A missed drift
is silent and corrodes every green badge; a false alarm costs a human a minute.

**Tests**: `tracelean/tests/unit/test_judge.rs`, 34 tests, driven by a scripted provider that
answers per fixture (the existing mock is interactive and its headless mode returns one fixed
string for every call). The Lean model is stood in for by a Python runner, so witness
execution is exercised for real.


### T4 — hierarchy, zoom, UI: DONE

The panel (`react_frontend/components/TracePanel.tsx`) renders the refinement DAG as a
tree with five zoom levels, an assurance *chain* per requirement (never one badge — see
the weakest-link rule), coverage bars that print `≥` when the denominator is open, and a
treemap in which untraced files are grey rather than absent.

Two pieces landed in this pass:

**Gutter chips.** `trace_links_in_file` feeds a CodeMirror gutter
(`traceGutter`/`TraceChip` in `Editor.tsx`): one chip per annotated line, glyph and colour
by role (`M` model, `I` implements, `T` tests, `D` DRT harness, `P` proof), tooltip naming
the requirement and clause, click opening that requirement in the panel. The chips clear
themselves on any document change, because the index is rebuilt from disk and stale line
numbers next to live code would be a lie rather than a lag.

**Progress over commits** (`core/src/trace/history.rs`, `trace_history`). `git log` gives
the commit list; each commit is materialized with `git archive` into a scratch directory,
indexed there, and summarized into `{ coverage, coverage_is_lower_bound, assurance_counts,
weakest }`. Points are cached by sha in `.tracelean/history.json` under a scheme tag, so a
commit is computed exactly once and a change to how a point is computed invalidates the
file instead of mixing old and new numbers on one chart. The panel draws a plain SVG line
behind a `trend` button (opt-in, because indexing 30 commits is not free): points whose
coverage is only a lower bound are drawn hollow, and commits that failed to index are
reported as a count rather than interpolated across.

Commits, not undo-tree nodes, are the unit: the undo tree is one developer's history,
while a requirement's coverage is a property of the shared project.

### T5 — LSP client and Myth: DONE (live diagnostics deferred)

**The client** (`core/src/lsp/`) is hand-rolled against the protocol rather than built on
`async-lsp`; the reasoning is recorded at the end of `docs/plan_t5_lsp_myth.md`. It
covers `transport.rs` (Content-Length framing, id correlation, in-order notifications,
answering server-initiated `workspace/configuration` and `client/registerCapability`),
`registry.rs` (language → server, with verus-analyzer preferred over rust-analyzer and
`lake serve` when a lakefile is present; a missing server is a first-class reported state
with an install hint), and `edits.rs`.

**`WorkspaceEdit` → `Command` (`core/src/lsp/edits.rs`)** is the load-bearing piece. An
LSP rename or quick fix is lowered into the same invertible commands a hand edit produces
and applied through the undo tree, which is what keeps "one history of everything" true
and what lets T1's anchors follow a rename. It refuses two things loudly rather than
quietly: an edit touching a document it cannot read fails whole (a half-applied rename is
worse than a refused one), and positions are decoded in a declared encoding — UTF-16 by
protocol default — because guessing corrupts exactly the files with non-ASCII content.
Edits within a document are emitted last-to-first so sequential `Replace` commands land
where the server meant them.

**Dynamic action providers (`core/src/myth/provider.rs`)** are the change everything else
hangs off. `bindings.json`'s capture map is now one `ActionProvider` beside an LSP
provider (code actions at the cursor, grouped by LSP's own `kind`) and a trace provider
(navigation and evidence on an annotated line; `annotate` on an unannotated one — offering
"go to requirement" where there is no requirement is the kind of dead entry that makes a
feature feel decorative). `Action` carries `title`, `group` and `priority`, so which-key
shows "Who wrote this line" rather than `goto_provenance`. Providers only *offer*;
everything still executes through `ActionRegistry::dispatch`, so the rule that mutating
actions return `Command`s survives the menu becoming dynamic.

**The mode stack (`ModeStack` in `keymap.rs`).** Modes nest and `Escape` pops one level.
This fixes a documented-versus-actual mismatch: the README promised `Escape` in `File`
went "back to Options" while every mode spelled it `→Main`. `pop` and `reset` are now
keymap verbs, `→Main` is kept as a spelling of `reset` (Main is the floor, never a level),
and an unbound key still cancels the whole stack.

**New modes** `Goto`, `Verify`, `Trace`, `CodeActions`, exactly as design §12 lists them.
`Trace` is the uniquely-TraceLean surface: `t u` runs `goto_provenance`, which answers
"who wrote this line" from the undo tree rather than from git
(`core/src/provenance.rs` — walking one `Replace` backwards maps a post-edit position to
its pre-edit one exactly, so no replay and no heuristics; text that arrived with the file
reports as the origin rather than being attributed to someone).

**The README keymap table is generated** from `keymap.json` by `Keymap::markdown_table`,
spliced between markers, and a test fails the build when it drifts
(`TRACELEAN_UPDATE_README=1 cargo test -p tracelean-core readme_keymap` regenerates it).

A real bug fixed on the way: `mythKeyName` lowercased every single-character key, so every
uppercase binding was unreachable — Trace mode's `M` (coverage map) arrived as `m` ("go to
tests").

**Deferred deliberately:** live diagnostics on every keystroke. They need incremental
re-parse in the surface model, which both the design (§12.4) and the plan (§6.8) put last
and explicitly forbid blocking the rest on. Everything else in the plan's order of work is
done.

### Frontend surface added

- `myth_actions_at` (providers at the cursor), `myth_node_at`, `lsp_query` (editor-side,
  reusing the long-lived registry and the *buffer* rather than the file on disk),
  `lsp_status`, `trace_history`.
- `Editor.tsx`: effect router for the new actions (`lsp_query`, `reveal`, `no_target`,
  `goto_node`, `step_diagnostic`, trace effects, `annotate`, `lake_build`), an LSP answer
  popup, a provider-driven context menu grouped by core's ordering, the trace gutter, and
  the dynamic `CodeActions` mode whose keys are resolved from the provider list.
- `WhichKeyBar.tsx`: mode breadcrumb (`Options › Trace`), titles instead of raw action
  names, provider groups kept together, and a note when a menu had to truncate.

### The example project: rebuilt

`example/` used to be the old world — `reqs/REQ-01.md`, `specs/REQ-01.lean`,
`tests/unit/test_auth.rs` — which is to say it demonstrated the filename conventions this
work deleted. It has been replaced by a checkout-pricing project built on the current
design. (The old tree was copied to this session's scratchpad before deletion; it was not
worth keeping in the repository.)

The layout is deliberately unconventional — `product/ formal/ engine/ service/ checks/
harness/` — because the strongest way to show that directory names no longer carry meaning
is a project that does not use the expected ones. It exercises all five roles, both
qualifiers, and two languages on the implementation side.

Four gaps are built in on purpose, each teaching something different:

| Gap | What it teaches |
|---|---|
| `REQ-DISCOUNT.coupon` has no implementation and no annotation | an annotation is a claim; writing one for absent code is the one thing this must never make easy |
| `REQ-RECEIPT` is `decomposition: open` | every percentage over it renders as `≥`, and it can never read as finished |
| three clauses are `Unbound` (model + implementation, no harness) | a model nobody runs is decoration, and without this finding it looks identical to one that passed |
| `REQ-RECEIPT.lines` has no model | not everything is worth formalizing — the point is that the tool says so |

It reports **`≥ 44%` coverage at `L1`**: everything annotated, nothing yet checked. That is
the truthful starting state of a project that has just adopted this, and a green demo would
have taught the opposite of the whole design.

`harness/drt_adapter.py` runs today with plain `python3`; `checks/` passes with
`python3 -m unittest discover -s checks -p '*_checks.py'`. A full differential run also
needs a Lean toolchain, which this machine does not have — it reports "no model runner"
rather than quietly passing.

Building it found two real defects in the code, both now fixed with tests:

- **`CoverageFloor::MinOutputVariants` could not be serialized at all.** As a tuple variant
  under `#[serde(tag = "kind")]` serde has no representation for it, so any project
  choosing that floor would have failed when saving `.tracelean/drt.json` — the one place
  the type has to survive a round trip. It is a struct variant now, and a test round-trips
  every floor.
- **`PolicyUnmet` emitted byte-identical duplicate findings.** A rule matching a
  three-clause requirement produced three findings whose text named only the glob, so the
  checker read as though it were stuttering. The message now names the clause.

### Three long-standing failures, fixed

The suite had been carrying three failures described as "pre-existing, not caused by this
work". They were real defects, and all three are now fixed.

**Two `model_catalog` failures — Bedrock Claude models sorted as unranked.**
`data/models.json` is regenerated from upstream catalogs that only rank the *bare* model id
(`anthropic.claude-sonnet-4-6`), so every cross-region row (`us.`, `eu.`, `global.`, …)
arrived with a null `coding_index`. Those prefixed ids are exactly the ones the Bedrock
picker offers — bare Claude ids are not valid Converse identifiers and fail with "use an
inference profile" — so in practice *every* Bedrock-picked Claude model appeared unranked.

The previous fix had been to hand-edit the prefixed rows in the data file, and the test
comment recorded that choice explicitly. It did not survive the next catalog regeneration,
which is the argument for deriving the value at load rather than storing it: a derivation
cannot go stale. `rankings_with_region_fallback` now resolves each entry's ranking once,
falling back to the bare sibling for any id carrying a known inference-profile prefix.

Only the ranking is inherited. Pricing is deliberately left alone, and a new test pins that:
Bedrock charges a cross-region uplift, so a `us.` row carries genuinely different numbers
and copying the bare row's price would understate every bill by about 10% — silently, and
in the direction that costs money.

**The flaky sandbox test was a real race, not noise.** `sandbox::session`'s tests set
`$HOME` and `$CLAUDE_CONFIG_DIR` to scratch directories and restore them afterwards.
`bwrap_argv` reads those at call time — correctly, since the sandbox must bind whatever
home the user actually has — but environment variables are process-global and `cargo test`
runs these in parallel threads of one process. One test's temporary `HOME` was therefore
being baked into another test's `bwrap` arguments and then deleted before `bwrap` ran, so
the failure was `bwrap: Can't find source path /tmp/.tmpXXXX` and it never reproduced when
the test was run alone.

The fix is a mutex taken by every sandbox test that *reads* the environment, not only by
the two that write it — a reader that does not participate is precisely the race. It
ignores poisoning, so one genuine failure does not cascade into thirty misleading ones.
Verified with 20 consecutive runs of the sandbox tests and 6 of the whole library, plus
runs under concurrent load, with zero failures.

### Current test status

`cargo test --workspace`: **313 tests in the tests crate + 300 in core + 1 in the GUI
backend pass. Zero failures, zero known flakes.**

Stability was checked deliberately rather than assumed: the core library suite was run six
times in a row and the sandbox tests twenty times, including while two copies of the tests
crate ran concurrently, with no failure in any run.
