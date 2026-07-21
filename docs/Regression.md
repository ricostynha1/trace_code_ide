# Regression.md — benchmark regression testing for compaction / cost-model changes

Quick recipe for checking that a change to compaction, retention, or the cost
model didn't make the agent dumber or more expensive. Not a test-writing guide.

*Paths are relative to [`tracelean/`](../tracelean/) (`cd tracelean` before running the commands below), not this `docs/` folder.*

## The harness

`tracelean-bench` (`core/src/bin/tracelean-bench.rs`) runs exercism tasks
through the real agent runtime headlessly and prints a `BenchmarkRun` JSON
(`core/src/ai/benchmark.rs`) to stdout. A spend cap aborts the run before it
gets expensive.

```sh
# One task, free, no API key — sanity check the harness itself:
cargo run --bin tracelean-bench -- --provider mock hello-world

# Real run (uses paid API — set the cap!):
cargo run --bin tracelean-bench -- \
  --provider openrouter --model anthropic/claude-sonnet-4-20250514 \
  --cap 0.50 --all > after.json

# `--verbose` streams prompts/tool calls to stderr if you need to eyeball behavior.
# `--list-models` shows what the provider offers.
```

Env fallbacks: `PROVIDER`, `MODEL_ID`, `SPEND_CAP_USD`, `OPENROUTER_API_KEY`,
`AWS_BEARER_TOKEN_BEDROCK` / `AWS_REGION` for Bedrock.

## The recipe

1. On the **baseline** commit (before your change):
   `cargo run --bin tracelean-bench -- <provider/model flags> --cap 0.50 --all > before.json`
2. Apply your change, same command → `after.json`. Same model, same tasks,
   same cap — only your change may differ.
3. Compare the two JSONs.

## How to read the results

Top-level: `pass_count` / `fail_count` (quality), `session_stats.total_cost_usd`
(cost), `aborted`/`abort_reason` (did the cap trip?).

Per task in `tasks[]`:

| Field | Regression smell |
|---|---|
| `passed` | A task that passed before and fails after = quality regression. Rerun once to rule out flakiness before blaming the change. |
| `model_iterations` | More loop iterations for the same task usually means the model lost context (over-aggressive trimming) and is re-deriving things. |
| `tool_calls` | Same signal: re-reading files it already read points at pruned context. |
| `token_usage` | `input_tokens` down with `passed` unchanged = successful cost reduction. `cached_tokens` down = your change is breaking the cache prefix (compaction that rewrites early messages destroys cache hits — can make cost go **up**). |
| `cost.total_usd` | The bottom line, per task. |
| `parsing_failures` | Should stay 0; increases mean malformed model output, often from a mangled context. |

Rule of thumb: a compaction change is good iff `pass_count` holds steady while
`total_cost_usd` (or input tokens) drops. Cheaper but failing = worse, not
better.

## Judging compaction behavior specifically

For a finer-grained look than the aggregate JSON, run the app and open the AI
panel's **Log** tab: each request that compacted shows a ✂️/📝 badge (messages
removed + tokens saved) and an expandable per-message decision list
(`trimmed` / `trim_candidate` / `folded into summary` / `kept (user message)`).
If trimming fires on the first couple of exchanges or repeatedly back-to-back,
the tuning is too aggressive — see `trimmed_summarization_problem.md` for the
knobs (`n_expected_rounds`, `recent_turns_protected`, the `net_benefit`
tie-break).
