---
name: tracelean
description: Use when working in a repository that uses the TraceLean method — requirements with clauses (REQ-X.clause), Lean models, @implements/@models annotations, differential testing, evidence marks, or the tracelean-trace / tracelean-view commands. Triggers on "tracelean", "REQ-", "clause", "@implements", "@models", "--stale", "--context", "findings", "Unbound", "Unmodeled".
---

# Working on a TraceLean project

The full pages live in `pages/` next to this file (a symlink to
`new_principle_tracelean/skills/`). Read `pages/00-the-method.md` first, then
only the page the task needs.

| Page | Read when |
|---|---|
| `pages/00-the-method.md` | always: the claim, vocabulary, bonds |
| `pages/01-orienting.md` | unfamiliar tree |
| `pages/02-annotations.md` | linking code, models or tests to clauses |
| `pages/03-requirements.md` | reading, writing or changing a requirement |
| `pages/04-models-and-bindings.md` | adding a model, comparing it to code |
| `pages/05-evidence.md` | what a claim is worth (note: the repo moved from the L1–L4 ladder to four independent marks, AJTP; if the page disagrees with the repo's docs, the repo wins) |
| `pages/06-making-a-change.md` | the loop for any change |
| `pages/07-findings.md` | reading the checker's output |
| `pages/08-setup-in-case-of-error.md` | only when a command fails |

## The method

```
requirement ──@models──► formal model ──@implements──► implementation
 (a clause)              (Lean function)                (code)
```

Every arrow is a comment somebody wrote; nothing is inferred. Bonds are never
averaged: a strong bond must not hide a missing one. `Unmodeled` is
information; `Unbound` (model and code exist, nothing compares them) is the
finding that matters most.

## Three commands

```bash
tracelean-trace .                            # what the project claims, and what is missing
tracelean-trace . --context REQ-X.clause     # everything needed to change that clause (run BEFORE changing)
tracelean-trace . --stale                    # what a change re-opened (run AFTER; redo or report it)
```

`tracelean-view . <station>` prints what the editor shows as text
(`requirements`, `design`, `requirement REQ-X`, `trace <file>`,
`coverage REQ-X`, `findings`, `evidence`). Add `--json` for the raw buffer.
Other `tracelean-trace` flags (`--show`, `--drt`, `--lock`, `--judge`,
`--hashes`, `--pins`, `--strength`, `--coverage`, `--unparsed`) are tabulated
in `pages/README.md`.

## Rules you will break first

1. No code without an annotation linking it to a clause. No clause? Write the clause first.
2. No link from a filename; only an `@implements` comment makes one.
3. Never write an evidence level or mark; a run earns it.
4. Never exempt or mark `@structural` to silence a finding.
5. Never test by supplying the value the code should have produced.
6. Run the checker before saying you are done.

If the repository's own docs (e.g. `docs/decisions/`) contradict these pages,
the repository wins.
