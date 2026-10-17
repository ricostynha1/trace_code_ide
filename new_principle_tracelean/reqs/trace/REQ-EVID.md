---
id: REQ-EVID
title: Evidence algebra
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  marks: A clause's evidence shall be four marks — annotated, judged, differentially tested and proved — each held or not independently, none implying another.
  bonds_separate: The requirement-to-model, model-to-implementation and model-property bonds shall be graded independently.
  mark_per_bond: The judged mark shall be held from a judgement of the requirement-to-model bond, the tested mark from a differential run of the model-to-implementation bond, and the proved mark from a proof of the model-property bond, and from nothing else.
  absent_is_unset: A bond with no evidence record shall leave its mark unset.
  monotone: Adding an evidence record shall never clear a mark.
  marks_written: Marks shall be written in fixed positions, A J T P, with a dash for each mark not held, so that a gap names what is missing.
  structural_not_applicable: A clause checked only by reading the tree shall hold neither the tested nor the proved mark, shall write both as not applicable rather than missing, and shall leave both marks of anything it is aggregated with as they are.
  judgement_caps: A judgement shall contribute at most the second level and shall never promote a link to tested or proved.
  record_reproducible: An evidence record shall carry what is needed to reproduce it, and shall not be expressible without it.
---

# Evidence algebra

An annotation is a claim; evidence is what backs it, and saying which evidence
is the difference between a document and an assurance.

The marks are independent because the work is. A model can be proved and
differentially tested before anybody has judged that it says what the clause
says; a single level on a ladder would read that as "annotated only", hiding two
of the three things done, and an average would invent a figure nothing
established. `AJTP` with gaps (`A-TP`) says exactly what is held and what is
missing. `bonds_separate` is what makes the marks expressible: each comes from
its own bond.

A record still carries a level: a judgement is the second, a differential run
the third, a proof the fourth. The level says what kind of record it is and
caps what it can set (`judgement_caps`); it is not summed or ranked across bonds.

`monotone` keeps the algebra sane under incremental work — evidence accumulating
must never make the display worse. It constrains *adding* a record;
[REQ-STALE](REQ-STALE.md) may clear a mark, and must.
