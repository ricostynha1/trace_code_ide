---
adr: 1
title: The new tree is a valid input to the old tool from day one
status: accepted
affects: [ARCH-SELFHOST]
---

# ADR-0001 — Bootstrap on stage-0 formats

## Context

This port has to be checkable before it can check anything. Until its own trace
kernel runs, the only tool that can scan it, resolve its anchors and run its
differential tests is the existing `tracelean/` tree.

## Decision

Every on-disk format this project writes is constrained to what the existing
TraceLean already parses: requirement frontmatter (`id`, `title`, `refines`,
`status`, `decomposition`, `clauses`), the annotation grammar
(`@role ID.clause key="value"`), `.tracelean/drt.json`, and the lockfile schema.

These were read out of the stage-0 source rather than designed, specifically:
`trace/requirement.rs::parse_markdown`, `trace/annotation.rs::annotation_re`,
`drt/config.rs`, `trace/lockfile.rs`.

Format improvements wait until stage 2, when the new kernel can read its own
tree and a format change stops being a change to somebody else's parser.

## Consequences

Two constraints inherited from the stage-0 parsers are worth writing down,
because violating either fails silently rather than loudly:

- A clause key may contain only `[A-Za-z0-9_]`. Hyphens are legal in a
  requirement id but **not** in a clause key, because the annotation regex
  splits id from clause on the first `.` and stops the clause at the first
  non-word character. `REQ-EVID.weakest_link` resolves; `REQ-EVID.weakest-link`
  silently resolves to the clause `weakest`.
- Frontmatter is a tiny hand-rolled subset, not YAML: `key: value`,
  `key: [a, b]`, and one level of nesting under `clauses:`. Anything else is
  reported as a frontmatter problem.

## Rejected

*Design the formats properly first.* It produces a tree nothing can read, which
means nothing is checked until the entire kernel is finished — the exact
big-bang the staged approach exists to avoid.
