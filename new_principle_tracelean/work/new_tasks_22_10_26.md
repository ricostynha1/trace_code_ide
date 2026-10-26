# New tasks — 2026-10-22

For review. Nothing here is implemented yet. Part A makes `P` mean what it says;
part B is an analysis of the tree's size and what could shrink.

Under the method, each task that changes behaviour becomes clauses in an
existing requirement (`REQ-EVID`, `REQ-STRENGTH`, `REQ-CHECK`) before code.

---

# Part A — `P` must be a proof of the clause

## The problem, with examples

Of the 288 theorems carrying `@proves`, **189 are closed statements**: no
variable, a few fixed inputs, closed by `native_decide`. They are unit tests
written in Lean, and they earn the same `P` as a general law.

`REQ-NAV.declaration_line` — *"A line shall declare the word after its
declaring word — `fn`, `struct`, `def`, `theorem`, `function` and the like —
when only modifiers come before that word …"* — is "proved" by four strings
(`formal/TraceLean/Nav.lean:187`):

```lean
theorem the_word_after_the_declaring_word_is_declared :
    declaredOn "    pub(crate) fn go(a: u8)" = some "go" ∧
    declaredOn "theorem open_ok : True := trivial" = some "open_ok" ∧
    declaredOn "let Kind::File p = kind" = none ∧
    declaredOn "x = open(1)" = none := by
  native_decide
```

`REQ-SCREEN.buffer_goes_home` is worse: the claimed proof is four buffers on
one fixed screen (`Screen.lean:1166`), while a general theorem a few lines up,
`destination_is_placed_or_stays` (`Screen.lean:432`), quantifies over every
kind and every screen and is deliberately *not* claimed, because it covers
only part of the clause.

Where the closed ones are: REQ-SHOW 21, REQ-SCREEN 15, REQ-REQDOC 10, REQ-ACT 8,
REQ-DRT-COVER 8, REQ-MYTH 8, REQ-VIEW 8, REQ-STRENGTH 7, REQ-DRT-PROTO 7,
REQ-ANCHOR 6, REQ-ANNOT 6, REQ-UNDO 6, and fewer elsewhere.

Today `crates/core/tests/proofs_are_earned.rs` refuses a theorem only when Lean
warns it uses `sorry`. Nothing looks at the statement's shape or the axioms.

## A1. A closed statement does not earn `P`

**What.** Classify each `@proves` theorem from its elaborated type:

- **general**: the statement binds at least one variable that flows into the
  model's arguments (`∀ x, …` or binders in the signature);
- **example**: no such variable — the statement is about fixed values.

Only *general* earns `P`. An *example* is reported as what it is: it may stay
in the tree as documentation, but it earns no mark (the differential test
already covers fixed inputs, with far more of them).

**Where.** The proof backend that writes `ModelProof` records
(`proofs_are_earned.rs`, `trace/earn.rs`). The classification has to come from
Lean, not from our tree-sitter text: have the backend emit, per theorem, its
binder count over the model's inputs (a small Lean script run by `lake env`).

**Clauses to add** (REQ-EVID or REQ-STRENGTH): *a theorem whose statement binds
no input of the model shall not establish the model-property bond*; *a refused
theorem shall be reported with its reason*.

**Done when.** `requirements` shows `P` only for clauses with a general theorem;
the 189 drop; the checker names each one ("example only — needs a general
statement"). Expect REQ-SHOW and REQ-SCREEN to lose most of their `P`.

## A2. Axioms: allowed when declared, refused when they come from tactics

Some axioms are wanted: an external call (sandbox copy, a probe, a transcript
read) has no Lean definition, so its *property* is assumed. These already exist
in `formal/TraceLean/Effects.lean` (`sandboxCopy`, `copy_obeys_its_law`,
`probeContainment`, `observedWrites`, `readTranscript`, …).

**What.** For every theorem that would earn `P`, record the axioms it depends
on (`#print axioms`, from the same Lean script as A1) into the evidence record.
Then:

| Axiom | Result |
|---|---|
| `propext`, `Classical.choice`, `Quot.sound` (Lean's own) | allowed |
| a project axiom **declared** as an external (see below) | allowed, and shown: `P` assuming `copy_obeys_its_law` |
| `Lean.ofReduceBool` (what `native_decide` adds) | refused — trusts the compiler, not the kernel |
| `sorryAx` | refused (as today) |
| any other undeclared axiom | refused, finding `UndeclaredAxiom` |

**Externals get proper definitions.** An external is declared once, in one
place, as an opaque operation with its stated law, and an annotation says what
in the implementation it stands for, e.g.

```lean
/-- The sandbox's copy of a workspace. @external crates/core/src/observe/workcopy.rs::copy -/
axiom sandboxCopy : Workspace → Workspace
/-- @assumes REQ-SBX.copy_is_faithful -/
axiom copy_obeys_its_law : ∀ w, sandboxCopy w = w
```

so the list of what the project *assumes* is a report (`tracelean-view .
axioms`), each assumption tied to a clause and visible on every `P` that leans
on it. Changing an assumed law makes the records that used it stale (it is one
of the record's inputs, like the model).

**Done when.** A proof by `native_decide` never earns `P`; every `P` lists its
assumptions; an axiom with no `@external`/`@assumes` is a finding.

## A3. `P` is a proof of the clause's spec

**What.** `P` is granted only to a theorem whose statement is the clause's
`@specifies` Prop applied to the model, for all inputs:

```lean
/-- @specifies REQ-NAV.declaration_line -/
def DeclaresWord (line : String) (w : Option String) : Prop := …

/-- @proves REQ-NAV.declaration_line -/
theorem declaredOn_spec : ∀ line, DeclaresWord line (declaredOn line) := …
```

The checker matches the theorem's type against the spec (head constant is the
`@specifies` def, every argument of the model bound). A clause with no
`@specifies` cannot earn `P` — today only 25 `@specifies` exist, so this is
the main backlog this task creates. The spec, short and readable, is what a
person judges against the requirement text (`J`).

**Done when.** Each `P` names its spec; `--stale`-style report lists clauses
with a model and no spec, ordered by how many implementations depend on them.

## A4. A clause too big to prove is split, not partly proved

Decided: no "proved in part" mark. When a general theorem covers only part of
a clause (the `buffer_goes_home` case), the requirement is at fault — it says
two things in one clause. Split it into clauses that can each be proved whole.

**What.**

- A report of candidate clauses to split: clauses whose text has several
  "shall" conditions joined by `;`/`and`/`—`, or whose model is used by more
  than one `@specifies` piece, or which have a general theorem that is not
  claimed (as in `Screen.lean:432`).
- Splitting is a requirement change, so it goes through the owner: the agent
  proposes the split, the person approves, judgements re-open.

First candidates: `REQ-SCREEN.buffer_goes_home` (where a kind belongs / what
happens when that pane is gone), `REQ-NAV.declaration_line` (declaring words /
modifiers / paths declare nothing).

## Order

A2's axiom recording and A1 share the one Lean script, so do them together;
then A3 (needs specs written — the long part); A4 as the A3 backlog shows which
clauses will not go through whole.

---

# Part B — Size: 82.6k lines, where, and what can shrink

## Where the lines are

| Area | Lines | Files | Note |
|---|---:|---:|---|
| core — src | 28,524 | 86 | the kernel and the pure surface |
| core — tests | 22,120 | 67 | 15,828 of them in 47 `differential_*.rs` |
| Lean — models | 15,082 | 59 | |
| Lean — `Spec*.lean` | 3,291 | 10 | |
| editor — src | 5,288 | 5 | `lib.rs` alone 4,789 |
| editor — tests | 2,790 | 11 | |
| web — src / test | 1,857 / 860 | 8 | the second implementation of the frontend |
| tui — src / tests | 893 / 1,227 | 5 | |
| desktop, demo, tools | 671 | 10 | |
| **total** | **82,635** | | `vendor/` (tree-sitter grammar) excluded |

What the Rust under `src/` actually is: **18,582 code**, 8,112 comment (30%),
5,956 inline `#[cfg(test)]`, 2,343 blank. So production Rust is about 18.6k
lines; **tests are the largest single thing in the tree** (~22k + 6k inline +
2.8k editor + 1.2k tui ≈ 32k).

## Duplication found

Measured by repeated 6-line windows after normalising literals and numbers:

| Scope | Lines in a repeated window |
|---|---:|
| Rust src | 538 of 19,178 (3%) — low |
| Rust tests | **3,673 of 14,601 (25%)** |
| Lean | 684 of 13,536 (5%), mostly `SpecLsp.lean` |
| tui + web frontends | 83 of 1,508 — low |

Copy-paste in production code is not the problem. The size comes from
**things stated twice** and **tests written by hand that the tree already
knows how to generate**.

### B1. Every differential binding is written twice — the biggest win

`.tracelean/drt.json` has 169 bindings, each naming the requirement, clause,
Lean function and arguments, and Rust entry. Then 48 test files construct 231
runners by hand, restating exactly the same names
(`crates/core/tests/differential_hash.rs:68`):

```rust
let op = "REQ-ANCHOR.whitespace_normalised";
let implementation = harness::rust_runner("REQ-ANCHOR", "whitespace_normalised",
    "crates/core/src/trace/hash.rs::normalize_of", &scratch);
let model = harness::lean_runner("TraceLean.Hash", "TraceLean.Hash.normalize",
    op, &["source", "removed", "protected"], &scratch);
let result = run(op, &input_schema(), &model, &implementation,
    RunOptions { seed: 55, cases: 3_000 }).expect("both runners answer");
support::agreed(&result);
```

The only thing a test file adds is the **input schema** (872 `Schema::` uses
across 49 files) plus a seed and a case count. A test,
`bindings_are_real.rs`, exists to check the two copies agree — the symptom of
two sources of truth. Meanwhile `drt/auto.rs` + `drt/derive.rs` already run a
differential test from the signatures alone (`tracelean-trace . --drt`).

**Change.** One place per binding: put the schema (or "derived"), seed and
cases into the binding (`drt.json`, or a `drt/` directory of one file per
requirement if one JSON grows too big). One generic test runs every binding.
Where `derive` can produce the schema from the two signatures, the binding
says nothing about it. Hand-written tests remain only for what is not a
binding: negative tests that bind the wrong function on purpose, shrinking
checks.

**Estimate.** Most of the 15.8k lines in `differential_*.rs` go; schemas move
into data (~3–5k lines of JSON, less where derived). `bindings_are_real.rs`
goes. Also aligns with the "generated code must not duplicate" rule.

**Risk.** Hand-written schemas carry comments on *why* an edge is there
(e.g. "ranges are unconstrained on purpose"). Keep a `why` field per schema
node, or the reasons are lost.

### B2. `crates/editor/src/lib.rs` — 4,789 lines, one `impl Editor`

172 methods in one impl, 110 `match`es; the largest are `refuse` (220 lines),
`chord` (135), `watch`, `perform`, `produce` (~95 each), `file_trace`,
`requirement`. The judge already recorded `ARCH-CORE-SHELL.shell_thin` as
drift because this file decides a lot and is not held to the shell ceiling.

**Change.** Not a line-count goal, a placement one: every branch here that
decides something (what a refusal says, which buffer a kind produces, how a
chord resolves) is a candidate to move into `core::surface`, where it is pure
and already modelled; what stays is effects. Then add `editor` to the shells
`shell_thin` checks. Expected result: the file shrinks, and some of `produce`
and `requirement` turn out to duplicate `core::surface::produce` /
`requirement_view` — check each before moving.

### B3. Comments — 30% of Rust src, 5.3k lines of Lean comments

The doc comments are good, but many restate the requirement clause the
annotation already links to. **Change:** a pass where a comment that only
paraphrases its `@implements` clause is cut to the annotation; reasons that
are not in the requirement stay. Worth maybe 2–3k lines; low risk; do it file
by file as files are touched, not as one sweep (each touched annotation
re-opens judgements).

### B4. Proof examples that A1 demotes

189 closed theorems (each ~5–15 lines with docstring, ~2k lines). After A1
they earn nothing. Each one either becomes a general theorem (A3) or is
deleted — they repeat what the differential test checks with thousands of
cases. Not a separate task: a consequence of A1/A3.

### B5. Small ones

- `trace/earn.rs` — 108 lines in repeated windows within the file (27 shared
  windows with itself, 9 with `lockfile.rs`): record-building repeated per
  bond kind. Factor the per-bond construction.
- `drt/rust_runner.rs` ↔ `drt/ts_runner.rs` ↔ `drt/lean_runner.rs` — shared
  runner-generation scaffolding (12 windows rust↔ts). One generator with a
  per-language backend.
- `SpecLsp.lean` — `go_spec` and `go_character_spec` are two ~150-line proofs,
  mirror images (column→offset, offset→column). A shared lemma over a generic
  "walk the characters" could carry both.
- `surface/offer.rs` — 62 repeated lines internally.

### B6. A size check so it does not come back

Add the duplication measure above (repeated-window share, per area) as a
report beside the coverage report, compared against itself over time, not
against a threshold. And the `ideas.md` §4 report (two models of one concept)
for the Lean side.

## Suggested order for Part B

1. **B1** — removes the largest block and a second source of truth.
2. **B2** — closes a recorded drift (`shell_thin`).
3. B5, then B3 as files are touched; B4 falls out of Part A.

Rough outcome if B1 and B2 land: ~82k → ~65–68k lines, with production code
roughly unchanged and the tests turned mostly into data.
