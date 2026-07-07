## Agent hardness architecture 

# T0: Check if the implementation of MCP is complient !!
Model Context Protocol (MCP) governs vertical interactions between an agent and its tools, acting as a "USB-C" for securely connecting agents to internal tools and databases.  It has become the default interface for tool discovery, with major harnesses like Claude Code, Cline, and CrewAI shipping native MCP support. 

## Suggested Harness Architecture for TraceLean

My recommendation is to keep TraceLean as the owner of state, permissions, command logging, and persistence, and treat any external agent SDK as a replaceable runtime adapter. That means the harness stays in Rust, while the agent runtime can be swapped later if needed.

### 1. Core Principle

- TraceLean owns the source of truth: `AppState`, command log, trace graph, undo tree, and file-backed persistence.
- Agents never mutate files directly.
- Every agent output is converted into reversible Commands or rejected before it reaches state.
- MCP should remain the tool protocol layer.

### 2. Proposed Layering

```text
Editor client
	-> Harness API
		-> Planner / Router
			-> Specialist Subagents
				-> Tool Gateway (read/search/trace/write/emit_command)
					-> AppState + Command Log + Trace Graph
```

The important separation is:

- Control plane: chooses what to do, which agent to spawn, and how much budget to spend.
- Data plane: reads project state, produces diffs, validates them, and commits commands.

### 3. Harness Modules

#### 3.1 Agent Router

Responsible for classifying the task and choosing the cheapest valid path.

- Requirement drafting
- Lean formalization
- Implementation
- Repair / refactor
- Verification / explanation

The router should default to a small model and only escalate to a stronger model if the task needs it.

#### 3.2 Context Builder

Builds the minimum useful context from:

- requirement text
- linked Lean spec
- trace graph links
- file symbols and call graph
- recent command history
- recent AI interaction summaries

This should be hash-aware so unchanged artifacts are not re-read or re-sent.

#### 3.3 Subagent Manager

Creates scoped workers for narrowly defined jobs.

Good subagent types:

- Planner
- Requirement writer
- Lean spec writer
- Implementation writer
- Repair writer
- Verifier / reviewer

Subagents should have explicit permissions and a bounded context window. A subagent should not get more access than it needs.

#### 3.4 Tool Gateway

This is the only place where agents touch the project.

- read_file
- search_files
- get_symbols
- query_trace_graph
- query_code_element
- list_requirements
- emit_command
- write_file / str_replace / insert_lines
- run_shell only when explicitly allowed

This gateway should enforce:

- path permissions
- shell permissions
- max tool count per turn
- max time per tool
- structured logging of every call

#### 3.5 Memory Manager

Use three memory layers:

- Ephemeral turn memory: the current subagent step, not persisted.
- Session memory: compact summaries of prior decisions, active goals, and unresolved questions.
- Artifact memory: requirement/spec/code/test pointers derived from the trace graph and command log.

Avoid storing raw conversation transcripts as the main memory model. Store summaries and artifact references instead. That is cheaper, smaller, and easier to invalidate.

#### 3.6 Cache Manager

Keep caches disposable and derivable from source.

- Prompt/context cache keyed by file hashes and graph slice IDs
- Tool-result cache for repeated read/search/trace calls
- Model list cache
- Build/test result cache
- Parsed symbol cache

Cache invalidation should be deterministic:

- file content hash changes
- command log append
- graph rebuild
- model/config change

Do not cache accepted truth in the cache. Only cache derived data.

### 4. Agent Execution Flow

1. User or editor integration submits a task.
2. Harness classifies the task and selects a workflow.
3. Context builder fetches the smallest useful slice of the project.
4. Router decides whether to run one agent or fan out to several subagents.
5. Subagents produce structured output, not direct edits.
6. Tool gateway executes any allowed reads or searches.
7. Diff engine converts proposed changes into reversible Commands.
8. Validator checks syntax, spec links, and cheap tests before commit.
9. Accepted changes are written as a `Batch` command so undo stays atomic.
10. Harness records cost, token usage, and the final summary.

### 5. Subagent Spawning Rules

Spawn subagents only when they save context or reduce risk.

Good reasons to spawn:

- independent analysis of requirement, spec, and code in parallel
- one agent can inspect files while another proposes a fix
- verifier can review a generated diff before commit

Bad reasons to spawn:

- every small thought step
- repeated rereading of the same files
- speculative branching without a budget limit

Recommended default:

- 1 planner
- up to 2 parallel specialist subagents
- 1 verifier

That keeps latency and cost under control.

### 6. Cost Controls

This is the part that matters most if you want the system to stay practical.

- Use a cheap router model for classification and task splitting.
- Use specialist models only for the step that needs them.
- Prefer targeted file and graph slices over full-project context.
- Reuse cached tool results whenever file hashes have not changed.
- Summarize prior runs instead of replaying full chat logs.
- Batch edits into one command group when they belong together.
- Stop after the first valid solution instead of generating multiple alternatives.
- Run local heuristics first, then call the model only when the heuristic cannot decide.

If a task can be answered from the trace graph, symbol table, or cached search result, do not call a large model.

### 7. OpenAI Agents SDK Position

Use the OpenAI Agents SDK only if it gives you faster agent orchestration or better tool-loop support.

My suggested stance is:

- Good fit: agent loop orchestration, tool calling, subagent coordination, structured responses.
- Not the source of truth: state, undo, permissions, persistence, cache invalidation.
- Not the harness owner: command application and traceability.

That makes the SDK a swappable runtime, not a lock-in point.

If you later swap to another framework, only the adapter changes. The harness, command system, and memory/cache policy stay the same.

### 8. Minimal Viable Implementation Order

1. Implement the harness router and context builder.
2. Add per-agent budgets and permission profiles.
3. Add subagent spawning with one planner and one verifier.
4. Add deterministic cache keys for file and graph slices.
5. Add session summaries and artifact memory.
6. Wire the editor integration boundary.
7. Optionally wrap the orchestration layer with OpenAI Agents SDK behind an adapter trait.

### 9. Bottom Line

For TraceLean, the most cost-effective architecture is a Rust-owned harness with:

- MCP for tools
- command-sourced state as the source of truth
- small planner + specialist subagents
- deterministic caches
- compact session/artifact memory
- OpenAI Agents SDK only as a replaceable orchestration adapter

That gives you a system that is easier to control, cheaper to run, and much easier to swap later.
