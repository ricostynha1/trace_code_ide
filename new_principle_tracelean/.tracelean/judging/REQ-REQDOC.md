# Judging advice — REQ-REQDOC

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `clause_addressable` | drift |
| `decomposition_claimed` | drift |
| `malformed_reported` | drift |
| `refines_dag` | agrees, with a note |
| `refines_resolves` | agrees (re-judged with spec; see below) |

## `clause_addressable`

*Drift.* The only model is `requirementHash`. It hashes *all* clauses and the
prose into one digest. As a staleness key, that makes the requirement the unit
evidence attaches to, not the clause: reword clause `a` and the hash under
evidence for clause `b` moves too. Decomposition into addressable clauses
(`Parsed.addressable`) is not bound to this clause.

## `decomposition_claimed`

*Drift: the second half is missing.* `parseLines` defaults to `open`, and a
misspelt value also falls to `open`, which is the safe direction. Rendering a
percentage over an open decomposition as a lower bound is not modelled.

## `malformed_reported`

*Drift: a document can be skipped silently.*
- `["---", "id: REQ-X", "title: T"]`, a fence that is never closed, makes
  `splitFrontmatter` answer `none`, and `parseLines` returns `{}` with no
  problem.
- A misspelt key such as `iD: REQ-X` is a well-formed key/value pair, so there
  is no `id` and no problem.

Either way a would-be requirement disappears without a report. The proof
`prose_is_not_a_requirement` only checks a document with no fence at all.

## `refines_dag`

*Note.* `canonGraph` keeps one declaration per duplicated id (later wins), so a
cycle that runs only through the dropped declaration's `refines:` is not found.
That duplicate is reported under `id_unique`, so nothing passes silently.

## `refines_resolves`

*Agrees.* Spec: `SpecSorted.UnresolvedRefines` says the report holds exactly
the (requirement, parent) pairs where a declared parent names no node. It adds
two conventions, both reasonable: each pair appears once, and pairs are in
ascending order (`PairLt`). `refines_resolves_pinned` holds for `danglingIn`.
`graphReport` canonicalises the graph first.
