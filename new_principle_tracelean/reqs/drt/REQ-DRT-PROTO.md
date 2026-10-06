---
id: REQ-DRT-PROTO
title: The conformance protocol
refines: [REQ-DRT, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  line_delimited: A case shall be one JSON object on one line of stdin, and a reply one JSON object on one line of stdout.
  reply_exclusive: A reply shall carry exactly one of an output or an error.
  case_echoed: A reply shall echo the case number it answers.
  op_dispatch: A case shall name the entry point it exercises, and an op shall be unique across a project.
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

`op_dispatch` carries a stage-0 lesson: ops defaulted to `"default"`, so a shared
runner's dispatch had several arms with the same name, the first won, and every
other requirement was silently answered by the wrong function.
