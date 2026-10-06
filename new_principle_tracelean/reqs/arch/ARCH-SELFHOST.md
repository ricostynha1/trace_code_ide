---
id: ARCH-SELFHOST
title: TraceLean is a proper project by its own definition
status: approved
decomposition: open
clauses:
  every_feature_traced: Every feature of this system shall be linked to a requirement clause by an annotation.
  no_convention: A link shall exist only where somebody wrote one, and shall never be inferred from a filename, a directory or a naming convention.
  fixpoint: The index this system produces for its own tree shall equal the index the bootstrap tool produces for it.
  idempotent: Running the system on its own output a second time shall change nothing.
  docs_checked: A document shall not be able to reference a requirement or symbol that does not exist.
---

# TraceLean is a proper project by its own definition

A methodology that has not survived being applied to something as awkward as an
IDE has been shown to be describable, not usable.

Self-application is also the sharpest available test. A traceability tool errs
about identity, staleness, aggregation and what counts as evidence — and a traced
project is exactly the input that exercises that class.

`fixpoint` and `idempotent` are the acceptance criteria. Neither proves the kernel
correct; both catch the specific way it would be wrong. Two readings of one
repository that disagree mean one is wrong about something both can see, and a
second run that changes the output means the first was not a function of its
input.
