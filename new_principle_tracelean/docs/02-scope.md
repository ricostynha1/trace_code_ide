---
describes: [ARCH-NO-DRIVING, REQ-OBS, REQ-MYTH]
described_hash:
  ARCH-NO-DRIVING: 5edd72e90d4b4ed1
  REQ-OBS: ddbaa15e6f6a11ec
  REQ-MYTH: 0120c37ad5d78c7b
---

# Scope: what this port drops

## Dropped — TraceLean driving an agent

| Dropped | Was at |
|---|---|
| Agent runtime and loop | `core/src/agent/` |
| Tool executor, registry, selector, dialects | `core/src/ai/tool_*.rs` |
| Provider clients, streaming | `core/src/ai/{openrouter,bedrock,mock,streaming}.rs` |
| Context retention, cost model, TTL tracking | `core/src/ai/{retention,cost_trimmed_summary_model,ttl_tracking,trimmed_rules_table}.rs` |
| MCP host and client | `core/src/ai/mcp_*.rs` |
| ACP client, built-in agent, orchestrator | `core/src/acp/` |
| Agent shell sandbox | `core/src/ai/shell_sandbox.rs` |
| Surgical edit strategies | `core/src/surgical_edit/` |

Most of `ai/`, all of `acp/`, all of `surgical_edit/` — by line count the largest
subsystem in the old tree.

## Kept — TraceLean observing an agent

`core/src/sandbox/`. The user runs an external tool themselves, in a plain shell,
inside a bubblewrap namespace bound to a reflink copy of the project. TraceLean
watches that copy and mirrors into buffers, file tree and undo tree the paths
whose content differs from what the copy started with (`sessions/<id>/base.json`)
— not every difference from the real tree, which also holds what the project
changed since. It never launches, prompts or drives the tool; it briefs it, with
a `CLAUDE.md` above the copy pointing at the skills.

The editor stops needing to know what a model is, what a token costs, how a tool
schema is shaped or how to keep a cache warm. An agent becomes a process that
edits files, watched by an editor that already understands edits.

Stated as `ARCH-NO-DRIVING` so that reintroducing agent-driving machinery is a
visible violation rather than a change of mind.

## Changed — the judge is a human

The old judge sent requirement and model to an LLM and parsed a verdict. Here a
person decides, with the requirement beside the model and beside any divergence.
TraceLean exports a **copyable prompt** for the user to carry to whatever tool
they like; the answer they bring back is entered as their own decision.

Evidence still sits at L2, because L2 always meant "a judgement, not a check".
The witness-execution machinery goes with the LLM — it existed to catch a model
confabulating about Lean. See
[ADR-0003](decisions/ADR-0003-human-judge.md).

## Kept and promoted — Myth

The modal keymap is retained despite working poorly, because data-driven bindings
are what make the interface configurable. Its problems are the reason it is
promoted: a mode machine is a pure total function, so "the keymap swallows a key"
is not a bug to chase but a missing totality theorem. See `REQ-MYTH`.

The keymap it ships is `assets/keymap.json`, loaded and validated by the same
`keymap::load` a user's own edit goes through: a keymap that does not hold
together is refused when it is read, not on the key that would have failed.
