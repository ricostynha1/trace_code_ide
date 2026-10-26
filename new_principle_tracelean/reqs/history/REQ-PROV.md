---
id: REQ-PROV
title: Provenance of a position
refines: [REQ-CMD, REQ-UNDO]
status: approved
decomposition: complete
clauses:
  position_question: When asked about a position, the system shall answer which recorded edit wrote the text there.
  exact_not_heuristic: The system shall compute the answer by inverting the recorded edits, and shall not estimate it.
  mapping_before: The system shall map a position before an edit's span back unchanged.
  mapping_after: The system shall map a position after an edit's span back shifted by the edit's length change.
  mapping_inside: If a position is inside an edit's replacement, then the system shall end the search at that edit.
  base_is_honest: The system shall attribute text that arrived with the file, rather than being written, to the file's base state.
---

# Provenance of a position

Put the cursor on a line, ask who wrote it, and land on the moment it was
written. Walking one replacement backwards maps a position by three cases, and
the inside case is the answer: no replay, no similarity scoring, no dependence
on the current buffer. Text that arrived with the file was never written by a
recorded command, and saying so is the true answer.
