# Requirements — Agent Cost Reduction

## Introduction

Reduce per-request token cost of the TraceLean IDE agent by: (1) maximizing prompt-cache hit rates via a static/dynamic tool split, (2) shrinking the static tool payload from ~13 schemas to 7, (3) adding a unified context-retention engine that uses cache-aware cost decisions to prune/summarize the model view, and (4) formalizing the cost math (explicit-cache as general case, automatic as special case) so decisions are economically grounded across all providers.

## Glossary

- **Static Core**: Fixed set of tool schemas sent every request — forms cacheable prefix.
- **Dynamic Tier**: Tool schemas loaded on-demand via `discover_tools`; appended after static prefix.
- **Model View**: Derived conversation history sent to the LLM (may differ from full user-visible history).
- **User View**: Complete, append-only conversation and tool history — never mutated.
- **Retention Engine**: Subsystem that builds model view from user view by applying pruning, summarization, and caching policies.
- **Prompt Cache**: Provider-side mechanism avoiding re-processing unchanged tokens (automatic or explicit).
- **Cache Miss Penalty**: Extra cost when a prefix change invalidates the cache.
- **N_expected**: Tunable parameter estimating remaining turns in session (default 4).
- **Brain Model**: Primary expensive model used for agent reasoning.
- **Cheap Model**: Secondary cost-effective model (e.g. MiniMax 2.5) for tasks like summarization.

## Requirements

### Requirement 1: Static/Dynamic tool split with prompt caching

**User Story:** As an operator, I want the agent's tool payload split into a small static core (always sent, cacheable) and a dynamic tier (loaded on demand), so that prompt-cache hit rates stay high and per-request cost drops.

#### Acceptance Criteria

1. THE system SHALL send exactly 7 static tool schemas on every request: `discover_tools`, `list_directory`, `find`, `read_file`, `edit_file`, `replace_str`, `run_shell`.
2. WHEN the model calls `discover_tools(query)`, THE system SHALL run embedding-based retrieval (already implemented), append matching tool schemas to the `tools` array, and return a `toolResult` listing the newly available tools.
3. THE system SHALL retain dynamically-loaded tools for a minimum of `retention_turns` (default: 5) after last use. After that period, tools become eligible for cost-based removal by the retention engine. This is NOT automatic removal — the cost engine decides.
4. THE system SHALL purge dynamic tools when their combined schema size exceeds a configurable limit (default: 2048 chars) OR on session reset.
5. THE system SHALL expose a user-triggered action ("remove dynamic tools") that immediately purges all dynamic-tier schemas and reverts to static-only.

### Requirement 2: Core tool set redesign (7 static tools)

**User Story:** As an operator, I want the static tool payload minimized to 7 well-designed schemas, so that the fixed per-request token cost is as low as possible.

#### Acceptance Criteria

0. Tool definitions SHALL live in an organized JSON file (`data/tools.json`) with a new field `embedded_txt` containing a rich description for embedding retrieval. A `help_tool(tool_name)` SHALL be part of the static core, allowing the model to retrieve detailed tool info on demand.
1. THE static core SHALL contain exactly: `discover_tools`, `list_directory`, `find`, `read_file`, `edit_file`, `replace_str`, `run_shell`.
2. `find` SHALL merge the functionality of old `find_grep` + `find_embed` via a `mode` parameter (`auto|regex|semantic`).
3. `edit_file` SHALL support line-range replacement (`[start, end)` semantics), insertion (`start == end`), and whole-file replacement (both omitted). Also handles file creation for non-existent paths.
4. `replace_str` SHALL remain a separate tool for unique-string replacement (single occurrence, fails if not exactly one match).
5. `read_file` SHALL return file metadata in its response: `lines`, `bytes`, `size` (human), `modified` (relative), `permissions`, `owner`, `group`. Reading an empty range retrieves only metadata.
6. Tools previously in the static set (`delete_file`, `query_trace`, `query_code`, `list_requirements`, `get_symbols`, `count_lines`) SHALL be demoted to the dynamic tier or removed entirely.
7. `count_lines` SHALL be removed (redundant with `read_file` metadata).

### Requirement 3: System prompt compactness rules

**User Story:** As an operator, I want the system prompt to enforce compact tool-calling behavior, reducing wasted output tokens.

#### Acceptance Criteria

1. THE system prompt SHALL instruct the model: "Tool calls: arguments only, no extra text unless explicitly asked."
2. THE system prompt SHALL instruct the model: "Batch: call multiple tools in a single response when possible."
3. THE system prompt SHALL instruct the model: "file_filter: glob syntax (shell wildcards), e.g. `*.py`, `src/**/*.ts`."
4. THE system prompt SHALL instruct the model: "Prefer `find` over `run_shell` for file search tasks."
5. THE system prompt SHALL instruct the model: "Do not repeat file contents already visible in context. Reference by path and line range."

### Requirement 4: Unified context-retention engine

**User Story:** As an operator, I want a single policy engine managing the model view — pruning stale data, deduplicating reads, summarizing old turns, and respecting cache boundaries — so that context stays within budget without ad-hoc logic scattered across the codebase.

#### Acceptance Criteria

1. THE retention engine SHALL maintain a separate model view (mutable, derived) and user view (append-only, immutable).
2. Each model-view entry SHALL carry retention metadata: `kind`, `resources`, `created_turn`, `last_used_turn`, `approx_tokens`, optional `TTL`, invalidation events, and retention action.
3. THE engine SHALL invalidate entries based on resource-level events (file edit overlaps a read region, newer listing supersedes older, etc.) per the policy table in the design doc.
4. THE engine SHALL deduplicate repeated tool calls with unchanged arguments and unchanged dependencies — serve cached result, don't append duplicate content.
5. THE engine SHALL treat validation/schema errors as ephemeral: send the correction once, never commit the failed call+error pair to durable history.
6. THE engine SHALL offload oversized tool outputs to a temporary file, retaining only a bounded preview + path + size + retrieval hint in the model view.
7. THE engine SHALL compact the middle of the model view (summarize older entries, preserve latest 3–5 turns, maintain user view intact) when the **cost engine determines summarization is profitable** using the same break-even formula as all other prune decisions. There is NO hardcoded budget threshold — compaction is a cost decision like everything else.
8. THE engine SHALL expose a manual "compact now" button that triggers immediate compaction regardless of cost calculation.
9. THE retention policy SHALL live in a readable JSON config file (`data/retention_policy.json`) for easy inspection, modification, and future agent self-tuning.

### Requirement 5: Cache-aware pruning decisions (cost model)

**User Story:** As an operator, I want pruning/compaction decisions to be economically grounded — only prune when the expected future savings exceed the one-time cache miss penalty — so that aggressive pruning doesn't accidentally increase cost.

#### Acceptance Criteria

0. THE engine SHALL reliably predict the number of cached tokens in the next prompt. It MUST log predicted vs actual cached tokens each turn, and record anomalies to tune the prediction algorithm.
1. THE engine SHALL use a unified cost model (explicit-cache as general case, automatic as special case with w=0) before deciding to prune or summarize.
2. THE engine SHALL support automatic-cache providers (OpenAI, DeepSeek, MiniMax passive, Qwen) and explicit-cache providers (Anthropic, MiniMax explicit) with their respective cost structures.
3. For all providers: prune only when `N_expected · Δ · d ≥ P_invalidated · (1 - d + w)` (where w=0 for automatic).
4. For summarization: include summarization LLM call cost in penalty calculation.
5. THE engine SHALL track `time_since_last_request` and discount penalty when TTL has likely expired (cache was already cold → penalty = 0, always profitable to prune). This is critical for short-TTL explicit providers.
6. `N_expected` SHALL be a tunable parameter (default: 4), configurable per-session or globally.
7. THE engine SHALL log each prune/keep/summarize decision with computed values (savings, penalty, N_expected, decision, cache_was_cold) for observability and future tuning.
8. Summarizer model selection SHALL be cost-decided: the engine picks whichever of brain model or cheap model produces lower total summarization cost.

### Requirement 6: Provider cache taxonomy

**User Story:** As an operator, I want the system to know each provider's caching model (automatic vs. explicit, discount rate, write cost, TTL) so that cost decisions are provider-correct.

#### Acceptance Criteria

1. THE system SHALL maintain a provider cache configuration (`data/provider_cache.json`) with fields: `cache_mode` (automatic|explicit), `cache_read_discount`, `cache_write_multiplier`, `ttl_seconds`, `requires_markers`.
2. THE configuration SHALL cover: OpenAI, Anthropic Claude (5min and 1h tiers), DeepSeek, MiniMax (passive and explicit modes), Qwen. Gemini deferred (hourly storage billing adds too much complexity).
3. THE cost model SHALL use each provider's specific values when computing prune decisions — not a single hardcoded discount.
4. Cache markers for explicit providers SHALL be placeable anywhere in the payload (not just static/dynamic boundary) — the cost engine determines optimal placement.
5. Provider cache info SHALL be integrated into the existing `models.json` generation script — the script adds cache fields if not present.
