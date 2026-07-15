# Tasks — Agent Cost Reduction

## Phase 1: Tool Registry & Static/Dynamic Split (Req 1, 2)

- [ ] 1. Create `data/tools.json` with all tool definitions: schema, `embedded_txt` field, tier (static/dynamic), category. Include `help_tool` in static core. (Req 2.0)
- [ ] 2. Define `ToolRegistry` struct with `static_tools`, `dynamic_tools`, `DynamicToolConfig` (retention_turns as minimum eligibility, max_schema_chars, force_purge). (Req 1.1)
- [ ] 3. Implement the 7 static tool schemas loaded from `data/tools.json`. Wire into request builder as immutable prefix. (Req 2.1–2.4)
- [ ] 4. Implement `discover_tools` handler: call existing embedding retrieval (uses `embedded_txt`), wrap results as `DynamicEntry`, append to `dynamic_tools`, return toolResult listing new schemas. (Req 1.2)
- [ ] 5. Implement dynamic-tool eligibility logic: track `last_used_turn`. After `retention_turns` elapsed, mark as **eligible** for cost-based removal (NOT automatic removal). Cost engine decides actual removal. (Req 1.3)
- [ ] 6. Implement user-triggered "remove dynamic tools" action — clears all `DynamicEntry`s, reverts to static-only. (Req 1.5)
- [ ] 7. Implement `find` tool merging old `find_grep` + `find_embed` with `mode: auto|regex|semantic`. (Req 2.2)
- [ ] 8. Add metadata to `read_file` response: lines, bytes, size (human), modified (relative), permissions, owner, group. Empty range = metadata only. Remove `count_lines` tool. (Req 2.5, 2.7)
- [ ] 9. Demote `delete_file`, `query_trace`, `query_code`, `list_requirements`, `get_symbols` to dynamic tier (register in embedding index via `embedded_txt`, remove from static payload). (Req 2.6)

## Phase 2: System Prompt (Req 3)

- [ ] 10. Add tool-calling rules to system prompt: args-only, batching, glob syntax, prefer `find` over `run_shell`, no redundant reads, `has_more` awareness. (Req 3.1–3.5)

## Phase 3: Provider Cache Configuration (Req 6)

- [ ] 11. Create `data/provider_cache.json` with fields: `cache_mode`, `cache_read_discount`, `cache_write_multiplier`, `ttl_seconds`, `requires_markers` for: OpenAI, Anthropic 5min/1h, DeepSeek, MiniMax passive/explicit, Qwen. Gemini deferred. (Req 6.1–6.2)
- [ ] 12. Update the existing `models.json` generation script to merge provider cache fields into `models.json` if not already present. (Req 6.5)
- [ ] 13. Implement config loader that parses `data/provider_cache.json` and exposes it to the retention engine. (Req 6.3)

## Phase 4: Context-Retention Engine — Core (Req 4)

- [ ] 14. Create `data/retention_policy.json` with all entry policies (TTLs, invalidation events, default actions). Human-readable, agent-tunable in future. (Req 4.9)
- [ ] 15. Define `RetentionEntry` struct: `kind`, `resources`, `created_turn`, `last_used_turn`, `approx_tokens`, `ttl`, `invalidation_events`, `action`. (Req 4.2)
- [ ] 16. Implement dual-view architecture: `UserView` (append-only) and `ModelView` (derived). Refactor request builder to construct model view from user view. (Req 4.1)
- [ ] 17. Implement resource dependency tracking: map entries to files, line ranges, directories, commands. Record state versions (file mtime/hash, dir listing hash). (Req 4.3)
- [ ] 18. Implement event-driven invalidation: detect file-edit overlapping a read region, newer listing superseding older, newer read superseding older in same range. (Req 4.3)
- [ ] 19. Implement TTL eligibility: if no invalidation event fires, mark entries as eligible for cost-based removal after their configured TTL. (Req 4.3)
- [ ] 20. Implement deduplication: detect repeated tool calls with same args + unchanged deps → serve cached result, skip append. (Req 4.4)
- [ ] 21. Implement ephemeral error handling: validation/schema errors sent once, never persisted to model view. (Req 4.5)
- [ ] 22. Implement overflow offloading: tool outputs > 4K tokens → write to tmp file, keep preview + path + size + hint in model view. (Req 4.6)
- [ ] 23. Write extended test collection for Phase 4 (create tests, run only after all Phase 4-8 implementation complete). Cover: invalidation, dedup, TTL, offload, ephemeral errors.

## Phase 5: Context-Retention Engine — Compaction & Summarization (Req 4, 5)

- [ ] 24. Implement token measurement: count model-view tokens before each request. Log predicted vs actual cached tokens. Record anomalies. (Req 5.0)
- [ ] 25. Implement cost-based compaction: cost engine determines when summarizing middle turns is profitable using break-even formula. No hardcoded budget threshold. (Req 4.7)
- [ ] 26. Implement "compact now" manual button: immediate compaction regardless of cost calculation. (Req 4.8)
- [ ] 27. Implement summarization call: cost-decide between brain model and cheap model, send entries to summarize, insert summary into model view replacing originals. Math comments in implementation. (Req 4.7, 5.8)
- [ ] 28. Frontend: add model selection UI — user picks brain model + cheap model (can be same). Engine uses these for cost decisions. (Req 5.8)

## Phase 6: Cost-Aware Pruning Decisions (Req 5)

- [ ] 29. Implement unified `should_prune` function: compute `savings_per_turn`, `penalty` using general formula `N·Δ·d ≥ P_invalidated·(1-d+w)`. Works for both provider types (w=0 for automatic). Include math explanation as comments. (Req 5.1, 5.3)
- [ ] 30. Implement `should_summarize` function: include summarization LLM cost in penalty. Pick cheapest summarizer (brain vs cheap). Math comments. (Req 5.1, 5.4, 5.8)
- [ ] 31. Integrate cost model into retention pipeline: eligible entries go through cost-based decision before actual removal. (Req 5.1)
- [ ] 32. Make `N_expected` configurable (per-session and global, default 4) via retention policy JSON. (Req 5.6)
- [ ] 33. Implement prune/keep/summarize decision logging: turn, entry_kind, resource, tokens, decision, savings, penalty, N_expected, provider, cache_was_cold. (Req 5.7)

## Phase 7: TTL Expiry Tracking & Explicit-Cache Support (Req 5, 6)

- [ ] 34. Implement turn timing tracker: record `time_since_last_request` for each turn. (Req 5.5 — CRITICAL)
- [ ] 35. Implement TTL-aware penalty discounting: if `time_since_last_request > provider.ttl_seconds`, set penalty = 0 (cache was already cold, always profitable to prune). (Req 5.5)
- [ ] 36. Implement optimal cache marker placement for explicit providers: cost engine determines best positions anywhere in payload (not just static/dynamic boundary). (Req 6.4)
- [ ] 37. Implement cache-write cost accounting in prune decisions for explicit providers (the `w` term). (Req 5.3)

## Phase 8: Integration & Observability

- [ ] 38. Wire retention engine into main request-building pipeline: user view → retention engine → model view → provider request.
- [ ] 39. Run the extended test collection from task 23. Fix any failures.
- [ ] 40. Add integration test: multi-turn session exercising dynamic tool loading, retention, cost-based pruning, TTL tracking, and compaction.
- [ ] 41. Add observability: structured log output for all decisions + predicted vs actual cache tokens + anomaly detection.
- [ ] 42. Benchmark: measure prefix-cache hit rate and per-request token cost before/after on representative session trace.
