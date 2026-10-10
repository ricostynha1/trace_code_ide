# Working on a TraceLean project

How to work in a repository that uses the TraceLean method. Read
[00-the-method.md](00-the-method.md) first.

| | |
|---|---|
| [00-the-method.md](00-the-method.md) | The claim, the vocabulary, the levels |
| [01-orienting.md](01-orienting.md) | What to read first in an unfamiliar tree |
| [02-annotations.md](02-annotations.md) | The only mechanism linking anything to anything |
| [03-requirements.md](03-requirements.md) | Reading, writing and changing a requirement |
| [04-models-and-bindings.md](04-models-and-bindings.md) | Adding a model and comparing it to the code |
| [05-evidence.md](05-evidence.md) | What a claim is worth, and why you cannot write that down |
| [06-making-a-change.md](06-making-a-change.md) | The loop for any change |
| [07-findings.md](07-findings.md) | Reading the checker's output |
| [08-setup-in-case-of-error.md](08-setup-in-case-of-error.md) | Only when a command fails: tools, errors, Lean the checker cannot read |

## Your three commands

```bash
tracelean-trace .                                  # what the project claims, and what is missing
tracelean-trace . --context REQ-X.clause           # everything you need to change REQ-X.clause
tracelean-trace . --stale                          # what your change re-opened, and what to run for each
```

`--stale` lists every judgement, differential test, proof, pinning verdict,
document and coverage measurement that something it rests on has changed
under — a reworded requirement re-opens all of them — and exits 1 if any.
**Run it after a change, and redo or report what it lists.**

`--context` prints, as Markdown, the clause, what it refines, the code, tests
and Lean models that claim it (with source), the tests likely to break, and
what each clause still lacks. Choose parts with `--parts`
(`requirement,refines,refined-by,code,tests,models,affected-tests`, or `all`).
**Run it before changing anything a requirement covers.**

In a TraceLean sandbox all of it is on hand: `tracelean-trace` is on `PATH`,
`$TRACELEAN_SKILLS` is this directory, and Lean and Rust are the host's. A
project sends its agent here with a `CLAUDE.md` like `demo/CLAUDE.md`. If a
command fails, [08](08-setup-in-case-of-error.md).

## Every flag

All take the project root first (`.`). "Writes" is under `.tracelean/`.

| Command | Does | Writes |
|---|---|---|
| *(none)* | counts per finding kind; last line says `blocking: true/false` | — |
| `--show <Kind>` | lists one kind in full (`Unbound`, `Dangling`, …: [07](07-findings.md)) | — |
| `--context X [--parts …]` | a clause's or requirement's context, as Markdown | — |
| `--stale` | what to redo, with the command for each; exit 1 if any | — |
| `--drt` | compares every clause with a Lean `def` modelling it and a Rust `fn` implementing it, no binding needed; records L3 where earned | evidence, lock |
| `--lock` | folds evidence into `trace.lock`, naming what went stale | lock |
| `--judge X` | prints the judging prompt for a person | — |
| `--judge X --verdict agrees\|drift\|unmodelable --by <who> [--delegated-by <who>] [--note …]` | records **a person's** verdict ([05](05-evidence.md)) | evidence, lock |
| `--hashes` | what each requirement hashes to now, for a document's `described_hash` ([03](03-requirements.md)) | — |
| `--pins` | asks Lean whether each `@pins` theorem pins its clause | pins |
| `--strength [symbol]` | what each model owes toward L4, and the statement to prove | — |
| `--coverage` | runs each Rust test alone under coverage: which tests ran which lines | coverage |
| `--unparsed <file>` | where the grammar stopped reading a file | — |

## The rules you will break first

1. **No code without an annotation** linking it to a clause. No clause? Write
   the clause first ([03](03-requirements.md)).
2. **No link from a filename.** Only an `@implements` comment makes one.
3. **Never write an evidence level.** Levels are earned by a run ([05](05-evidence.md)).
4. **Never exempt or mark structural to silence a finding.**
5. **Never test by supplying the value the code should have produced.**
6. **Run the checker before saying you are done.**

If the repository's own docs (e.g. `docs/decisions/`) contradict these pages,
the repository wins.
