# Orienting in a tree you have not seen

Do this before you change anything. It takes a few minutes and it is the
difference between a change that fits the project and one that has to be undone.

## 1. Ask the project what it claims

The checker is the fastest way to understand a TraceLean project, because it
answers with the project's own account of itself: what is claimed, what backs
each claim, and what is missing.

```bash
tracelean-trace .          # or however this project invokes it — see below
```

Read the tail first: the counts of documents and findings, and whether anything
is blocking. Then read the findings. `Unmodeled` tells you where the
specification stops. `Unbound` tells you where it *looks* finished and is not.

**Finding the command.** A TraceLean project ships a binary that does this. Look
for it in that order:

1. The repository's own `README` — it almost always shows the command.
2. A `Makefile`, `justfile`, or `docs/` page.
3. The binary targets: in a Rust project, `grep -r "\[\[bin\]\]" --include=Cargo.toml`.

Every capability the project's editor exposes is also reachable from a command
line binary calling the same function, so anything you can do in the UI you can
do from a shell. Prefer the shell.

## 2. Read the requirement index

There is an index of requirements — commonly `reqs/README.md`. It groups the
documents by area and gives a clause count for each. Read the group headings and
the one-line titles. Do not read all the documents; read the index, then read
the two or three documents that touch what you were asked to change.

The frontmatter of a requirement carries the identity and the clauses; the body
says **why the clauses are what they are**. When you need to know whether your
change is in scope, the body is where the answer is.

## 3. Read the decisions

Look for `docs/decisions/` (ADRs) or equivalent. These record choices that
constrain what you may do — a parser the project cannot use, a thing that is
deliberately axiomatised, a rule about where effects live. A change that
contradicts an accepted decision is a change that gets reverted.

## 4. Find out what the project is built and checked with

Before running anything, find out what it needs:

- **Language toolchains** — `Cargo.toml` / `lean-toolchain` / `package.json` /
  `pyproject.toml`. A TraceLean project has at least two: the implementation's
  and the model's.
- **The model's package** — commonly a `formal/` directory with its own build
  file. Building it is usually `lake build` for Lean.
- **How the suites are split** — differential tests build a model package and a
  runner package and are slow, so they are usually behind a flag. In Rust that
  looks like `#[ignore]` and `cargo test -- --ignored`. Find out before you
  conclude the suite is fast.
- **Containers** — if the project ships a `Dockerfile` or similar, it is the
  authoritative list of what the suite needs. Read it even if you do not use it.

## 5. Find the project's state

Two places carry it:

- A **progress document** (`docs/progress.md` or similar) — living status,
  written for a person. What is done, what is left, what the open questions are.
- The **lock file** (often `.tracelean/trace.lock`) — the committed index of
  what has been established. Do not edit it by hand; it is written by a command.

If the project has a rollover or handoff document, read it. It is written for
exactly the situation you are in.

## 6. Then, and only then, look at the code

By now you know which clause your change is about. Find its annotations:

```bash
grep -rn "REQ-THING.the_clause" .
```

That returns the model, the implementation, the tests and the binding in one
shot — which is the whole point of the method. There is no filename convention
to guess at, because there deliberately is not one.

## A checklist you can run

- [ ] Checker run; findings read; nothing blocking (or you know why it is).
- [ ] Requirement index skimmed; the two or three relevant documents read.
- [ ] Decisions read.
- [ ] Toolchains identified; the fast suite run once so you know it was green
      before you touched it.
- [ ] Progress/handoff document read.
- [ ] `grep` for the clause you are about to change.
