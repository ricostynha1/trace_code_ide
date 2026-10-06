---
id: ARCH-NO-DRIVING
title: TraceLean observes agents and never drives them
status: approved
decomposition: open
clauses:
  no_launch: TraceLean shall not launch, prompt, steer or terminate a coding agent.
  no_model_call: TraceLean shall make no call to a language model, and shall contain no provider credentials, spend accounting or token budgeting.
  observation_only: An external tool's effects shall reach the editor only by observing a workspace it wrote, never by a protocol that instructs it.
  user_runs_it: The external tool shall be started by the user, in their own shell, with TraceLean uninvolved in its invocation.
  export_not_call: Where a model's opinion is useful, TraceLean shall export a prompt for the user to carry, and shall not send it.
---

# TraceLean observes agents and never drives them

Stage-0 TraceLean contained an agent runtime, tool registry and selector,
provider clients, context retention, a cost model, an MCP host and client, and a
protocol for launching agent subprocesses. It was the largest subsystem in the
tree and all of it existed so the editor could drive an agent.

It is replaced by the inverse: the user runs an external tool themselves in a
sandboxed copy, and the editor watches that copy.

This is a requirement rather than a historical note because boundaries erode one
reasonable exception at a time — a small provider client for the judge, a button
that starts the agent, a token count in the status bar. Each is defensible alone;
together they rebuild what was removed. As clauses, they are violations.

See [02-scope.md](../../docs/02-scope.md).
