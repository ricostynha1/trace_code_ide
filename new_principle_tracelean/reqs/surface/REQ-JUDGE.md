---
id: REQ-JUDGE
title: The human judge
refines: [ARCH-NO-DRIVING, REQ-EVID, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  human_decides: The system shall record a judgement as the decision of an identified person, or of a delegate whom a person listed as a judge of the project authorised, naming both, and shall refuse a judgement naming nobody or a delegate no listed person authorised.
  no_call: When recording a judgement, the system shall call no external service.
  prompt_exported: The system shall export a judging prompt for the user to carry to a tool of their choosing.
  advice_is_not_evidence: The system shall let anything returned by such a tool enter the record only as the human's own decision.
  caps_at_judgement: When recording a judgement, the system shall write evidence at the judgement level and shall never promote a link further.
  proposal_not_mutation: If a clause is judged unmodelable, then the system shall produce a proposal and shall never modify the project.
  invalidated_by_change: When either the requirement text or the model changes, the system shall invalidate the judgement.
  divergence_presented: When a judgement is made, the system shall present any known divergence between model and implementation alongside.
  structural_judged: If nothing models a clause and a test checks it by reading the tree, then the system shall judge the clause against that test, show it as such, and invalidate the judgement when the test changes.
  drift_recorded: If a judgement finds drift or unmodelable, then the system shall record it at the lowest level, with its note and the requirement and model it was about, in place of any earlier judgement of the same clause.
  judgement_shown: The system shall show, for each clause of a requirement, its judgement, a drift or unmodelable one with its note, and a delegated one with the person who delegated it.
---

# The human judge

No execution binds English to a formal model, so this bond is judged by a
person and graded below the bonds checked by running something. The system
supplies the clause, the model, whatever differential testing found, and a
prompt the person may carry to a tool; what comes back is advice, and the
decision is recorded as theirs. A reading is not an execution, so a judgement
never promotes a link past its level.

A person may delegate judging to an agent in `.tracelean/judges.json`; the
verdict then counts in that person's name and both names are kept. See
[ADR-0003](../../docs/decisions/ADR-0003-human-judge.md) and
[ADR-0015](../../docs/decisions/ADR-0015-delegated-judges-and-recorded-drift.md).
