---
adr: 8
title: An unparsed model file caps its own claims, and says so
status: accepted
affects: [REQ-ANCHOR, REQ-ANNOT]
---

# ADR-0008 — Lean grammar limits are reported, not absorbed

## Context

Anchoring needs a parser. The available Lean grammar (`tree-sitter-lean4` 0.3.0)
does not read everything Lean 4.12 accepts: `mutual` blocks fail outright and
take the whole file's declarations with them, `termination_by` and `where`
truncate, and some definitions in this project's own models produce an error
node that loses every declaration after it.

The scanner was reporting those files as precisely anchored anyway. That is the
worst available behaviour: annotations after the error silently anchored to the
whole file while the scan claimed to be precise, so a claim about one function
was recorded as a claim about a file, at full confidence.

## Decision

A parse containing an error node marks the scan **imprecise**. Its annotations
anchor to the whole file, are capped at the lowest evidence level, and the file
is reported under its own finding kind, `Imprecise`, at warning severity.

Not an error: the claims are real and still recorded, only not precisely placed.
Blocking a build because a language construct is outside a third-party grammar
would be the tool failing the project for its own limitation.

The models are written in the subset the grammar reads wherever that costs
nothing, and the table below says what each rewrite replaced. A model checked by
a tool has to be written in the language the tool reads, and the comment saying
so sits beside each definition it applies to.

Scopes are read from the text, because the grammar makes `namespace … end` a
pair of siblings rather than a node containing its declarations. Only an `end`
that *names* a scope closes one: Lean writes a bare `end` to close a `mutual`
block or an anonymous section, and treating that as the end of the enclosing
namespace moved every later declaration out of it — an anchor for a symbol that
does not exist under that name, which is worse than an imprecise one.

## Consequences

One model file reports as imprecise — `Command.lean`, for the reason above — so
its annotations are capped (per annotation, not per file — see ADR-0011). The
differential evidence is unaffected — that comes from running the model, not
from parsing it — but the `@models` and `@proves` links in that file are L1
until the grammar improves.

This is visible in the checker's output rather than buried, which is the whole
point: a capped claim a person can see beats a confident claim that is wrong.
`tracelean-trace --unparsed <file>` prints the regions themselves, so the cap can
be looked at rather than guessed about.

Looking at them settled what can be done. Every construct the grammar could not
read had an equivalent spelling it can, and the equivalent says the same thing:

| Not read | Written instead |
|---|---|
| `h₁`, `h₂` | `h1`, `h2` |
| `a ∈ l`, `a ∉ l` | `List.Mem a l`, `Not (List.Mem a l)` |
| `A ⊕ B` | `Sum A B` |
| `\| .a \| .b => x` | one arm per line |
| `by_cases h : e` | `cases e` |
| `rcases h with a \| b` | `refine h.elim ?_ ?_` then `intro` |
| `induction xs with \| nil => …` | `induction xs` then `case nil => …` |
| `unfold f at h` before a `<;>` chain | `simp only [f] at h` |
| `split at h <;> …` | `split at h` then `all_goals …` |
| `x ^^^ y`, `x >>> n`, `x &&& m` | `UInt64.xor`, `UInt64.shiftRight`, `UInt64.land` |
| `'\x0c'` | `Char.ofNat 12` |
| `"a {b}"` | a string built from `Char.ofNat 123` |
| `let rec go` | a top-level `private def` |
| `fun acc (kv : A × B) =>` | `fun acc kv =>`, or a named definition |
| a lambda holding a `match` over several lines | a named definition |
| a `let` whose value is a multi-line `match` or `if` | a named definition |
| a `let` in **both** branches of an `if` | `match b with \| true => … \| false => …` |
| a destructuring `let (a, b) :=` in a branch | `.1` and `.2` |
| `.open` for a constructor named after a keyword | a definition returning it |
| a structure instance as a multi-line argument | a named definition |
| `«theorem»`, `«by»`, `«case»` as field names | `theoremName`, `judgedBy`, `caseNumber` |

So the models are written in that subset, each rewrite carrying a comment saying
which limit it is written around, and the wire names the renamed fields produce
are matched on the Rust side.

**One construct is not written around.** `Command.lean` defines `apply` and
`inverse` by recursion into a batch's members, which Lean 4.12 accepts only with
`List.attach` and a `decreasing_by` proof — and `decreasing_by`, `termination_by`
and `mutual` are all unreadable to the grammar. Every alternative (fuel, a
`mutual` block, `partial`) changes the definition rather than its spelling: fuel
needs a computable size the derived `SizeOf` does not give, and `partial` would
not reduce, so the round-trip theorems could not be stated. So that one file's
annotations stay capped at L1, and this is the reason.

## Rejected

*Fall back to a line-based scan for files that fail to parse.* It would recover
declaration names by pattern-matching text, and be wrong in exactly the cases
that made the parse fail. A binding that cannot be placed is better than one that
appears to be.
