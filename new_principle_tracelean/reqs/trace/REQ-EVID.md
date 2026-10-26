---
id: REQ-EVID
title: Evidence algebra
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  marks: The system shall record a clause's evidence as four marks, annotated, judged, differentially tested and proved, each held or not independently, none implying another.
  bonds_separate: The system shall grade the requirement-to-model, model-to-implementation and model-property bonds independently.
  mark_per_bond: The system shall give the judged mark only from a judgement of the requirement-to-model bond, the tested mark only from a differential run of the model-to-implementation bond, and the proved mark only from a proof of the model-property bond.
  absent_is_unset: If a bond has no evidence record, then the system shall leave its mark unset.
  monotone: When an evidence record is added, the system shall never clear a mark.
  marks_written: The system shall write marks in fixed positions, A J T P, with a dash for each mark not held, so that a gap names what is missing.
  structural_not_applicable: Where a clause is checked only by reading the tree, the system shall give it neither the tested nor the proved mark, write both as not applicable rather than missing, and leave both marks of anything it is aggregated with as they are.
  judgement_caps: The system shall let a judgement contribute at most the second level, and shall never let it promote a link to tested or proved.
  record_reproducible: The system shall require an evidence record to carry what is needed to reproduce it, and shall not let one be expressed without it.
---

# Evidence algebra

An annotation is a claim; evidence is what backs it. The marks are independent
because the work is: a model can be proved and differentially tested before
anybody judged that it says what the clause says, and a single level would read
that as annotated only, while an average would invent a figure. `A-TP` says
exactly what is held and what is missing.

A record still carries a level (a judgement the second, a differential run the
third, a proof the fourth), which says what kind of record it is and caps what
it can set; it is not summed or ranked across bonds. Evidence accumulating must
never make the display worse, though [REQ-STALE](REQ-STALE.md) may clear a mark,
and must.
