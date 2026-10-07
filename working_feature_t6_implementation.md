# Working state — T6 (infoview, project graph, requirement UX)

Plan: `docs/plan_t6_infoview_project_graph_and_requirement_ux.md`.
Read that for the *why*; this file is the state of the work.

## Status

| # | Topic | State |
|---|---|---|
| 6a | Coverage map (backend + real treemap) | **done** |
| 4 | Findings filter | **done** |
| 5 | Judge prompt export / paste-back | **done** |
| 1 | Lean infoview | **done** |
| 3 | Harness binding flow + example harnesses | **done**, running end to end |
| 6b | Project graph | **done** |
| 2 | Delete the trace graph | **done** |
| — | Toolchain + containers | **done** |

`cargo test --workspace` → 661 pass, 5 ignored, zero failures, zero warnings.
`npm test` → 21. `npx tsc --noEmit` clean.

## Toolchain installed on this machine

`elan` + `leanprover/lean4:v4.12.0` (`~/.elan/bin`). Verified: `example/formal`
builds with `lake build`, **including the proofs** — `CheckoutProofs.lean` did not
compile as written and was fixed against the real compiler (`Nat.div_le_div_right`
does not exist in core Lean 4.12; the bound now goes through `omega` after
establishing `subtotal * p <= subtotal * 100`).

`rust-analyzer` and `pyright-langserver` were already present. `vitest` was added
as a dev dependency — the frontend had no test runner at all, and this work adds
non-trivial frontend logic (treemap layout, and the project graph to come).

## Done so far

### 6a — the coverage map was broken twice over, verified

**Backend (`core/src/trace/map.rs`).** Entries were emitted per *link*, so a
function carrying three `@implements` produced three rectangles over the same
nine lines and counted those nine lines three times; a `.min(total)` clamp then
hid the overcount in small files and left it in large ones. Rewritten to attribute
every source line to **at most one** anchor, innermost wins, so rectangles tile
the file exactly and coverage counts each line once by construction. Four new
tests pin it, including "rectangles sum to the file".

**Frontend.** `CoverageTreemap` was not a treemap: each rectangle's width came
from `sqrt(share * 1.6)` independently of every other, so no area was conserved;
`y` ran past the canvas and a `Math.min(h, height - y)` clamp then drove later
rectangles to zero height so they silently vanished; `slice(0, 400)` truncated
without saying so. Replaced by a real squarified treemap
(`react_frontend/components/treemap.ts`, Bruls–Huizing–van Wijk) laid out over the
**directory hierarchy** — which also removes the truncation, because a directory
is one rectangle until it is large enough to open. 14 vitest cases pin the
invariants: areas sum to the container, rectangles are disjoint, nothing escapes
the container, order is preserved.

### 4 — findings filter

`all · errors · warnings · unbound · stale`, with counts that double as the
project summary, plus a flat findings list sorted by severity for when you are
fixing rather than surveying. `errors` means **blocking**, not "severity is
error": `Finding` gained a `blocking` flag set from the policy's `block_on` list,
so the panel and the CI gate cannot disagree about what an error is. Filtering
keeps ancestors so a filtered tree is still a tree, and an empty result names the
active filter instead of rendering as "no requirements".

### 5 — judge prompt export

`prompt::render_standalone` concatenates the system prompt and the task into one
pasteable block, inlining the clause, the Lean source, the input schema and the
reply schema so it works in a session with no repository access.
`service::judge_prompt` returns it with no provider call;
`service::judge_apply_reply` takes the reply back.

The load-bearing property: **the pasted reply goes through the identical
pipeline** — `verdict::parse` (so a drift verdict with no witness is rejected
before it can mean anything) and `witness::check` (so a claim about Lean that
does not hold is discarded). Changing the transport does not change the standard
of evidence. `EvidenceDetail::Judge` gained `source: "api" | "pasted"`, because
who produced a verdict is part of what the record is for.

The panel's primary button is now **copy judge prompt**; "judge via API" is
demoted to secondary. If no compiled model runner exists, the copy action says so
up front rather than letting the user discover it after doing the work.

### A fourth "flaky" test that was a real bug

`drt::runner_death_carries_the_stderr_text` failed only under load. Cause:
`Runner::death_note` read the captured stderr the instant stdout closed, but the
stderr reader thread had not necessarily been scheduled — so on a busy machine a
dying runner reported its death **with no reason attached**, exactly when the
reason matters most. `Runner` now waits (bounded at 500 ms, so a child holding
the pipe open cannot hang a run) for the reader to reach EOF before deciding
there was nothing to say. The regression test runs the scenario twelve times.

### 3 — differential-testing harness, integrated (done, end to end)

The example project now runs real differential tests: six bindings, each a
compiled Lean model against the Python implementation, 2000 random cases per
binding, zero divergences. `run_example_drt` reproduces it.

Getting there turned up six defects, five of which made the feature impossible
rather than merely awkward.

**Lean anchors resolved to the keyword, not the declaration.**
`tree-sitter-lean4` parses `def f ...` as a named `definition` node whose first
child is an anonymous `def` token, and gives the keyword inside
`structure C where` the kind `"structure"` — the same string as the declaration
node around it. `DECL_KINDS` listed the keywords, so every Lean anchor in every
project resolved to the symbol path `"def"` with a one-line span. That broke the
body hash (a model could be rewritten without ever going stale), the gutter
chips, the coverage map, and binding, which needs the declaration's name to read
its signature. Fixed in `core/src/trace/anchor.rs`: the Lean entry is
`"definition"`, and `is_declaration` requires `node.is_named()`.

**One runner package, one requirement.** `drt_generate_runner` wrote its module
into the single `.tracelean/drt` directory, so generating for a second
requirement overwrote the first — while every binding's `model.cmd` still
pointed at the one `drtRunner` path. `service::drt_build_runner` now generates
one module covering every binding.

**Every binding called its entry point `"default"`.** With one shared runner the
dispatch `match` had several arms with the same literal: the first won and every
other requirement was answered by the wrong model function. `config::qualified_op`
makes the op `REQ-SHIPPING.flat`, and `drt_build_runner` refuses two bindings
that claim one op for different functions.

**`lake build` deleted the model.** The generated lakefile required the model
package by its *directory* name. The example's model lives in `formal/` and
declares `name = "checkout-model"`, and lake 4.12 answers that mismatch by
**removing the required package's source directory** — it prints
`package '«checkout-model»' was required as 'formal'` and deletes `formal/` on
the way. It destroyed the example's entire Lean model, recovered from the session
transcript. Two changes: `lean_runner::lake_package_name` reads the declared
name (quoting it as `«…»` when it is not a plain identifier), and
`lean_runner::check_requires` refuses to invoke `lake build` at all when a
`require` disagrees with the package it points at. Reproduced and pinned in
`a_require_naming_the_package_wrongly_is_refused_before_lake_sees_it`.

**Module and declaration names were unqualified.** Lean names a module from the
*lake package* root, not the project root, so `formal/Checkout.lean` imports as
`Checkout` (`entry_from_link_relative`); and a declaration inside
`namespace Checkout` is `Checkout.discountCents`, which the grammar cannot tell
us because it parses `namespace Foo` and its `end Foo` as flat siblings
(`namespace_prefix`).

**Relative paths were resolved against the wrong directory.** `.tracelean/drt.json`
is written project-relative, and nothing puts the running process in the project
root. `service::rebase_runner` makes the program absolute when the project
contains it and defaults the working directory to the project root.

Two quality fixes on top, both about not reporting a green that was never
earned:

- **`infer::boundaries`** collects every numeric literal in the model source,
  with its neighbours, as `Schema::Nat { edges }`. Without them an unbounded
  `Nat` is drawn from the whole 32-bit range: a model branching at 5000 and
  20000 is exercised above both in nearly every case, thousands of cases visit
  one band, and the coverage floor still says "met". Proof that it matters:
  changing `>=` to `>` at the 20000 threshold in `engine/pricing.py` produces
  **0 divergences** without the derived edges and **32 of 2000** with them.
- **`bind::propose` reports keyword mismatches.** The adapter calls the
  implementation with the *model's* binder names; the example's model says
  `subtotal` where Python says `subtotal_cents`, so the generated call would
  raise on the first case. The proposal now names both lists. The mapping stays
  the author's — matching positionally would silently swap two arguments of the
  same type, which is how a harness comes to agree about the wrong thing.

Known limitation, stated rather than hidden: the `all-input-constructors`
coverage floor is vacuous for a scalar or single-struct input — it reports one
constructor named `value`, seen, floor met. It says nothing about whether the
run reached the model's branches. The derived edges are what make the run
diverse; the floor is not yet the thing that checks it.

### 6b — the project graph (done)

`core/src/trace/graph.rs` builds a graph whose **nodes are code**: directories,
files, and top-level declarations from the same tree-sitter symbol table the
anchors resolve against (`anchor::declarations_in`). Requirements are an overlay
— a tint, a list, a badge — rather than nodes of their own.

Three rules it keeps, which the trace graph did not:

- **Untraced code is in the graph.** `assurance: None` is the grey and is *not*
  `L1`: `L1` means somebody claimed this and nothing has checked it, grey means
  nobody claimed it. Collapsing the two would make an unannotated file look like
  an annotated one awaiting verification.
- **Assurance aggregates by the minimum**, the same weakest-link rule as the
  roll-up. A test pins that every node's tint equals the minimum over its
  requirements, computed independently.
- **Edges carry their confidence.** Containment is `Certain`; a reference found
  by matching a name in source text is `Textual`, and the panel says what that
  means. Reference edges stay inside one language — a Python test mentioning
  `price` is not a reference to the Lean `price`, and drawing that edge buried
  the two relations worth seeing.

Capped at 600 declarations, with `omitted_declarations` reported rather than
silently truncated; files and directories are never capped, so the shape of a
large project survives even when its detail does not.

The frontend (`react_frontend/components/ProjectGraph.tsx`) lays containment out
as a squarified treemap, sharing §6a's implementation, and a finding renders as
a badge *on* its node. Selecting a requirement in the trace panel highlights
every node serving it (`TracePanel`'s `onRequirementFocus`), and selecting a
node lists its requirements, assurance, findings and references.

### 2 — the trace graph is gone

Deleted: `core/src/trace_graph/` (820 lines), `TraceabilityDashboard.tsx` (329
lines), `tests/unit/test_trace_graph.rs`, `TraceGraph`/`TraceGraphStats`/
`TraceGraphWrapper`, `SharedApp.graph`, `AgentContext.graph`, `AiService.graph`,
`service::build_trace_graph`/`update_trace_graph_file`, and six IPC commands.

Two things were *replaced* rather than dropped, because deleting them would have
cost a capability:

- The agent tools `query_trace_graph` and `query_code_element` became one
  `query_project_graph(req_id?, file?, symbol?)` backed by `trace::graph`. Two
  scanners over one project is two answers to every question, and the agent had
  been reading the one that knew nothing about evidence, staleness or findings.
- `ai::context`'s assembler was rebuilt on the annotation index. It had been
  finding a requirement and its model by *building paths* —
  `reqs/REQ-01.md`, `specs/REQ-01.lean` — which is the filename convention this
  project exists to replace. On a project laid out any other way, which is every
  project, it silently assembled nothing and the model was asked to reason about
  a requirement it had never been shown.

### Toolchain and containers

`rust-toolchain.toml` pins 1.95.0 with `rust-analyzer`, `rustfmt` and `clippy`,
so the laptop, the container and CI agree on the compiler and on the language
server the editor spawns.

- **`Dockerfile.dev`** — the image behind `make test`. Rust (pinned), Node 22,
  Python 3, Lean 4.12.0 via elan, pyright, rust-analyzer. Its last layer runs
  `--version` on every one of them: a container that builds green and then
  cannot elaborate Lean is the same lie as a dashboard reporting a check nobody
  ran. Two real bugs came out of that layer — `elan-init --default-toolchain`
  records the preference without fetching, so Lean was absent; and
  `pyright-langserver --version` exits 0 while printing an error, so the check
  now uses `pyright --version`.
- **`Dockerfile`** — release build, rewritten. It referenced `src-tauri/`, which
  has not existed since the crates were split into `core/`, `gui_backend/`,
  `tui/` and `tests/`, so it could not have built. `cargo install tauri-cli` is
  gone: `tauri-build` embeds `dist/` at compile time, so
  `npm run build && cargo build --release -p tracelean` produces the same binary
  without the long layer that compiles a CLI only to pass `--no-bundle`.
- **`Dockerfile.ci`** — adds Lean, Python, `npm test` and the end-to-end
  differential run, which now asserts instead of printing.
- **`.dockerignore`** — `.tracelean` was unanchored, so it also excluded
  `example/.tracelean`, i.e. the example project's bindings and policy. Now
  `/.tracelean`.

### Two things the container build found

Building the CI image was not a formality; it found two checks that were
reporting green without running.

**The CI build context was `tracelean/`, so half the end-to-end checks never
ran.** `example/` lives one directory above it, and the example tests are
written to `return` when the project is absent — the correct behaviour on a
machine without the fixture, and a silent pass inside an image that was supposed
to be exercising it. The keymap test failed outright for the same reason: it
regenerates the repository's root `README.md`, which was not in the context
either. The context is now the repository root, with a root `.dockerignore`.

**`bwrap_available()` asked the wrong question.** It ran `bwrap --version`, which
succeeds inside a container even though the default seccomp profile forbids
unprivileged user namespaces, so every real invocation fails with
`Creating new namespace failed: Operation not permitted`. It now probes an actual
namespace (`bwrap --ro-bind / / true`). That fixes more than CI: TraceLean was
offering sandboxed agent sessions in environments where they could not start.
The sandbox tests skip with that reason printed under `make ci` — in its own
`--nocapture` step, because `cargo test` swallows the reason and two skips
otherwise look like two ordinary passes. `make sandbox-test` runs them for real,
and needed finding out what bubblewrap actually wants from Docker:
`seccomp:unconfined` alone gets as far as `Failed to make / slave: Permission
denied`, and the default AppArmor profile has to be lifted too. With both, all
seven sandbox tests run in the container rather than skipping.

Two more volume-ownership bugs came out of running it. Docker seeds a named
volume from whatever sits at its mount point in the image, **ownership
included**, so `/usr/local/cargo/registry` and a non-existent `/app/target`
became root-owned volumes under a container that runs as a non-root user, and
every `cargo` invocation failed with "Permission denied". `CARGO_HOME` now lives
in the user's home and `target/` is created, owned, in the image.

### `make ci` is green, verified

`docker build -f tracelean/Dockerfile.ci .` exits 0 on a clean build from the
repository root:

- `cargo check --workspace --all-targets`, then `cargo test --workspace` — 661
  pass;
- the sandbox tests re-run alone with `--nocapture`, printing
  `skipping: bwrap cannot create a namespace here` twice, so the log states what
  it did not check;
- `npx tsc --noEmit` and `npm test` — 21 pass;
- the end-to-end differential path: the example's Lean model runner built inside
  the container covering six entry points, then all six bindings run at 2000
  cases each with zero divergences.

The image is large (~29 GB with build artifacts) because it is a throwaway: the
point is the exit code, not the image.
