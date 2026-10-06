---
id: REQ-PROV
title: Provenance of a position
refines: [REQ-CMD, REQ-UNDO]
status: approved
decomposition: complete
clauses:
  position_question: The system shall answer which recorded edit wrote the text at a given position.
  exact_not_heuristic: The answer shall be computed by inverting the recorded edits and shall not be estimated.
  mapping_before: A position before an edit's span shall map back unchanged.
  mapping_after: A position after an edit's span shall map back shifted by the edit's length change.
  mapping_inside: A position inside an edit's replacement shall terminate the search at that edit.
  base_is_honest: Text that arrived with the file rather than being written shall be attributed to the file's base state.
---

# Provenance of a position

History as one record of everything is useful only if it answers a question about
a *place*. Put the cursor on a line, ask who wrote it, land on the moment it was
written.

`exact_not_heuristic`: walking one replacement backwards maps a post-edit
position to a pre-edit one by three cases, and the *inside* case is the answer.
No replay, no similarity scoring, no dependence on current buffer contents.

`base_is_honest` names the one thing the method cannot see — text that arrived
with the file was never written by a recorded command, and "this is how the file
arrived" is the true answer rather than a plausible wrong one.
