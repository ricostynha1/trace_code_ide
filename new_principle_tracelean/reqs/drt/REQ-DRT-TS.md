---
id: REQ-DRT-TS
title: TypeScript conformance runner
refines: [REQ-DRT-PROTO, REQ-DRT-BIND, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  generated: The system shall generate the runner, which is never written or maintained by hand.
  project_untouched: When generating the runner, the system shall write only under the cache directory and shall never modify a file the project owns.
  runs_the_shipped_source: The system shall have the runner call the implementation's own source file, without a compilation step that could differ from what ships.
  params_from_source: The system shall take argument order from the implementation's own signature rather than from the binding.
  failure_is_an_answer: If an implementation throws, then the system shall report an error for that case rather than end the run.
  second_implementation_same_oracle: The system shall compare a second implementation of a clause against the model, never against the first implementation.
---

# TypeScript conformance runner

The web frontend is a second implementation of the functions that read a
buffer: splitting its text into lines and working out what can be done at a
position. Node strips the types and runs the file, so no build output can differ
from what the browser loads; the price is that a binding that does not fit is a
runtime error, reported as that case's error and never as a divergence about
values. Two implementations agree perfectly when both are wrong, so each is
compared against the Lean model.
