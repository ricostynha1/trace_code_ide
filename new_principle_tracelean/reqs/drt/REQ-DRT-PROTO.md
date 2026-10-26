---
id: REQ-DRT-PROTO
title: The conformance protocol
refines: [REQ-DRT, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  case_one_line: The system shall write a case as one JSON object on one line of stdin.
  reply_one_line: The system shall read a reply as one JSON object on one line of stdout.
  reply_exclusive: The system shall require a reply to carry exactly one of an output or an error.
  case_echoed: The system shall require a reply to echo the case number it answers.
  case_names_op: The system shall name, in each case's op, the entry point the case exercises.
  ops_unique: The system shall keep every op unique across a project.
  runner_shared: The system shall serve every binding of one language in a project with one runner process.
  failure_named: If a runner cannot start, times out, dies or emits a non-reply, then the system shall distinguish it from a runner that answered.
---

# The conformance protocol

```text
→ {"case":41,"op":"REQ-EVID.weakest_link","input":{"bonds":["L4","L1"]}}
← {"case":41,"output":"L1"}
← {"case":42,"error":"index out of bounds"}
```

This is the whole coupling between the system and the code it checks, small
enough that a runner can be a shipped script for one language and a generated
binary for another. An op shared by two bindings lets the first arm of a shared
runner's dispatch answer for both. `failure_named` is decided per asked case
from what happened: no start, no line within the timeout, a broken or closed
pipe, or a line; only a line read as an answer is one.
