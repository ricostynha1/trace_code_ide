---
id: REQ-JUDGE
title: The human judge
refines: [ARCH-NO-DRIVING, REQ-EVID, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  human_decides: A judgement shall be recorded as the decision of an identified person.
  no_call: Recording a judgement shall involve no call to any external service.
  prompt_exported: The system shall export a judging prompt for the user to carry to a tool of their choosing.
  advice_is_not_evidence: Anything returned by such a tool shall enter the record only as the human's own decision.
  caps_at_judgement: A judgement shall write evidence at the judgement level and shall never promote a link further.
  proposal_not_mutation: A judgement that a clause cannot be modelled shall produce a proposal and shall never modify the project.
  invalidated_by_change: A judgement shall be invalidated when either the requirement text or the model changes.
  divergence_presented: A judgement shall be made with any known divergence between model and implementation presented alongside.
---

# The human judge

Nothing binds English to a formal model by execution — there is no oracle for
prose. Every other bond is checked by running something; this one cannot be,
which is why it is graded below them.

So the judge is a person, and the system supplies the material: the clause, the
model, and whatever differential testing found, plus a prompt they can carry to a
tool if they want a second opinion. What comes back is advice; the decision is
theirs and recorded as theirs.

`caps_at_judgement` survives from the design where the judge was a machine,
because it was never about the judge being a machine. A reading is not an
execution, and a judgement that could promote a link past its method would let
the most fallible bond produce the most confident output.

See [ADR-0003](../../docs/decisions/ADR-0003-human-judge.md).
