---
id: REQ-EVID
title: Evidence algebra
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  ladder: Evidence levels shall be totally ordered, with annotation below judgement below differential testing below proof.
  bonds_separate: The requirement-to-model, model-to-implementation and model-property bonds shall be graded independently.
  absent_is_lowest: A bond with no evidence record shall contribute the lowest level.
  weakest_link: A link's assurance shall be the minimum over its bonds, and shall never be an average.
  monotone: Adding an evidence record shall never lower an assurance.
  chain_rendered: An assurance shall be presentable as a per-bond chain, not only as a single value.
  judgement_caps: A judgement shall contribute at most the second level and shall never promote a link to tested or proved.
  record_reproducible: An evidence record shall carry what is needed to reproduce it, and shall not be expressible without it.
---

# Evidence algebra

An annotation is a claim; evidence is what backs it, and grading it is the
difference between a document and an assurance.

`weakest_link` is the most important rule here. A proved model whose
implementation nobody bound to it is not three-quarters assured — it must read
`L4 model · L1 code`. Collapsed into one averaged number, the number has no
referent and a team acts on it anyway. `bonds_separate` is what makes the rule
expressible: without three independent bonds there is nothing to take a minimum
over.

`monotone` keeps the algebra sane under incremental work — evidence accumulating
must never make the display worse. It constrains *adding* a record;
[REQ-STALE](REQ-STALE.md) may lower an assurance, and must.
