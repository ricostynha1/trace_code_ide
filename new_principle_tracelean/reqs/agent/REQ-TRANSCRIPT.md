---
id: REQ-TRANSCRIPT
title: Reading an external tool's transcript
refines: [REQ-OBS, ARCH-NO-DRIVING]
status: draft
decomposition: complete
clauses:
  read_only: The system shall read the transcript and never write it.
  partial_line_held: If a record has not finished being written, then the system shall hold it until complete rather than parse it.
  unknown_preserved: If a record's shape is not recognised, then the system shall report it as unrecognised and shall not abort the read.
  no_interpretation: The system shall use the transcript to show what the tool reported doing, and shall not use it to decide what the editor does.
  absent_is_fine: Where a tool writes no transcript, the system shall fully support it with observation of the workspace alone.
  tool_format_read: When a transcript is in a known tool's own format — Claude Code's content blocks — the system shall read it into the conversation it records, a row for what was said, thought, called and returned, leaving a record still being written for the next read.
---

# Reading an external tool's transcript

Some tools write their own session record, which shows what the tool said it
was doing beside what it changed. The workspace diff is the truth and the
transcript is the tool's account; acting on the account would be taking
instruction from the tool (`ARCH-NO-DRIVING`). A record read while it is still
being appended parses as malformed, and that happens most when the tool is
busiest.
