---
adr: 16
title: A clause may carry narrowings, and evidence rests on its own clause
status: accepted
affects: [REQ-REQDOC, REQ-STALE, REQ-ROLLUP]
supersedes-part-of: ADR-0001
---

# ADR-0016 — Narrowings and clause hashes

## Context

Pinning shows most clauses leave something open (the empty case, an order, a
format), and a spec may use only what the clause says. Separately, evidence is
per clause but rested on a hash of the whole requirement, so rewording one
clause re-opened every clause's judgement, tests and proofs. ADR-0001 froze
the frontmatter to the stage-0 subset until stage 2; that freeze has ended.

## Decision

**Format.** Under `clauses:` a clause is either one line or a block:

```yaml
clauses:
  weakest_link: The minimum over bonds.
  min_not_mean:
    text: The aggregate shall be the minimum of its parts' levels.
    empty: With no parts, the aggregate shall be L1.
```

`text:` is the clause; every other key is a **narrowing**. Keys are
`[A-Za-z0-9_]`; `text` is reserved. Reported (named kinds, Rust and Lean):
a block without text, a clause or narrowing given twice, an invalid key, a
fence never closed, an unknown top-level key (in a requirement, or a
requirement key misspelt in case such as `iD:` anywhere).

**Meaning.** A narrowing fixes *which answer* a clause allows. It is part of
its clause: annotations name only the clause, it adds nothing to a roll-up's
denominator, and the judge, the spec and the pin see it with the clause.

**Hash.** The `requirement` input of a clause's records is the hash of that
clause alone: key, text, narrowings (sorted). A requirement without clauses
hashes its body, as before. Prose is not hashed into evidence; document
review (`doclink`) covers it through the whole-document hash.

## Consequences

Every existing record went stale once when this landed (the hashed content
changed), then settles. Adding a narrowing re-opens its clause only.
