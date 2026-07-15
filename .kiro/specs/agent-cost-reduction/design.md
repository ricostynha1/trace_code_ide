# Design — Agent Cost Reduction

## Overview

Three interlocking subsystems reduce per-request cost:

1. **Prompt caching layer** — splits tool payload into static prefix (7 schemas, always cached) and dynamic suffix (loaded via `discover_tools`, retained briefly).
2. **Context-retention engine** — builds a pruned/summarized model view from the full user view before each request, using a single policy table and cache-aware cost decisions.
3. **Cost model** — unified formalization (explicit-cache as general case, automatic as special case with w=0) for when pruning/compaction is profitable.

No new public-facing API. Changes are internal to request-building pipeline, tool registry, and context manager.

---

## Architecture

### Request structure (post-change)

```
┌─────────────────────────────────────────────────────┐
│  SYSTEM PROMPT  (static, cacheable)                 │
│    - role instructions                              │
│    - tool-calling rules (T3)                        │
├─────────────────────────────────────────────────────┤
│  STATIC TOOL SCHEMAS  (7 tools, cacheable prefix)   │
│    discover_tools, list_directory, find,            │
│    read_file, edit_file, replace_str, run_shell     │
├─────────────────────────────────────────────────────┤
│  DYNAMIC TOOL SCHEMAS  (0-K tools, suffix)          │
│    (loaded by discover_tools, retained N turns)     │
├─────────────────────────────────────────────────────┤
│  [CACHE MARKERS — inserted at optimal boundaries]   │
│    (provider-dependent, can be placed anywhere      │
│     including deeper in conversation for explicit   │
│     cache providers)                                │
├─────────────────────────────────────────────────────┤
│  MODEL VIEW  (pruned conversation history)          │
│    - recent turns (3-5, always kept)                │
│    - summaries of older turns                       │
│    - active tool results (per policy table)         │
└─────────────────────────────────────────────────────┘
```

**Cache marker placement**: For explicit-cache providers (Anthropic, MiniMax explicit), cache markers are NOT limited to the static/dynamic boundary. They can be inserted anywhere in the payload where it makes economic sense — including deep inside conversation history if a stable block of content would benefit from caching. The cost engine decides optimal placement.

For **automatic-cache** providers (OpenAI, DeepSeek, MiniMax passive, Qwen): longest matching prefix is cached implicitly. Keeping system prompt + static tools unchanged maximizes prefix length.

---

## Component Design

### 1. Tool Registry & Dynamic Loader

Tool definitions live in an organized JSON file (see `data/tools.json`). Each tool entry includes:

```json
{
  "name": "read_file",
  "schema": { /* JSON Schema for tool parameters */ },
  "embedded_txt": "Long description for embedding retrieval. Explains all capabilities, edge cases, examples. Used by discover_tools semantic search.",
  "tier": "static",
  "category": "file_ops"
}
```

The `embedded_txt` field provides rich text for the embedding index, enabling accurate semantic retrieval via `discover_tools`.

```
ToolRegistry
  ├── static_tools: Vec<ToolSchema>       // 7 schemas, loaded from tools.json at init
  ├── dynamic_tools: Vec<DynamicEntry>    // loaded on demand
  └── config: DynamicToolConfig
        ├── retention_turns: usize         // default 5 (minimum turns before eligible for removal)
        ├── max_schema_chars: usize        // default 2048
        └── force_purge: bool             // user-triggered

DynamicEntry
  ├── schema: ToolSchema
  ├── loaded_at_turn: usize
  ├── last_used_turn: usize
  └── schema_chars: usize
```

**Lifecycle:**
1. `discover_tools(query)` → embedding retrieval (already implemented) → append results as `DynamicEntry`.
2. Each turn: increment turn counter. If `current_turn - last_used_turn > retention_turns` → mark as **eligible** for removal. (This is NOT automatic removal — it only means the cost engine can now consider removing it.)
3. Cost engine evaluates whether removing eligible dynamic tools is profitable using the same break-even formula as all other prune decisions.
4. Session reset or user action → clear all dynamic entries.

**Key point**: `retention_turns` is a minimum holding period, not a hard removal trigger. After that period, the tool enters the normal cost-based decision pipeline.

### 2. Static Tool Schemas (7 tools)

| Tool | Rationale |
|------|-----------|
| `discover_tools` | Gateway to dynamic tier. Embedding retrieval already implemented. |
| `list_directory` | Paginated, replaces old non-paginated version. |
| `find` | Merges `find_grep` + `find_embed`. `mode: auto|regex|semantic`. Saves one full schema. |
| `read_file` | Paginated, returns metadata (lines, bytes, size, mtime, permissions, owner, group). Subsumes `count_lines`. |
| `edit_file` | Line-range replace. `start==end` → insert. Both omitted → whole file. Also creates new files. |
| `replace_str` | Separate tool for clarity. Unique-match string replacement. Fails if != 1 occurrence. |
| `run_shell` | Arbitrary command. Head/tail truncation. |

Additionally, a `help_tool(tool_name)` is part of the static core — it retrieves the `embedded_txt` for any tool, giving the model detailed usage info on demand without bloating the schema payload.

### 3. `data` Field Conventions (Self-Teaching Output)

All tools return `ToolResult { success, content, data }`. The `data` field carries structured JSON:

- **Pagination**: `has_more`, `next_offset`, `total_matches`.
- **Truncation**: lines > 300 chars truncated; `any_truncation: true` + hint on how to fetch full.
- **Hints**: natural-language micro-instruction telling the model what to call next.

`data` is retained in model view for 1 turn (enough for model's next decision), then stripped. See retention policy.

### 4. Metadata Formatting Rules

| Field | Format |
|-------|--------|
| `size` | Human units: `10M`, `5K`, `312B` |
| `modified` | `<1h`: minutes (`3m`). `<1d`: hours+minutes (`3h12m`). `<30d`: days (`5d`). `>=30d`: `>30d`. |
| `permissions` | `ls -l` style: `drwxr-xr-x` |
| `owner`, `group` | String names |

### 5. System Prompt Additions (T3)

Appended to the static system prompt:

```
Tool rules:
- Tool calls: arguments only, no extra text unless explicitly asked.
- Batch: call multiple tools in a single response when possible.
- file_filter: glob syntax (shell wildcards), e.g. *.py, src/**/*.ts.
- Prefer `find` over `run_shell` for file search tasks.
- Do not repeat file contents already in context. Reference by path:line.
- When a tool returns has_more=true, decide whether more data is needed before calling again.
- Minimize redundant reads: if a file region is already in context and unmodified, do not re-read it.
```

### 6. Retention Policy Configuration File

The retention policy table lives in a readable JSON config file (`data/retention_policy.json`) for easy inspection and modification:

```json
{
  "_comment": "Retention policy — easily editable. Future: agents can tune these parameters.",
  "defaults": {
    "n_expected": 4,
    "budget_threshold_pct": 30,
    "recent_turns_protected": 4,
    "compression_ratio": 0.25
  },
  "entry_policies": {
    "discover_tools_result": {
      "ttl_turns": 1,
      "invalidate_on": ["schemas_in_payload", "dynamic_set_purged"],
      "default_action": "prune_after_forward"
    },
    "list_directory_result": {
      "ttl_turns": 5,
      "invalidate_on": ["write_in_dir", "delete_in_dir", "rename_in_dir", "newer_listing"],
      "default_action": "event_driven_ttl_fallback"
    },
    "find_result": {
      "ttl_turns": 5,
      "invalidate_on": ["edit_in_searched_scope", "newer_equivalent_search"],
      "default_action": "event_driven"
    },
    "read_file_content": {
      "ttl_turns": 8,
      "invalidate_on": ["edit_overlaps_region", "newer_read_covers_region"],
      "default_action": "prune_stale_keep_disjoint"
    },
    "tool_data_hints": {
      "ttl_turns": 1,
      "invalidate_on": ["followup_call_made", "unrelated_action"],
      "default_action": "strip_after_1_turn"
    },
    "edit_result": {
      "ttl_turns": 3,
      "invalidate_on": ["newer_edit_supersedes", "fresh_read_or_test"],
      "default_action": "keep_briefly"
    },
    "run_shell_result": {
      "ttl_turns": 6,
      "invalidate_on": ["relevant_edit_changes_inputs", "newer_command_supersedes"],
      "default_action": "event_driven_ttl_fallback"
    },
    "validation_error": {
      "ttl_turns": 0,
      "invalidate_on": ["retry_produced"],
      "default_action": "ephemeral_send_once"
    },
    "oversized_output": {
      "threshold_tokens": 4000,
      "default_action": "offload_to_tmp"
    }
  }
}
```

This file is designed to be human-readable and agent-tunable in the future.

---

## Context-Retention Engine

### Dual-View Architecture

```
UserView (append-only, immutable)
  └── full conversation: user msgs, assistant msgs, tool calls, tool results

ModelView (derived, mutable — rebuilt before each request)
  └── filtered/transformed entries from UserView
```

### Retention Entry Metadata

```rust
struct RetentionEntry {
    kind: EntryKind,          // ToolCall, ToolResult, ToolData, UserMsg, AssistantMsg, Summary, OverflowPtr
    resources: Vec<Resource>, // files, line ranges, dirs, commands, dynamic tools
    created_turn: usize,
    last_used_turn: usize,
    approx_tokens: usize,
    ttl: Option<usize>,       // turns until eligible for cost-based removal
    invalidation_events: Vec<InvalidationEvent>,
    action: RetentionAction,  // Keep, StripData, Prune, CacheResult, Offload, Summarize
}
```

### Retention Engine Pipeline (per-request)

```
1. Record dependencies + state versions on entry creation
2. Apply invalidation events (resource-overlap-first, then TTL eligibility)
3. Transform entries per policy table (strip data, offload oversized)
4. Check repeated calls → serve from cache if unchanged
5. Measure model-view tokens + estimate prefix-cache cost
6. Immediate prune: only for correctness (errors, overflow, revoked tools, consumed data)
7. Cost-based prune: for all eligible entries, compute break-even → only prune when profitable
8. Compaction trigger: manual "compact now" button OR cost engine determines summarization is profitable
```

**Important**: There is NO hardcoded overflow limit. Compaction happens when:
- User presses "compact now" button (explicit action)
- The cost engine determines that summarizing middle turns is more profitable than keeping them (using the same break-even math as all other decisions)

---

## Cost Model — Unified Formalization

The explicit-cache model is the **general case**. Automatic-cache is the special case where `w = 0`.

### Variables

| Symbol | Meaning |
|--------|---------|
| `c` | Cost per token, uncached input |
| `c_r` | Cost per token, cached read |
| `c_w` | Cost per token, cache write (explicit only; 0 for automatic) |
| `d` | Cache read discount = `c_r / c` (e.g. 0.1 for Anthropic, 0.5 for OpenAI) |
| `w` | Cache write multiplier = `c_w / c` (e.g. 1.25 for Anthropic 5min; 0 for automatic) |
| `P` | Current cached prefix length (tokens) |
| `L` | Total model-view length before pruning (tokens) |
| `L'` | Total model-view length after pruning (tokens) |
| `Δ` | Tokens removed = `L - L'` |
| `P_unchanged` | Prefix tokens that remain identical after prune |
| `P_invalidated` | Prefix tokens that become uncached = `P - P_unchanged` |
| `N` | Expected remaining turns in session (tunable, default 4) |
| `S` | Summary length when compacting (typically ~25% of removed content) |
| `R` | Tokens being summarized (removed content) |
| `c_sum_in` | Summarizer model input cost per token |
| `c_sum_out` | Summarizer model output cost per token |
| `T_idle` | Time since last request (seconds) |
| `TTL_provider` | Provider's cache TTL (seconds) |

### General Break-Even Formula (covers both provider types)

```
# Savings per future turn from sending fewer tokens:
savings_per_turn = Δ · d · c

# One-time penalty from cache invalidation + re-write:
#   - (1-d) = cost of re-processing invalidated tokens at full price minus cached price
#   - w = cost of re-writing cache (0 for automatic providers)
penalty = P_invalidated · (1 - d + w) · c

# TTL discount: if cache was already cold, penalty is reduced
#   - If T_idle > TTL_provider, cache already expired → P_invalidated effectively 0
if T_idle > TTL_provider:
    penalty = 0  # cache was already cold, no miss penalty

# Break-even: prune when expected savings exceed penalty
N · Δ · d · c ≥ penalty

# Simplified (dividing both sides by c):
N · Δ · d ≥ P_invalidated · (1 - d + w)    [when cache is warm]
N · Δ · d ≥ 0                                [when cache is cold — always profitable]
```

**Automatic-cache** (w=0): `N · Δ · d ≥ P_invalidated · (1 - d)`
**Explicit-cache** (w>0): `N · Δ · d ≥ P_invalidated · (1 - d + w)`
**Cache cold** (T_idle > TTL): Always profitable to prune (penalty = 0).

### Summarization Cost Extension

```
# Net tokens saved after inserting summary:
Δ_net = R - S    (where S ≈ 0.25 · R by default compression ratio)

# Summarization LLM call cost:
summarization_cost = R · c_sum_in + S · c_sum_out

# Full break-even with summarization:
N · Δ_net · d · c ≥ P_invalidated · (1 - d + w) · c + summarization_cost

# Expanded:
N · (R - S) · d · c ≥ P_invalidated · (1 - d + w) · c + R · c_sum_in + S · c_sum_out
```

### Summarizer Model Selection

Two models available: brain model (expensive, primary) and cheap model (e.g. MiniMax 2.5).
The cost engine decides which to use for summarization:

```
# Cost with brain model (may benefit from cached prefix):
cost_brain = R · c_brain_in + S · c_brain_out

# Cost with cheap model (separate call, no cache benefit):
cost_cheap = R · c_cheap_in + S · c_cheap_out

# Use whichever is cheaper:
summarizer = if cost_cheap < cost_brain { cheap_model } else { brain_model }
```

The system supports exactly 2 model slots: brain + cheap. They can be the same model. Selection is automatic based on cost computation. User picks both models in the frontend.

### Middle-Prune (Prefix Intact)

```
# When pruning from conversation middle (not affecting prefix):
# P_invalidated = 0, so penalty = 0
# Always profitable if Δ > 0
# This is the common case for conversation compaction
```

### Decision Algorithm (Pseudocode)

```rust
/// Unified prune decision. Works for both automatic (w=0) and explicit (w>0) providers.
/// Math: N · Δ · d ≥ P_invalidated · (1 - d + w) when cache warm.
///       Always profitable when cache cold (T_idle > TTL).
fn should_prune(entry: &RetentionEntry, ctx: &PruneContext) -> Decision {
    let delta = entry.approx_tokens;
    let p_invalidated = ctx.compute_prefix_invalidation(entry);
    let provider = ctx.provider_config();

    // TTL discount: if user idle time exceeded provider TTL, cache is already cold
    // In that regime, there's no miss penalty because we'd pay full price anyway
    let cache_is_cold = match provider.ttl_seconds {
        Some(ttl) => ctx.time_since_last_request_secs > ttl,
        None => false, // automatic providers: cache "never" expires (managed by provider)
    };

    // savings_per_turn = Δ · d · c
    // Each future turn sends Δ fewer cached tokens, saving Δ·d·c per turn
    let savings_per_turn = delta as f64 * provider.cache_read_discount * provider.cost_per_token;

    // penalty = P_invalidated · (1 - d + w) · c
    // One-time cost: invalidated prefix re-processed at full price + cache re-write cost
    // For automatic providers: w=0, so penalty = P_invalidated · (1-d) · c
    // For explicit providers: w>0, adds write cost on top
    let penalty = if cache_is_cold {
        0.0 // cache already expired, no additional miss cost
    } else {
        p_invalidated as f64
            * (1.0 - provider.cache_read_discount + provider.cache_write_multiplier)
            * provider.cost_per_token
    };

    let n = ctx.n_expected; // tunable, default 4

    // Break-even: N · savings_per_turn ≥ penalty
    if n as f64 * savings_per_turn >= penalty {
        Decision::Prune { savings_per_turn, penalty, n }
    } else {
        Decision::Keep { reason: "penalty exceeds expected savings" }
    }
}

/// Summarization decision. Same unified model + summarization LLM cost.
/// Math: N · (R-S) · d · c ≥ P_invalidated · (1-d+w) · c + R·c_sum_in + S·c_sum_out
fn should_summarize(entries: &[RetentionEntry], ctx: &PruneContext) -> Decision {
    let r: usize = entries.iter().map(|e| e.approx_tokens).sum();
    let s = (r as f64 * ctx.compression_ratio) as usize; // ~25% of original
    let delta_net = r - s;
    let p_invalidated = ctx.compute_prefix_invalidation_bulk(entries);
    let provider = ctx.provider_config();

    let cache_is_cold = match provider.ttl_seconds {
        Some(ttl) => ctx.time_since_last_request_secs > ttl,
        None => false,
    };

    // savings_per_turn = Δ_net · d · c  (net savings accounting for summary insertion)
    let savings_per_turn = delta_net as f64 * provider.cache_read_discount * provider.cost_per_token;

    // Cache miss penalty (same as prune)
    let cache_penalty = if cache_is_cold {
        0.0
    } else {
        p_invalidated as f64
            * (1.0 - provider.cache_read_discount + provider.cache_write_multiplier)
            * provider.cost_per_token
    };

    // Summarization LLM call cost — pick cheaper of brain vs cheap model
    // cost = R · c_sum_in + S · c_sum_out
    let (sum_in_cost, sum_out_cost) = ctx.pick_cheapest_summarizer();
    let sum_cost = r as f64 * sum_in_cost + s as f64 * sum_out_cost;

    let total_penalty = cache_penalty + sum_cost;

    // Break-even: N · savings_per_turn ≥ total_penalty
    if ctx.n_expected as f64 * savings_per_turn >= total_penalty {
        Decision::Summarize { savings_per_turn, total_penalty, n: ctx.n_expected }
    } else {
        Decision::Keep { reason: "summarization cost exceeds expected savings" }
    }
}
```

### Provider Cache Configuration

Lives in `data/provider_cache.json`. This info also gets merged into the generated `models.json` — the generation script adds these fields if not present.

```json
{
  "providers": {
    "openai": {
      "cache_mode": "automatic",
      "cache_read_discount": 0.5,
      "cache_write_multiplier": 0.0,
      "ttl_seconds": null,
      "requires_markers": false
    },
    "anthropic_5min": {
      "cache_mode": "explicit",
      "cache_read_discount": 0.1,
      "cache_write_multiplier": 1.25,
      "ttl_seconds": 300,
      "requires_markers": true
    },
    "anthropic_1h": {
      "cache_mode": "explicit",
      "cache_read_discount": 0.1,
      "cache_write_multiplier": 2.0,
      "ttl_seconds": 3600,
      "requires_markers": true
    },
    "deepseek": {
      "cache_mode": "automatic",
      "cache_read_discount": 0.2,
      "cache_write_multiplier": 0.0,
      "ttl_seconds": null,
      "requires_markers": false
    },
    "minimax_passive": {
      "cache_mode": "automatic",
      "cache_read_discount": 0.2,
      "cache_write_multiplier": 0.0,
      "ttl_seconds": null,
      "requires_markers": false
    },
    "minimax_explicit": {
      "cache_mode": "explicit",
      "cache_read_discount": 0.1,
      "cache_write_multiplier": 1.25,
      "ttl_seconds": null,
      "requires_markers": true
    },
    "qwen": {
      "cache_mode": "automatic",
      "cache_read_discount": 0.2,
      "cache_write_multiplier": 0.0,
      "ttl_seconds": null,
      "requires_markers": false
    }
  }
}
```

**Note**: Gemini removed from scope for now (hourly storage billing adds too much complexity; deferred).

---

## TTL Expiry Tracking (Critical)

The engine MUST track `time_since_last_request` for each turn:

```rust
struct TurnTiming {
    turn_id: usize,
    request_sent_at: Instant,
    response_received_at: Instant,
    idle_before_turn: Duration, // time between previous response and this request
}
```

When computing penalty, if `idle_before_turn > provider.ttl_seconds`, the cache was already cold. In that regime:
- Penalty drops to 0 (for explicit providers)
- Pruning is always profitable (any Δ > 0 saves money)
- The engine should be more aggressive about compaction during "cold" turns

This is critical for explicit-cache providers where TTL is short (Anthropic 5min).

---

## Observability

Every prune/keep/summarize decision is logged:

```json
{
  "turn": 12,
  "entry_kind": "read_file",
  "resource": "src/main.rs:1-50",
  "tokens": 420,
  "decision": "prune",
  "reason": "edit overlaps region",
  "savings_per_turn": 0.0042,
  "penalty": 0.0,
  "n_expected": 4,
  "provider": "anthropic_5min",
  "cache_was_cold": false,
  "time_since_last_request_s": 45
}
```

Additionally, the engine logs **predicted vs actual cached tokens** each turn to detect anomalies and tune the prediction algorithm (Req 5.0).

---

## Open Questions

1. **N_expected tuning**: Default 4 is conservative. With session telemetry, could become adaptive (median remaining turns from historical sessions of similar type). Deferred to v2.
2. **Summarizer model selection**: Currently cost-decided between brain and cheap model. Worth benchmarking whether summarization prompt can share the system prompt prefix (making brain model cheaper for summarization). 
3. **TTL expiry racing**: Implemented in v1 — engine tracks inter-turn latency and adjusts penalty. May need refinement based on real-world session timing distributions.
4. **Agent-tunable parameters**: The retention policy JSON is designed for future agent self-tuning. Whether to expose this as an agent capability is TBD.
