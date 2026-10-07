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

## Your two tools

```bash
tracelean-trace .                                  # what the project claims, and what is missing
tracelean-trace . --context REQ-X.clause           # everything you need to change REQ-X.clause
```

`--context` prints, as Markdown, the clause, what it refines, the code, tests
and Lean models that claim it (with source), the tests likely to break, and
what each clause still lacks. Choose parts with `--parts`
(`requirement,refines,refined-by,code,tests,models,affected-tests`, or `all`).
**Run it before changing anything a requirement covers.**

In a TraceLean sandbox both are on hand: `tracelean-trace` is on `PATH` and
`$TRACELEAN_SKILLS` is this directory. A project sends its agent here with a
`CLAUDE.md` like `demo/CLAUDE.md`.

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
