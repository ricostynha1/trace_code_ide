---
id: ARCH-SELFHOST
title: TraceLean is a proper project by its own definition
status: approved
decomposition: open
clauses:
  every_feature_traced: The system shall link every one of its features to a requirement clause by an annotation.
  no_convention: The system shall create a link only where somebody wrote one, and shall never infer one from a filename, a directory or a naming convention.
  fixpoint: The system shall produce for its own tree the same index the bootstrap tool produces for it.
  idempotent: When the system is run on its own output a second time, the system shall change nothing.
  docs_checked: The system shall not let a document reference a requirement or symbol that does not exist.
---

# TraceLean is a proper project by its own definition

A traceability tool errs about identity, staleness, aggregation and evidence,
and its own tree exercises exactly that. `fixpoint` and `idempotent` are the
acceptance criteria: two readings of one repository that disagree mean one of
them is wrong, and a second run that changes the output means the first was not
a function of its input.
