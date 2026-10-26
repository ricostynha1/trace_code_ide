---
id: REQ-DIFF
title: What a change did to the evidence
refines: [ARCH-HONEST, REQ-EVID, REQ-LOCK]
status: approved
decomposition: open
clauses:
  per_clause: The system shall report the difference between two states of a project clause by clause, naming each clause that was added or removed.
  text_shown: When a clause's text changed, the system shall show its text before and after.
  marks_moved: The system shall report each mark a clause gained and each mark it lost.
  links_moved: The system shall report each annotation of a clause that was added or removed, and each whose annotated body changed, by role and symbol.
  assumptions_moved: The system shall report each axiom an accepted proof of the clause newly rests on and each it no longer rests on.
  unchanged_omitted: The system shall omit a clause none of whose text, marks, annotations or assumptions changed.
  losses_first: The system shall list a clause that was removed or lost a mark before any other change.
  base_from_history: The system shall read the earlier state from a named revision of version control, without checking it out.
---

# What a change did to the evidence

A reviewer of a change made by an agent does not want the code diff first; it
wants to know which promises the change touched and what happened to the
evidence for each. The diff of two states of the project answers that clause by
clause: a clause reworded (and how), a mark gained or lost, a model, test or
proof added, removed or changed underneath its annotation, and an axiom a proof
now leans on. A clause the change did not touch is not listed, so the length of
the report is the size of the change, not of the project.

A loss comes first. A clause removed, or one that held `P` and no longer does,
is the part of a change most likely to be a mistake made quietly, and the part a
reviewer must not have to scroll to find.

The earlier state is read from the committed lock (`.tracelean/trace.lock`),
the requirement files and the proof verdicts (`.tracelean/proofs.json`) at the
named revision through `git show`, so comparing against `main` or `HEAD~1`
needs no second checkout.

```
tracelean-trace . --diff main
```
