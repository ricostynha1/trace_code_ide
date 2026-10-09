---
id: REQ-DRT-PROTO
title: The conformance protocol
refines: [REQ-DRT, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  case_one_line: A case shall be written as one JSON object on one line of stdin.
  reply_one_line: A reply shall be read as one JSON object on one line of stdout.
  reply_exclusive: A reply shall carry exactly one of an output or an error.
  case_echoed: A reply shall echo the case number it answers.
  case_names_op: A case shall name, in its op, the entry point it exercises.
  ops_unique: An op shall be unique across a project.
  runner_shared: One runner process shall serve every binding of its language in a project.
  failure_named: A runner that cannot start, times out, dies or emits a non-reply shall be distinguished from a runner that answered.
---

# The conformance protocol

```text
→ {"case":41,"op":"REQ-EVID.weakest_link","input":{"bonds":["L4","L1"]}}
← {"case":41,"output":"L1"}
← {"case":42,"error":"index out of bounds"}
```

This is the entire coupling between TraceLean and the code it checks. Keeping it
this small is what lets a runner be a shipped script for one language and a
generated compiled binary for another.

`ops_unique` carries a stage-0 lesson: ops defaulted to `"default"`, so a shared
runner's dispatch had several arms with the same name, the first won, and every
other requirement was silently answered by the wrong function.

Writing a case and reading a reply are separate clauses (`case_one_line`,
`reply_one_line`) because they are separate functions that can each be wrong
alone; the same for naming the op (`case_names_op`) and the op being unique
(`ops_unique`). `failure_named` is decided per asked case from what happened —
no start, no line within the timeout, a broken or closed pipe, or a line — and
only a line read as an answer is one.
