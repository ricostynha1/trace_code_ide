# Ideas

Things worth doing that are not requirements yet. Each says what is wrong now,
what would replace it, and what would have to be true for the change to be an
improvement rather than a rearrangement.

Nothing here is a commitment. An entry that becomes work becomes a requirement
with clauses first, like everything else. Done ideas are removed; git keeps
them.

## 1. A performance register

**Now.** The suite's cost is known only by watching it. The one shared runner
per side took a full differential run from about 1364 s to 86 s; both numbers
were measured by hand, once, and nothing would report either moving.

**Change.** A register in `.tracelean/` recording, per op, the seed, the case
count, and the wall-clock of the run that produced the evidence — written by the
same two-phase path that writes the L3 record, since that path already knows all
four. A report reads it and names what got slower.

**True if.** The numbers stay out of the evidence. A duration is not a claim
about a requirement: it cannot raise a level, and a record that carried one
would invite exactly the reading the ladder exists to prevent. The register
sits beside the evidence and is compared against itself over time, never
against a threshold that would make a slow machine look like a failed proof.
The stored value must therefore not enter the lockfile's bytes, or two machines
would produce two lockfiles for the same work.

## 2. Shrinking that reports what it removed

**Now.** `RunOptions.shrink_rounds` is 100 everywhere and the shrunk input is
what a divergence reports. Nothing says how much was removed, so a shrinker that
quietly stopped working would still produce a plausible-looking failure.

**Change.** Report the original input beside the shrunk one and the number of
rounds that made progress.

## 3. The judging queue as a report

**Now.** `--stale` lists judgements whose inputs changed, but not clauses that
were never judged. Finding those meant a one-off script over the lock in the
2026-10-09 review.

**Change.** `--stale` (or a sibling) also lists every clause whose
requirement↔model bond is at L1 with a model present — the clauses where a
person's reading is the only thing that would raise the level — ordered by how
many implementations depend on them.

## 4. A report that finds two models of one concept

**Now.** Eleven names once meant two things each, found by accident when
somebody imported every module at once and the compiler objected. Nothing looks
for it. The opposite case is worse and entirely invisible — two models of *the
same* concept under two different names, which no compiler will ever complain
about. `Record` in `Evidence` and `Staleness` was one, and it was only noticed
because its name happened to collide.

This is the failure mode a coding agent is worst at. An agent asked to model a
new clause reads that clause, writes a function, and does not know the tree
already contains one. Every incentive it has points at adding: adding compiles,
adding passes, and the duplicate reads as thorough. Nothing in the method
currently says *stop, this exists*.

**Change.** A report that names candidate duplicates, and a rule that a new
model must answer it. Three signals, cheap to compute and none of them
conclusive alone:

1. **Same shape.** Two model functions whose argument and result types are
   structurally equal, modulo field names. Read from the declarations, not from
   the text.
2. **Same clauses.** Two functions whose `@models` sets overlap, or whose
   requirements refine a common parent. Two models under one clause is either a
   decomposition somebody meant or a duplicate nobody noticed, and the report
   does not have to know which — it has to ask.
3. **Same behaviour.** The one this project can actually settle. Two functions
   with the same shape already have a schema and a generator between them: run
   both over generated inputs and see whether they ever disagree. Two models
   that never disagree over a met coverage floor are one model, and the report
   can say so with the same evidence a binding produces.

Signal 3 is the point, and it is only available because the machinery for it
already exists. Duplication detection elsewhere is a similarity score over text;
here it can be a differential run between two things that both claim to be
specifications, judged by the same rule that judges everything else.

**For an agent.** The rule to add to `skills/04-models-and-bindings.md` is a
step before writing: run the report, and if it names a candidate, either reuse
it or say in the new model's doc comment why the two are different. The second
half matters as much as the first — "these look alike and here is why they are
not" is a sentence a reviewer can check, and its absence is what let eleven
names drift into meaning two things each.

**True if.** The report never deletes anything and never merges anything. A
machine that decided two models were one would be making the requirement↔model
judgement, which is the bond the method reserves for a person. It reports, and a
person or an agent answers in prose that lives next to the code.

**Cost.** Signal 3 is a differential run per candidate pair, which is the
expensive kind. Worth gating behind a flag and running when a model is added
rather than on every check. Signal 2 is the cheap half of the action plan's
§3 (catching replication).
