# T3 — Requirement ↔ model judge: implementation plan

Design reference: `docs/formal_traceability_and_lsp_plan.md` §6.3, §6.4, §6.5.
Depends on T1 (links, evidence records) and T2 (the compiled model runner, reused to execute
witnesses).

## What it does
When a requirement clause or its `@models` anchor changes, the link goes stale. The judge
decides whether the Lean model still faithfully formalizes that clause — and its claims
about Lean are **executed**, not trusted.

## 1. Module layout — `core/src/judge/`

| File | Contents |
|---|---|
| `mod.rs` | `judge_clause()` orchestration, evidence writing |
| `prompt.rs` | versioned prompt template + rendering |
| `verdict.rs` | verdict types, JSON parsing, validation |
| `witness.rs` | witness execution against the compiled model |
| `calibration.rs` | the calibration suite runner |

## 2. Prompt (`prompt.rs`)

The prompt text is fixed in design §6.3 and is copied verbatim into a `const PROMPT_V1: &str`
with `{placeholders}`. Rules:
- The template carries a **version string** (`"judge/v1"`) recorded in every evidence record.
  Changing the text requires bumping the version and re-running calibration.
- Rendered inputs: requirement id, clause key, clause text (current + previous if changed),
  model source (current + previous if changed), glossary, previous verdict.
- Previous-versions come from the lockfile's recorded hashes plus git blob lookup; if the
  previous text is unavailable, pass `null` and note it — never fabricate a diff.

## 3. Verdict (`verdict.rs`)

```rust
pub enum Verdict { Agrees, UnderConstrained, OverConstrained, Both, Unmodelable, Unclear }
pub struct JudgeReply {
    pub verdict: Verdict,
    pub witness: Option<Witness>,     // { input: Value, clause_requires: String, model_produces: String }
    pub explanation: String,
    pub suggested_patch: Option<String>,
    pub confidence: Confidence,
}
```
- Parsed strictly from JSON; a reply that does not parse is retried once with a repair
  instruction, then recorded as `Unclear { reason: "unparseable" }`.
- **Validation:** a drift verdict (`UnderConstrained`/`OverConstrained`/`Both`) without a
  witness is invalid → retried once, then downgraded to `Unclear`. This is enforced in code,
  not left to the prompt.

## 4. Witness execution (`witness.rs`) — the part that makes this defensible

1. Take `witness.input`, wrap it as a DRT `Case`, send it to the **compiled model runner**
   from T2 (same protocol, same binary, microseconds).
2. Compare the model's actual reply to the judge's claimed `model_produces`. Comparison is
   semantic where possible: parse both as JSON and compare structurally; fall back to
   normalized string comparison; when the claim is prose ("accepted"), use a small
   containment/heuristic match and record the comparison mode in the evidence.
3. Outcomes:
   - **confirmed** → finding `judge-drift` (error), witness appended permanently to
     `.tracelean/drt/seeds/<REQ-ID>.jsonl`, evidence records the confirmed witness.
   - **falsified** → verdict discarded, logged against the prompt version, retried once; a
     second falsification records `judge-unreliable` (warning, surfaced to the user) and
     writes **no** drift finding.
   - **inexecutable** (model runner missing / input does not decode) → `Unclear`, with the
     decode error attached. Never treat as drift.

## 5. Levels and what a verdict may do

- `Agrees` (witness-free by definition) → writes evidence at **L2** for bond
  `RequirementModel`, and nothing else. It can never promote L3/L4 (design §7 rule 2).
- Drift verdicts → **remove** L2 for that bond and raise `judge-drift`.
- `Unmodelable` → proposes `@exempt` + a required `@tests` link; proposal only, applied by a
  `Command` if the user/agent accepts.
- `Unclear` → leaves the link stale and asks for a human. Never a level change.

## 6. Second opinion by backtranslation (§6.5)

Enabled per-requirement (default: only for requirements matching the policy's `high_value`
glob). Three calls:
1. Render the Lean model into English *without* the clause in context.
2. A comparison call scores the produced English against the real clause text.
3. Disagreement with the primary `Agrees` verdict downgrades it to `Unclear` and flags for
   review — it never by itself produces a drift finding.

## 7. Calibration suite (`calibration.rs`)

- Fixtures under `tests/fixtures/judge_calibration/`: pairs of (clause, model) with an
  expected verdict class, including deliberately mutated models — threshold changed,
  quantifier flipped, an error case dropped, a condition negated, a clause made
  unmodelable.
- Runner: `cargo test judge_calibration -- --ignored` (gated: needs a provider key), plus a
  **mock-provider** variant that runs in normal CI using recorded replies, so prompt-format
  regressions are caught without network.
- **Gate: zero false `Agrees` on mutated pairs.** False alarms on the agreeing pairs are
  reported but do not fail the suite — the asymmetry is deliberate (a missed drift is
  silent; a false alarm is merely annoying).
- Results written to `docs/judge_calibration_results.md` on each run, with prompt version.

## 8. Provider wiring

Reuse the existing `core/src/ai/` provider stack (Bedrock / OpenRouter / mock). Requirements:
- temperature 0, fixed max tokens, no streaming.
- Every call records: model id, prompt version, input hashes, and cost, into the evidence
  record — verdicts are evidence and must be attributable and reproducible.
- Respects the existing spend cap; a judge run that would exceed it fails cleanly and writes
  no evidence.

## 9. Wiring
- `service.rs`: `judge_clause(req_id, clause)`, `judge_stale_all()`, `judge_accept_proposal(...)`.
- Tauri commands; Myth `Verify` mode `j` → `run_judge`, `w` → `replay_witness`.
- Stale-link requeue: T1's checker marks bond `RequirementModel` stale → the judge queue
  picks it up (manual trigger in v1; automatic on save behind a setting).

## 10. Tests
- prompt renders deterministically for a fixed input (snapshot test)
- strict JSON parsing; malformed → one retry → `Unclear`
- drift verdict without a witness is rejected
- witness confirmed → `judge-drift` finding + seed appended
- witness falsified → verdict discarded, no finding, `judge-unreliable` after two
- `Unmodelable` produces a proposal, not a mutation
- `Agrees` writes L2 only, never touches other bonds
- calibration suite passes with zero false `Agrees` (mock provider)

## 11. Order of work
1 verdict types + parsing → 2 prompt + rendering → 3 provider call → 4 witness execution →
5 evidence + findings → 6 proposals → 7 calibration → 8 backtranslation → 9 wiring.

## Risks
- Requires T2's model runner; if T2 slips, the judge can still run but **without** witness
  execution — in that case it must record verdicts at a reduced trust marker and never emit
  `judge-drift` as an error. Implement that degraded mode explicitly rather than assuming
  the runner exists.
- Provider cost on large requirement sets: batch by staleness, cap per run, report spend.

---

# Revisions after review

## R1 — Witnesses must be executable, so the prompt must ask for a case
The prompt asked for `"input": <json>` with no `op`, while the DRT `Case` dispatches on `op`.
Every witness would have landed in "inexecutable → Unclear", quietly disabling the one
mechanism that makes this judge defensible. The prompt now renders the binding's **declared
input schema** and requires the witness as `{"op": …, "input": …}`, shaped to that schema.

## R2 — `model_produces` must be comparable, not prose
A prose claim compared by "containment heuristics" cannot gate an error-severity finding. The
reply must carry `model_produces_json`, shaped like a DRT `Reply` payload, and comparison uses
`drt::protocol::values_agree`. A witness with only prose is **inexecutable**, never
`confirmed`.

## R3 — Degraded mode, stated once
If no model runner exists, the judge still runs but its verdict is recorded with
`degraded: true` and **never** produces a `judge-drift` finding. `EvidenceDetail::Judge` gains
`degraded`, `confidence`, `comparison_mode` and `cost_usd`, all `#[serde(default)]` because the
lockfile is persisted and older records must keep parsing.

## R4 — The spend cap is not enforced inside the provider
`AiProvider::complete` has no cap; the cap lives in the agent tool loop. The judge therefore
performs its own pre-call check and post-call accounting, following the existing
`summarize_messages` pattern: check the cap, call, then `usage.estimate_cost(...)` and record.
A judge run that would exceed the cap fails cleanly and writes no evidence.

## R5 — No structured output; parse defensively
The provider stack has no `response_format`/`json_schema`/`tool_choice`. The judge parses free
text: strip code fences, take the first balanced `{…}`, then deserialize. A parse failure is
retried **once with the parse error appended** — a bare retry at temperature 0 would reproduce
the same text verbatim.

## R6 — Temperature and model are overridden explicitly
`temperature` and `max_tokens` come from the user's `ModelConfig`. The judge clones the
selected model and sets temperature 0 and a fixed token budget, because a verdict is evidence
and must be reproducible.

## R7 — The judge takes a provider, not a global
`judge_clause` accepts `&dyn AiProvider`, so tests supply a scripted provider that answers per
fixture. The existing mock is interactive and its headless mode returns one fixed string for
every call from a process-global env var — unusable for a calibration suite and racy under
parallel tests.

## R8 — Missing enum and policy members
Add `FindingKind::JudgeUnreliable` (a falsified witness twice over is a fact about the judge,
not about the requirement) and a `high_value` glob to `Policy` for enabling backtranslation.

## R9 — Records must not overwrite each other
`put_evidence` dedupes by `(key, backend)`. Backtranslation must therefore use a distinct
backend label, or it silently replaces the primary verdict.

## R10 — Failure modes to handle explicitly
Truncated responses (`AiResponse.truncated`) are reported as truncation, not as a parse error;
clause text and Lean source are fenced in the prompt and explicitly labelled as untrusted data
so an instruction inside a requirement cannot steer the verdict; a missing provider or API key
is a clean "not configured" error; and drift/`Unclear` records are written at `Level::L1`,
since `level` is mandatory and only `Agrees` earns L2.
