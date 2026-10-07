# ROLLOVER — Traceability (requirements ↔ Lean model ↔ code) + LSP/Myth

## Goal
Replace TraceLean's filename-convention traceability with annotation-based traceability,
bound to code by an executable Lean reference model + differential testing (the AWS Cedar
pattern), language-independent. Plus an LSP client and a Myth keyboard layer over it.

## Status
**T1–T6 done.** `cargo test --workspace` → **661 pass**, 5 ignored (they write into
`example/` or need a Lean toolchain), zero failures, zero warnings. `npm test` → 21.
`npx tsc --noEmit` clean.

T6 is the Lean infoview, the coverage-map fix, the findings filter, the judge-prompt
export, the differential-testing harness end to end, the project graph, and the removal of
the trace graph. Read `working_feature_t6_implementation.md` for the per-topic writeup.

**The differential loop now runs for real.** Lean 4.12.0 is installed via elan on this
machine, `example/` compiles, and all six of its bindings run 2000 random cases each
against the compiled model with zero divergences. That exercise found six defects that had
made the feature impossible rather than merely awkward — see the T6 writeup; the worst of
them is in Gotchas below because it destroys files.

Read `working_feature_traceability_implementation.md` for the full per-topic writeup — it
is the document written for the user to read; this file is the operational handoff.

## What exists now (verified)

### T1 — traceability core, `tracelean/core/src/trace/`
`annotation.rs` (tree-sitter comment scan), `anchor.rs` (own recursive walk — `SymbolTable`
only sees top-level items), `hash.rs` (body/link/requirement/clause hashes, scheme `v1`),
`requirement.rs` (hand-rolled frontmatter parser, no YAML dep), `evidence.rs`
(`Bond`/`Level`/`EvidenceRecord`/`Assurance::weakest`), `checker.rs` (20 finding kinds +
policy gate), `lockfile.rs` (`.tracelean/trace.lock.json`), `rollup.rs`, `map.rs`,
`history.rs`. Entry point: `trace::build(root) -> TraceIndex`.

### T2 — differential testing, `tracelean/core/src/drt/`
`protocol.rs` (long-lived runner, per-case timeout, stderr capture), `schema.rs`, `gen.rs`
(edge-biased generation + shrinking), `run.rs` (`DrtResult::earns_l3` needs no divergence
*and* a coverage floor), `config.rs` (`.tracelean/drt.json` + adapter scaffolds),
`lean_runner.rs` (toolchain detection, generated runner, `lake build`).

### T3 — judge, `tracelean/core/src/judge/`
`prompt.rs` (`judge/v1`, four worked examples), `verdict.rs` (a drift verdict without a
witness is rejected at parse time), `witness.rs` (the witness is *executed* against the
compiled model; falsified ⇒ verdict discarded), `calibration.rs` (acceptance = zero false
`agrees` on mutated pairs).

### T4 — hierarchy/zoom/UI
`rollup.rs` + `TracePanel.tsx` (tree, assurance chain, `≥` coverage bars, treemap with grey
untraced files, 5 zoom levels), gutter chips in `Editor.tsx` (`traceGutter`), commit trend
from `trace/history.rs` behind the panel's `trend` button.

### T5 — LSP + Myth
`tracelean/core/src/lsp/`: `transport.rs` (hand-rolled Content-Length JSON-RPC — reasoning
at the end of `docs/plan_t5_lsp_myth.md`), `registry.rs`, `edits.rs`
(`WorkspaceEdit` → invertible `Command`s).
`tracelean/core/src/myth/`: `provider.rs` (`ActionProvider` + static/LSP/trace impls,
`Action{title,group,priority}`), `keymap.rs` (`ModeStack`, `pop`/`reset` verbs,
`markdown_table`), `actions.rs` (Goto/Verify/Trace/CodeActions actions).
`tracelean/core/src/provenance.rs`: "who wrote this line" from the undo tree.

### T6 — infoview, harness, project graph
`react_frontend/components/LeanInfoview.tsx` (proof state from `$/lean/plainGoal`),
`treemap.ts` (squarified layout, shared by the coverage map and the project graph),
`core/src/drt/{infer,bind}.rs` (schema inference and one-action binding),
`core/src/trace/graph.rs` + `react_frontend/components/ProjectGraph.tsx` (the project
graph), `service::{drt_build_runner, trace_project_graph}`.

## What is deliberately NOT done
**Live diagnostics on every keystroke.** They need incremental tree-sitter re-parse in the
surface model. Both `docs/formal_traceability_and_lsp_plan.md` §12.4 and
`docs/plan_t5_lsp_myth.md` §6–7 put this last and forbid blocking anything else on it. It
overlaps with "Bug -1 layer 3" in `docs/TODO_lsp_integrations_with_myth.md`; doing that
first de-risks it. Everything else in every plan's order-of-work is complete.

## Next, if picking this up
1. **A coverage floor that can fail.** `all-input-constructors` is vacuous for a scalar or
   single-struct input: it reports one constructor named `value`, seen, floor met. It says
   nothing about whether the run reached the model's branches. What makes the example's runs
   diverse today is `infer::boundaries` seeding the generator with the model's own numeric
   literals; the floor is not yet the thing that checks it. A branch-coverage floor would
   need the model runner to report which arm it took.
2. **Incremental re-parse**, then live diagnostics through the existing surface pipeline
   (a diagnostic is a node with an `error`/`warning` capture — no parallel UI).
3. **Judge calibration against a live provider.** `judge/calibration.rs` ships 7 fixtures
   and the acceptance rule; it has never been run against a real model. The judge-prompt
   export (`service::judge_prompt`) makes this cheap to do by hand first.
4. **Reference edges from the LSP.** The project graph's reference edges are textual name
   matches, labelled `Confidence::Textual`. A call-hierarchy request would upgrade them
   where a server is running; the graph must keep working with no toolchain, so the textual
   path stays as the fallback.

## The example project (`example/`)

Rebuilt from scratch; the old `reqs/`/`specs/` tree is gone (a copy of it is in this
session's scratchpad, not in the repo). Domain: checkout pricing, integer cents, pure.
Layout is deliberately unconventional — `product/ formal/ engine/ service/ checks/
harness/` — to demonstrate that no directory name carries meaning any more.

It exercises all five roles, both qualifiers, and gaps that each teach something
different: an unimplemented clause (`REQ-DISCOUNT.coupon`), an `open` decomposition
(`REQ-RECEIPT`, so percentages render as `≥`), an unmodeled clause, and a requirement that
sits at `L1` although two of its three clauses are `L3`, because assurance takes the
minimum. Today it reports `REQ-CHECKOUT` 100%/`L3`, `REQ-SHIPPING` 83%/`L3`,
`REQ-DISCOUNT` 67%/`L1`, `REQ-RECEIPT` `≥ 50%`/`L1`.

**The whole loop runs.** Six bindings in `.tracelean/drt.json`, six generated adapters in
`harness/`, one Lean runner built into `.tracelean/drt/`, 2000 cases each, zero
divergences:

```sh
cargo test -p tracelean-tests build_example_runner -- --ignored --nocapture
cargo test -p tracelean-tests run_example_drt     -- --ignored --nocapture
```

That the harness *can* fail is checked by hand rather than assumed: change `>=` to `>` at
the 20,000-cent threshold in `engine/pricing.py` and `REQ-DISCOUNT.tiers` reports 32
divergences in 2000 cases. Without `infer::boundaries` seeding the generator with the
model's own literals, the same mutation survives untouched.

`checks/` passes with no dependencies:
`python3 -m unittest discover -s checks -p '*_checks.py'`.

## Gotchas (all still live)
- **`lake build` DELETES the required package's source directory when a `require` names it
  differently from the way that package names itself.** Lake 4.12 prints
  `package '«checkout-model»' was required as 'formal'` and removes `formal/` on the way
  to that message. It destroyed `example/formal/` once — the model, the proofs, the
  lakefile — recovered only from the session transcript. Two guards now exist:
  `lean_runner::lake_package_name` reads the declared name out of the lakefile instead of
  guessing it from the directory, and `lean_runner::check_requires` refuses to invoke
  `lake build` at all when a `require` disagrees with the package it points at. Do not
  weaken either, and do not point a generated `require` at a directory you would mind
  losing.
- **A Docker named volume inherits the ownership of whatever sits at its mount
  point in the image.** An absent or root-owned directory there becomes a
  root-owned volume, and a container running as a non-root user cannot write to
  it — which is how `cargo` came to fail with "Permission denied" on both the
  registry and `target/`. Create and `chown` such directories in the Dockerfile;
  a `chmod` on the mount point is shadowed by the volume.
- **Bubblewrap needs both `seccomp:unconfined` and `apparmor:unconfined` under
  Docker.** With seccomp alone it gets as far as `Failed to make / slave:
  Permission denied`. `docker build` can grant neither, so `make ci` cannot run
  the sandbox tests at all; `make sandbox-test` is where they run.
- **`example/` is not in git.** It was rebuilt in this session and never committed, which
  is why the deletion above was nearly unrecoverable. Commit it.
- **Do not reintroduce filename conventions**, not even as a fallback or a "cheap first
  step". The user rejected this explicitly, tests included.
- The Lean model must stay *independent* of the implementation — written from the
  requirement, not derived from the code, or differential testing degenerates into testing
  a transliteration of the code against itself.
- The Lean model must be a **shallow** embedding (plain compiled Lean functions), first-order
  and total. This is why Strata/Laurel was rejected as the model layer.
- A DRT divergence is NOT automatically an implementation bug — model and adapter are
  equally suspect. Triage is a recorded decision, never automatic.
- The judge's claims about Lean must be **executed**, not trusted.
- Case counts without model-branch coverage are theatre; report both.
- Assurance aggregates by **minimum**, never mean, and renders as a chain — `L4` is a
  property of the *model*, not of the code.
- LSP positions are **UTF-16** code units unless negotiated otherwise. `edits.rs` takes the
  encoding as a parameter for this reason; do not default it silently anywhere else.
- `Editor.tsx` gutter chips clear on `docChanged` on purpose: the index is rebuilt from
  disk, so line numbers go stale the moment you type.
- The README keymap table is **generated**. After editing `ui_settings/keymap.json` run
  `TRACELEAN_UPDATE_README=1 cargo test -p tracelean-core readme_keymap`, or the build fails.
- **Annotations only count inside comments.** A Python docstring is a string literal, so
  annotations in one are invisible. Conversely, writing `@implements` in prose *does*
  create an annotation and the checker reports it — refer to roles by name in prose.
- `myth/actions.rs` actions are synchronous and get only `&AppState`; anything needing the
  LSP registry or the network emits a `Ui` effect that the frontend routes. Do not add I/O
  to an `ActionFn`.
- **Tests that set `$HOME` or `$CLAUDE_CONFIG_DIR` must take `sandbox::session`'s
  `env_guard()`.** Environment variables are process-global and `cargo test` runs threads in
  one process, so a test that repoints `HOME` rewrites the world for every other test
  calling `bwrap_argv` at that moment. Readers take the lock too — a reader that opts out is
  the race.
- **Do not hand-edit rankings into `data/models.json`.** It is regenerated from upstream and
  the edit will be lost; that is exactly how the Bedrock picker's "everything is unranked"
  bug came back. `rankings_with_region_fallback` derives them at load instead. Pricing must
  *not* be derived that way — cross-region rows are genuinely more expensive.
- `npx vite build` fails with `EACCES ... rmdir dist/assets` — a pre-existing permission
  problem on this machine, unrelated. The type gate is `npx tsc --noEmit`, which passes.

## Key paths
- `working_feature_traceability_implementation.md` — the user-facing writeup.
- `docs/formal_traceability_and_lsp_plan.md` — the design (decisions only).
- `docs/verification_backends_considered.md` — why other backends were not chosen.
- `docs/plan_t1..t5_*.md` — per-topic plans, each with its review-revision section.
- `tracelean/core/src/{trace,drt,judge,lsp,myth,provenance.rs}` — the implementation.
- `tracelean/tests/unit/test_{trace,drt,judge,lsp,myth}.rs` — most of the 661 tests.
- `tracelean/Dockerfile{,.dev,.ci}`, `docker-compose.yml`, `Makefile`, `rust-toolchain.toml`
  — the reproducible toolchain: `make test`, `make e2e`, `make ci`.
- `tracelean/tests/fixtures/trace_project/` — the annotated fixture project.

---

# Step: application-independent differential harness (this session)

## Goal
Remove the per-binding harness files. A project should bind a Lean model to an
implementation by *describing* the call, not by writing a protocol implementation
for every clause.

## Why
Six files under `example/harness/` differed in about six lines each and shared
~52 lines of identical boilerplate. The user named it as a design defect, and it
is one. Cedar (`cedar-policy/cedar-spec`) has one harness for the whole system:
`cedar-lean-ffi` marshals, `cedar-policy-generators` generates, and each of the
30-plus fuzz targets is a short declaration. Mechanism once, specifics per entry
point.

## Done
- **New** `tracelean/core/src/drt/runners/python_runner.py` — the shipped Python
  runner. Embedded with `include_str!`, written to
  `<project>/.tracelean/drt/tracelean_drt_runner.py` at run time. Imports the
  implementation *by path* (no package assumptions), maps model field names onto
  parameter names, unwraps dataclasses / named tuples / plain objects into the
  shape `deriving ToJson` produces, turns a raise into an `error` reply, flushes
  after every line, and serves every op from one process.
- **`drt/config.rs`** — `Binding.implementation` is now
  `Implementation` = `Process(RunnerSpec)` | `Call(CallSpec)`, untagged, with
  `Process` first so an object carrying `cmd` still parses as an adapter.
  `CallSpec { language, entry: "path.py::symbol", params, convert }`.
  `Implementation::spec(root, all_bindings)` materializes the runner and writes
  `.tracelean/drt/bindings.<lang>.json`. `Binding::resolved()` turns a named
  function into a command.
- **`drt/run.rs`** — refuses an unresolved binding by name rather than guessing.
- **`drt/bind.rs`** — `propose()` emits a `Call` for Python and writes no file.
  A keyword mismatch is now *resolved into* `params` by pairing in declaration
  order, and reported as inferred. `adapter_path`/`adapter_source` are `Option`.
  `python_adapter()` and `python_module()` deleted.
- **`trace/mod.rs` + `trace/checker.rs`** — `TraceIndex.drt_bindings` holds the
  `(req, clause)` pairs from `drt.json`; the `Unbound` finding now accepts a
  binding as the bond. Without this, deleting the adapters would have made every
  clause report "no `@drt` harness".
- **`example/`** — `harness/` deleted entirely; the six bindings rewritten
  declaratively.
- **LSP**: `lsp/registry.rs` gained `resolve_program()` — PATH, then
  `~/.elan/bin`, `~/.cargo/bin`, `~/.local/bin`, `/usr/local/bin`,
  `/opt/homebrew/bin`. This was the "Lean language server is not running" bug:
  Lean *was* installed, but elan's PATH line lives in `~/.profile`, which only a
  login shell reads.
- **UI**: traceability zoom buttons named (Capabilities / Requirements /
  Evidence — three, because levels 4 and 5 rendered what 3 rendered); assurance
  levels drawn as `L3 tested` rather than `L3`, with tooltips.
- **Docs**: `docs/differential_harness_explained.md`,
  `docs/differential_harness_walkthrough.md` (new, one case end to end with real
  file contents), `example/README.md`, `docs/architecture.md`,
  `tracelean/README_DOCKER.md`, `tracelean/Dockerfile.dev`.

## Verified
- `cargo test --workspace`: 669 pass, 6 ignored, 0 fail, 0 warnings.
- `npx tsc --noEmit` clean; `npm test` 21 pass.
- Real end-to-end with **no harness files in the project**: six bindings, 2000
  cases each, 0 divergences.
- Mutation check: injecting the pre-discount-subtotal bug into
  `example/engine/pricing.py` produces 61 divergences in 2000 cases, shrunk in
  one step to `{"remote": false, "subtotalCents": 10001}`. Restored afterwards.

## Follow-up in the same step: the escape hatch is gone
`Binding.implementation` is now `CallSpec` outright — there is no `Process`
variant and no adapter template in any language. The reason is not duplication:
an adapter is an untrusted participant in the comparison it exists to enable, and
arbitrary code between the implementation and the comparator can make a
divergence vanish. `drt::run` now takes the implementation's `RunnerSpec` as a
parameter (`run(binding, implementation_spec, options)`), so "unresolved binding"
is no longer a representable state. `scaffold_adapter`, `rust_adapter`,
`python_adapter`, `BindProposal::adapter_*` all deleted. `propose()` on a
non-Python implementation returns a clear error instead of a template that had
never been run end to end. Re-verified: 668 tests, six bindings, 0 divergences.

## Follow-up: spec strength (new feature, this session)
`tracelean/core/src/trace/strength.rs`. TraceLean generates the obligation
`∀ f g, Spec f → Spec g → f = g` for each `@models` declaration, where `Spec` is
the conjunction of its `@proves` theorems abstracted over the model symbol. The
proof is the human's part; the generated file arrives with `sorry`.
- New role `@pins`, new qualifier `@nondeterministic reason="..."`.
- Grouped by declaration, not clause (`shippingCents` serves three clauses).
- The generated file re-proves that the model satisfies its own abstracted spec,
  which is a guard on the textual abstraction.
- `strength::check` runs `lake env lean` and maps `sorry` warnings to declaration
  spans. States: `pinned | attempted | open | nondeterministic`.
- Measured on the example: shippingCents was weak (nothing constrained it below
  10,000), two theorems were added, and `shippingCents_pinned` now proves.
  `discountCents` stays `attempted` because its one property is satisfied by
  `fun _ => 0`.
- Gotcha hit and fixed: the generated docstring contained the literal
  `@nondeterministic ...` as an example, and the scanner read it as a real
  annotation — the exact trap `example/README.md` warns about.
- Design + measurements: `docs/spec_strength.md`.

## Follow-up: strength is live, and coverage tooling is in the image
- `ClauseView.strength` carries the declared state, computed on every scan for
  free. The panel renders it beside the assurance chain as `spec pinned |
  unfinished | unasked | not determined`, and a `spec` button asks the kernel
  (`trace_strength_check` → `lake env lean`) and marks verified answers with a
  tick. An unverified `@pins` claim is never shown as pinned.
- New finding `UnpinnedProof` (info): a clause is proved and nobody has asked
  whether the proof determines the model.
- `trace_strength_scaffold` writes the obligation file and refuses to overwrite
  one, because the proofs in it are the point.
- Coverage tooling added to the dev image: `cargo-llvm-cov` 0.6.16 (pinned,
  `--root /usr/local` so it survives the CARGO_HOME move) and Debian's
  `python3-coverage`; `llvm-tools` added to `rust-toolchain.toml`; `make cover`
  and a `cover` compose service. `README_DOCKER.md` now separates the three
  things called coverage: requirement coverage, DRT input coverage, line
  coverage.
- The dev compose services now bind-mount `../example:/example` and
  `../README.md:/README.md`. Without them the e2e service's tests resolved
  `/example`, found nothing, and skipped — green on checks that never ran.

## Follow-up: the Project view is a node-link graph
The treemap is gone from that panel. `trace/graph.rs` gained `role_graph(index,
findings) -> RoleGraph` — `RoleNode { id, column, label, sublabel, roles,
assurance, strength, stale, exempt, partial, findings }`, `RoleEdge { from, to,
role, stale }`, plus `unlinked_clauses` for clauses nothing annotates (counted,
not drawn as floating orphans). `Column` is `requirement | model |
implementation | evidence`, and a declaration carrying several roles is drawn
once, in the leftmost column it claims.
- Frontend: `react_frontend/components/roleLayout.ts` (pure layout, 5 vitest
  cases) and a rewritten `ProjectGraph.tsx` (`orderNodes`, 4 vitest cases).
  Ordering ranks each declaration by the requirement it serves, so edges are
  short hops rather than long diagonals.
- Edges are coloured by role and dashed when stale. Selecting a requirement
  lights the whole subgraph it reaches, breadth-first.
- `trace_role_graph` service fn + IPC command; the treemap code in `treemap.ts`
  stays because the coverage map in the traceability panel still uses it.
- Pre-existing, unrelated: `tracelean/dist/` is root-owned from an old container
  image, so `npm run build` fails at `rmdir`. `sudo rm -rf tracelean/dist` fixes
  it; documented in README_DOCKER's troubleshooting table.

## Follow-up: node properties on the graph
Each node now carries what is actually a property of it.
- **Code coverage on code nodes.** New `trace/coverage.rs`: a normalized
  `.tracelean/coverage.json` plus importers for `cargo llvm-cov --json` and
  `coverage json`. Lines that are not executable are in neither set, so a
  twenty-line function with three statements is 3/3 rather than 3/20. A file
  nobody measured shows no badge — never 0%.
- **Tests on implementation nodes.** New `trace/test_results.rs`: normalized
  `.tracelean/test_results.json`, importers for libtest JSON and the normalized
  form, plus `tracelean/scripts/unittest_results.py` for Python. Matched to
  annotations by the declaration's last name segment; ambiguity resolves to the
  worse outcome. `passing` stays `None` until a runner actually said so.
- **Pinned state on model nodes.** `strength` is set on the model node, drawn as
  a colour-coded dot plus the word.
- **A harness node.** Synthesized per bound clause from `.tracelean/drt.json`
  and the `ModelImpl` evidence record: `unbound | never run | stale | N diverged
  | agreeing`, coloured by state rather than by level. Previously the harness had
  no node at all and its state was invisible.
- **Colour clash fixed.** `implements` edge green was the *same hex* as L3 and
  `proves` blue the same as L4. Edges are now one neutral colour, dashed only
  when stale; the whole colour budget goes to assurance. A vitest guards it.
- `make cover` now also emits `llvm-cov.json`, `coverage-py.json` and
  `test-results.json`; `trace_import_coverage` / `trace_import_test_results`
  read them.
- Measured on the example: `discount_cents` 6/6 lines and 2/2 passing,
  `shipping_cents` 3/3 and 3/3, six harness nodes at 2000 cases / 0 divergences.

## Follow-up: a failing example, status colours, two UI fixes
- **The example now contains a real divergence.** New clause `REQ-DISCOUNT.cap`
  ("no order is discounted by more than 5000 cents"): `Checkout.cappedDiscountCents`
  applies `min ... 5000`, `pricing.capped_discount_cents` defines
  `DISCOUNT_CAP_CENTS` and never uses it. Seven bindings now; six agree, one
  reports ~1365 divergences in 2000 cases, shrunk to subtotal 33,351 (model 5000,
  implementation 5002). `run_example_drt` asserts that clause diverges and that
  the run does not earn L3 — an example where everything agrees cannot make the
  argument for having a model.
- **Status colours are one palette with per-node meaning.** `STATUS = {ok,
  progress, missing, declared}`; green/amber/red mean working / under way /
  missing-or-failing on every node, and *what* they are about changes: on a model
  the dot is spec strength (pinned / unproven / unasked / random), on an
  implementation the test outcome (untested is red), on a harness the DRT state.
  `open` strength is red on purpose — the check does not exist.
- **A model node always shows its dot**, including `unasked`. No badge read as
  "does not apply" when the truth was "nobody answered".
- **Weakest-link strength**: a model serving several clauses takes the weakest
  answer (`weaker_strength` in graph.rs), so the badge no longer depends on map
  iteration order.
- **Generated-annotation leak fixed**: the strength scaffold's own docstrings
  contained the literal proof-role marker, so the checker reported three
  "annotation without a requirement id" warnings against the generated file. The
  same trap `example/README.md` warns about, hit twice now — generated text must
  name roles in prose.
- **UI**: the which-key bar at the bottom is disabled behind
  `SHOW_WHICH_KEY_BAR` in `App.tsx` (component and keymap plumbing kept). The
  project-view detail pane scrolls now — it needed `min-height: 0`, since a flex
  item will not shrink below its content and `overflow-y: auto` then has nothing
  to scroll.
- **`scripts/unittest_results.py` has a test** that runs it over a suite with a
  passing, a failing and a skipped case. A silently broken importer would turn
  "3 of 3 passing" into "3 tests" with no error anywhere.

## Next
1. ~~Project view → node-link graph~~ done (requirements → models → code → evidence;
   solid edges = annotation, dashed = textual reference). The user chose this
   over the treemap; `react_frontend/components/ProjectGraph.tsx` currently draws
   a squarified treemap.
2. `tracelean-trace` CLI binary calling the same `core::service` functions the
   Tauri IPC layer calls, emitting JSON, so external LLM agents share the tool's
   baseline instead of re-deriving it.
3. Rust implementations need a *generated* dispatching runner — one bin per
   project, built like the Lean side. Until then `propose()` refuses them.
4. Spec strength as an L4 qualifier (`docs/spec_strength.md`): mutate the model,
   rebuild the `@proves` file, report the mutants whose survival shows the
   property set does not pin the model. L3 has a coverage floor; L4 has nothing.

## Gotchas
- `Implementation` is `#[serde(untagged)]`. It disambiguates on required fields
  -- `cmd` for `Process`, `language` + `entry` for `Call` -- so neither variant
  can swallow the other. Adding a variant with only optional fields would break
  that, because untagged matching would then accept anything.
- `example/` is still untracked in git.
