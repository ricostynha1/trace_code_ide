# TraceLean

A traceability-first IDE core, built under its own methodology.

TraceLean defines what it means for a project to be *proper*: requirements with
identity, a formal model saying what each clause means, an implementation
claiming to realise it, and the claim checked rather than believed. This tree is
TraceLean rebuilt under those rules — the same move as compiling a compiler with
itself.

**New here? Read [docs/USING.md](docs/USING.md)** — how to use the editor, in
five minutes. To change it — keys, actions, colours, panels — read
[docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md).

## Layout

```
reqs/       requirement documents — indexed in reqs/README.md
formal/     Lean 4 models (no mathlib)
crates/     core     the traceability kernel, and everything pure
            editor   the editor's state: what an intent does
            tui      the terminal frontend
            desktop  the window (Tauri)
web/        the page the window loads — a second implementation, checked as one
docs/       methodology, decisions, progress
assets/     keymap.json — the modal keymap this project ships
demo/       a small ordinary tree to open the editor on, and the one
            the driving suite drives
skills/     how to work on a project that uses this methodology
tools/      the stage-0 driver: the old TraceLean, tracing this tree
.tracelean/ drt.json — every differential binding
```

A key becomes an action ([REQ-MYTH](reqs/surface/REQ-MYTH.md)), an action becomes
an intent ([REQ-ACT](reqs/surface/REQ-ACT.md)), an intent becomes a buffer
([REQ-SHOW](reqs/surface/REQ-SHOW.md)), and a frontend draws it
([REQ-VIEW](reqs/surface/REQ-VIEW.md)). Every arrow is a function from a value to
a value, modelled and differentially tested; a frontend owns the last one only.
See [docs/06-editor.md](docs/06-editor.md).

**[reqs/README.md](reqs/README.md)** indexes all forty-one requirements by area.

## Checking this tree

```bash
# what this project claims, and what backs it
cargo run -p tracelean-core --bin tracelean-trace -- .

# where a model file did not parse, and so what caps its claims
cargo run -p tracelean-core --bin tracelean-trace -- . --unparsed formal/TraceLean/Evidence.lean

# write the committed index, carrying recorded evidence through unchanged
cargo run -p tracelean-core --bin tracelean-trace -- . --lock

# unit tests
cargo test -p tracelean-core

# differential tests: one shared Lean package and one shared Rust crate, each
# built by the first suite that wants it and reused by the rest.
cargo test --workspace -- --ignored
tools/differential-all.sh        # the same run, using the whole machine

# the models
cd formal && lake build
```

## Running it

```bash
node web/build.mjs               # web/src/*.ts -> web/dist/*.js, no dependencies
node web/test/pointer.mjs        # the window's mouse and keys, in headless Chrome
node web/test/look.mjs out.png   # a photograph of the page, a real editor behind it
cargo run -p tracelean-tui       # the terminal frontend
cargo run -p tracelean-desktop   # the window
```

Both open on the first TraceLean's arrangement: the file listing on the left,
the document in the middle, and a side panel on the right that holds whatever a
station opens — requirements, the design graph, the sandbox
([REQ-SCREEN](reqs/surface/REQ-SCREEN.md) `workbench_has_three_places`,
`buffer_goes_home`).

**F1** lists every key. In the window: **click** a file to open it, **click into** a file to put the
cursor there and type, **right-click anything** for every action available
there (with the keys that reach it), drag or Shift+arrows to select, then type over it or `Ctrl+C`/`Ctrl+X`/`Ctrl+V`,
`Ctrl+W` closes a tab, `Ctrl+/` comments a line, `Ctrl+Left/Right` move by word,
`Ctrl+S` saves, `Ctrl+Z`/`Ctrl+Y` undo and redo, `Ctrl+F` finds (`F3` next), `Ctrl+H` replaces,
`Ctrl+P` opens a file by name, `Ctrl+Shift+P` does anything by name, `F12` or Ctrl+click goes to a
definition and `Alt+Left` comes back, `Ctrl+Shift+F` searches every file,
the wheel scrolls (the terminal has the same chords, `Ctrl+R` for replace; Tab in its find row
searches every file). Tabs and open folders come back when the project is
reopened. Folders open and close with a click; a requirement opens
with every clause and a `path:line` link to each claim. Colours and
fonts are `assets/theme.json`; a project overrides any of them in
`.tracelean/theme.json`, picked up when the window regains focus.

In both: `i` types, Escape leaves, `.` lists what can be done where the cursor
is, Space opens the leader menu, `q` closes the
terminal one. Under the leader: `f` files, `h` history, `t` trace, `d` differential
testing, `a` what an agent left in the tree.
[docs/09-using-the-tui.md](docs/09-using-the-tui.md) walks through a session on
[`demo/`](demo) — the tree `crates/tui/tests/driving.rs` opens on a real
pseudo-terminal, so the page and the suite name the same keys.

Reports are reads of the tree, so they answer offline. The three that would mean
*running* something — `drt run`, `drt shrink`, `drt coverage` — name the command
instead: the editor starts no processes, and never calls a model
([ARCH-NO-DRIVING](reqs/arch/ARCH-NO-DRIVING.md)).

The stage-0 driver needs building once, and is what the fixpoint test compares
against:

```bash
cd tools/stage0-trace && cargo build --release
```

## What the output means

| Finding | Means |
|---|---|
| `Unmodeled` | A clause has no Lean model yet. Information, not a fault. |
| `Unimplemented` | Modelled, nothing claims to implement it. |
| `Unbound` | Model and implementation both exist and **nothing compares them**. The most important finding here. |
| `Untested` | Implemented, no test. |
| `Imprecise` | An annotation sits in or after a region the grammar could not parse, so it anchors to the whole file and is capped at L1 ([ADR-0008](docs/decisions/ADR-0008-lean-grammar-limits.md), [ADR-0011](docs/decisions/ADR-0011-precision-is-per-anchor.md)). |
| `@structural` | Not a finding: a clause about the tree rather than about a value, answered by a test that reads the tree ([ADR-0012](docs/decisions/ADR-0012-structural-clauses.md)). |

Nothing in that list blocks a build except malformed documents, dangling
annotations, refinement cycles, duplicate identifiers, contested clauses and
unsound exemptions.

## Evidence

| Level | Means |
|---|---|
| `L1` | The annotation resolves. Nothing checked. |
| `L2` | A person judged requirement and model consistent. |
| `L3` | Differential testing found no disagreement, over a stated case count **that reached the situations the binding declared** ([docs/04-coverage.md](docs/04-coverage.md)). |
| `L4` | A Lean theorem discharges the property. |

Three bonds — requirement↔model, model↔implementation, model-property — graded
separately and aggregated by **minimum**, never averaged.

A level is **earned**, never annotated. A backend writes a record into
`.tracelean/evidence/`, and `--lock` folds the directory into the committed
index, dropping whatever went stale:

```bash
cargo test -p tracelean-core --test proofs_are_earned -- --ignored   # L4: what the kernel accepted
cargo run  -p tracelean-core --bin tracelean-trace -- . --lock       # collect into the lock
```

Each record names the inputs it depended on — the model it was checked against,
the implementation it compared, the toolchain that accepted it — so changing any
of them makes the record stale rather than leaving it standing.

**L3 needs both halves.** `coverage::level` grants it for agreement *and* a met
floor the binding declared. A situation is a predicate over generated values, so
the binding names it and its floor in `.tracelean/drt.json` while the generation
test counts how often the name was reached; the differential test reports
agreement. Neither half alone is L3, and the two are usually separate processes,
so each writes to `.tracelean/pending/` and whichever lands second composes the
record. 34 of 86 bindings declare floors; the rest answer `Undeclared` and stay
at L1. See [docs/04-coverage.md](docs/04-coverage.md) and
`REQ-DRT-COVER.floor_stated`.

## Reading order

[docs/00-methodology.md](docs/00-methodology.md) ·
[docs/01-bootstrap-ladder.md](docs/01-bootstrap-ladder.md) ·
[docs/02-scope.md](docs/02-scope.md) ·
[docs/03-doc-sync.md](docs/03-doc-sync.md) ·
[docs/04-coverage.md](docs/04-coverage.md) ·
[docs/05-view.md](docs/05-view.md) ·
[docs/06-editor.md](docs/06-editor.md) ·
[docs/07-frontends.md](docs/07-frontends.md) ·
[docs/08-ideas.md](docs/08-ideas.md) ·
[docs/09-using-the-tui.md](docs/09-using-the-tui.md) ·
[reqs/README.md](reqs/README.md) ·
[skills/README.md](skills/README.md) ·
[docs/decisions/](docs/decisions/) ·
[docs/progress.md](docs/progress.md) ·
[ROLLOVER.md](ROLLOVER.md)
