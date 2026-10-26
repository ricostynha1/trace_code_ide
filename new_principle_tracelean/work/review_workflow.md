# Review workflow — a proposal to think on

Not implemented. This describes what a review workflow on top of TraceLean
would do, how it would be used, and what it would need from the tool. Nothing
here is a requirement yet; under the method, whatever is kept becomes clauses
(most naturally in a new `REQ-REVIEW`) before any code.

## The problem it solves

An agent can change requirements, models, code, tests and proofs in one go, and
re-earn every mark itself. The lock then says the project is fine. A person
reviewing the change today has two bad options: trust the summary, or read the
whole diff, including `.tracelean/` JSON. Neither scales, and the second is what
TraceLean exists to avoid.

The workflow gives the reviewer **one change at a time, expressed as a change in
evidence**, with the risky events first and the routine ones collapsed.

## The flow

```
agent change ──► evidence diff ──► reviewer spot-checks cards ──► blocking event?
      ▲                                                       │ no        │ yes
      └──────── evidence weak: back to the agent              ▼           ▼
                                                            merge ◄── owner signs off
```

1. **Agent change.** The agent works as today and ends with `tracelean-trace .
   --stale` at 0 to redo.
2. **Evidence diff.** `tracelean-trace . --diff <base>` compares the lock and the
   requirement files at `<base>` with the working tree and writes the diff (as
   text, JSON and a section of the HTML report).
3. **Spot-check.** The reviewer opens the cards of the clauses the diff lists and
   reads what changed in each: clause text, theorem statements, bindings,
   verdicts.
4. **Blocking event?** If the diff holds any blocking event (below), the
   requirement's owner must sign off before merge. Otherwise the reviewer's
   approval is enough.
5. **Back to the agent.** A reviewer who finds the evidence weak writes a comment
   on the clause; the comment becomes a finding the agent must clear.

## What the evidence diff shows

Per clause, only what changed:

| Section | Content |
|---|---|
| Clause text | Old and new wording, word-level diff |
| Marks | `AJTP` before → after, each lost mark named with its reason |
| Statements | Theorem and spec statements that changed (proof bodies are not shown) |
| Links | `@models` / `@implements` / `@tests` / `@drt` / `@proves` added or removed |
| Verdicts | Judgements added, changed or invalidated, and who made them |
| Findings | New and cleared findings |
| Coverage | Change in the clause's line coverage |

Ordered: blocking events first, then lost marks, then everything else; clauses
with no change are not listed.

### Blocking events

- a clause added, removed or reworded in an `approved` requirement;
- a clause newly marked `@structural`, or a structural check removed;
- a mark lost (`J`, `T` or `P`);
- a new drift verdict;
- a theorem or spec statement changed (the proof changing is not blocking);
- a declared axiom (`@external` / `@assumes`) added or changed.

## Sign-off as evidence

A sign-off is a record in the lock like any other:

```json
{ "kind": "signoff", "req": "REQ-SHOW", "clause": "producers_are_total",
  "by": "ricostynha", "clauseHash": "…", "evidence": ["…", "…"], "at": "<commit>" }
```

It names the clause hash and the evidence hashes the person saw, so it goes
stale under the same rules as a run: an approval never outlives what was
approved. `owner:` in a requirement's frontmatter says who may sign it, the way
CODEOWNERS routes code review.

## How it would be used

```bash
# the agent, at the end of its change
tracelean-trace . --stale            # 0 to redo
tracelean-trace . --diff main        # what this change did to the evidence

# the reviewer
tracelean-trace . --diff main --report out/   # cards + diff as HTML
tracelean-trace . --signoff REQ-SHOW.producers_are_total --by ricostynha
tracelean-trace . --diff main --blocking      # exit 1 while anything blocking is unsigned
```

In CI the last command is the gate: a pull request posts the diff as a comment
and fails while a blocking event lacks its sign-off.

## Sampling agent verdicts

Agents may judge (that is decided: provenance is enough to trust the record).
To know *how far* agent verdicts can be trusted, each release a small sample of
agent-made verdicts is drawn — seeded by the commit, so the draw is
reproducible — and a person confirms or overturns each. The agreement rate is
reported beside the `J` count. It costs a few minutes per release and gives a
measured, not assumed, trust in delegated judging.

## Open questions

- Is the owner per requirement enough, or per clause?
- Should a reviewer's comment be a finding (blocks `--stale` at 0) or only a note?
- Does the diff compare against a branch, a tag, or the last signed-off lock?
- How large a sample is worth the reviewer's time — five verdicts, or a share?
