---
id: REQ-TRANSCRIPT
title: Reading an external tool's transcript
refines: [REQ-OBS, ARCH-NO-DRIVING]
status: draft
decomposition: complete
clauses:
  read_only: The transcript shall be read and never written.
  partial_line_held: A record that has not finished being written shall be held until complete rather than parsed.
  unknown_preserved: A record whose shape is not recognised shall be reported as unrecognised and shall not abort the read.
  no_interpretation: The transcript shall be used to show what the tool reported doing, and shall not be used to decide what the editor does.
  absent_is_fine: A tool that writes no transcript shall be fully supported, with observation of the workspace alone.
  tool_format_read: A transcript in a known tool's own format — Claude Code's content blocks — shall be read into the conversation it records, a row for what was said, thought, called and returned, a record still being written being left for the next read.
---

# Reading an external tool's transcript

Some tools write their own session record; reading it shows what the tool said it
was doing alongside what it changed.

`no_interpretation` and `absent_is_fine` keep this a convenience. The workspace
diff is the truth about what happened; the transcript is the tool's account. A
system acting on the account would be taking instruction from the tool, which
`ARCH-NO-DRIVING` forbids.

`partial_line_held` is the tailing bug everybody writes once: a record read while
still being appended parses as malformed, and treating that as an error makes the
feature fail exactly when the tool is most active.
