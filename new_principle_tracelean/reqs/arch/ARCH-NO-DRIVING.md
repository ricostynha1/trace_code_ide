---
id: ARCH-NO-DRIVING
title: TraceLean observes agents and never drives them
status: approved
decomposition: open
clauses:
  no_launch: The system shall not launch, prompt, steer or terminate a coding agent.
  no_model_call: The system shall make no call to a language model, and shall contain no provider credentials, spend accounting or token budgeting.
  observation_only: The system shall take an external tool's effects only by observing a workspace the tool wrote, never by a protocol that instructs it.
  user_runs_it: The system shall leave the starting of an external tool to the user, in their own shell, and take no part in its invocation.
  export_not_call: Where a model's opinion is useful, the system shall export a prompt for the user to carry, and shall not send it.
---

# TraceLean observes agents and never drives them

The user runs an external tool themselves, in a sandboxed copy, and the editor
watches that copy. Each clause names one way of driving an agent, because each
one alone looks like a reasonable exception. See
[02-scope.md](../../docs/02-scope.md).
