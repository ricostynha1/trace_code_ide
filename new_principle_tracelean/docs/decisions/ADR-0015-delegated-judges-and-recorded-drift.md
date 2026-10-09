---
adr: 15
title: A person may delegate judging; every verdict is recorded
status: accepted
affects: [REQ-JUDGE]
amends: ADR-0003
---

# ADR-0015 — Delegated judges, recorded drift

## Context

ADR-0003 made the judge a person, but nothing checked who `--by` named: an
empty name was accepted, and agent verdicts made at a person's request were
signed `claude-review (simulated human)` with no record of who asked. A drift
verdict wrote nothing — it only withdrew an earlier agreement — so it could not
be shown, and nothing re-opened it when the model changed.

## Decision

- `.tracelean/judges.json` lists who may judge and whom each delegates to:
  `{"people": [{"name": "ana", "delegatesTo": ["claude-review"]}]}`.
- `--by` blank is refused. A listed person judges in their own name. Anyone
  else needs `--delegated-by <person>`, a listed person who delegates to them;
  the record keeps both (`judgedBy`, `delegatedBy`). Without a judges file a
  name is taken at its word and the tool says the verdict is unattributed to a
  listed person.
- Every verdict is recorded in the clause's one judgement slot, with its note
  and the requirement and model hashes: `agrees` at L2, `drift` and
  `unmodelable` at L1. Levels are a maximum per bond with an L1 floor, so an L1
  record raises nothing; it is shown, replaces an earlier agreement, and goes
  stale like any record.

## Consequences

A delegated verdict counts as L2, as a person's does: delegation is the person
saying the agent judges for them. The view shows `L2, delegated by <person>`
so the difference stays visible. `delegatedBy` and `note` are optional and
omitted when absent, so older records load and keep their bytes.
