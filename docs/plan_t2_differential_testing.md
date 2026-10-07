# T2 — Differential testing (model ↔ implementation): implementation plan

Design reference: `docs/formal_traceability_and_lsp_plan.md` §5, §7.
Depends on T1 (annotations `@models`, `@implements`, `@drt`; evidence records).

## The bond
Run the executable Lean reference model and the real implementation on the same generated
inputs; require identical outputs. Language-independent because both sides are subprocesses
speaking one JSON protocol.

## 1. Conformance protocol

Line-delimited JSON on stdin/stdout. One line in, one line out, same `case` id.

```
→ {"case": 41, "op": "login", "input": {"username":"ana","password":"hunter22"}}
← {"case": 41, "output": {"Err": {"weakPassword": {}}}}
← {"case": 42, "error": "panic: index out of bounds"}          // runner-level failure
```

Rules:
- `op` selects the entry point when a binding exposes several.
- Either `output` or `error`, never both. `error` on one side and `output` on the other is a
  divergence, not a crash of the run.
- Runners are long-lived: spawn once, stream many cases. Restart on runner death, and mark
  the case that killed it as a divergence candidate.
- Ordering is not assumed; match on `case`.

Rust side: `core/src/drt/protocol.rs` — `Case`, `Reply`, `Runner` (spawn, send, recv,
timeout per case, kill).

## 2. Model runner (generated)

For a `@models` anchor, generate `<lake_project>/drt/<ReqId>Runner.lean`:

```lean
import <ModelModule>
def handle (c : Case) : Reply := ...   -- dispatch on c.op
def main : IO Unit := do
  let stdin ← IO.getStdin
  repeat (read line → fromJson → handle → toJson → putStrLn)
```

- JSON encoding comes from `deriving ToJson, FromJson` on the model's types (Lean core
  `Lean.Data.Json`). If the model's types lack the deriving clause, emit a clear error
  telling the user to add it — do not silently hand-write encoders.
- Built with `lake build`; binary path cached with its input hashes.
- Regenerate when the model anchor hash changes.

`core/src/drt/lean_runner.rs`: generation, `lake build` invocation, build-error surfacing,
binary cache in `.tracelean/drt/`.

## 3. Implementation adapter (hand-written, per binding)

The only thing a user must write. Small: decode, call, encode.

- Discovered by its `@drt REQ-ID` annotation, so it is anchored and drift-checked like
  everything else.
- Declared in `.tracelean/drt.json`: `{ "REQ-AUTH-03": { "adapter_cmd": ["cargo","run","--bin","auth_drt"], "cwd": "." } }`
- TraceLean generates a **skeleton** adapter on request (`drt_scaffold` command) for Rust
  and Python: reads stdin lines, parses JSON, `// TODO call your function`, writes JSON.
  Reduces the friction that is the approach's main cost.

## 4. Generators (`core/src/drt/gen.rs`)

Three sources, combined per run:

1. **Type-directed** — from a JSON schema of the model's input type. Emit the schema from
   the Lean side (`--emit-schema` mode in the generated runner) so the generator never
   parses Lean. Supports: Nat/Int (with boundary bias 0, 1, max, overflow-adjacent),
   String (empty, unicode, long, control chars), Bool, Option, List (empty/1/large),
   structures, inductives (every constructor, uniformly).
2. **Seed corpus** — witnesses confirmed by the judge (T3), past divergences, user-supplied
   cases in `.tracelean/drt/seeds/<REQ-ID>.jsonl`. Always replayed first.
3. **Mutation** — small perturbations of seeds (off-by-one, empty, boundary swap).

Deterministic: a run is `(seed: u64, count: usize)`; same seed → same cases. No wall-clock,
no thread-dependent ordering.

**Shrinking**: on divergence, shrink by structural reduction (smaller numbers, shorter
strings/lists, earlier constructors) while the divergence persists. Report the minimal case.

## 5. Model-branch coverage

Case counts without coverage are theatre (design §5.2). Implementation:
- Generated runner is built with a **branch-trace mode**: the model is instrumented by a
  simple source transform that records `(decl, branch_id)` hits into a set, emitted at exit.
  If instrumentation proves too invasive for v1, fall back to *constructor coverage* of the
  input type plus *output-variant coverage* of the result type, which needs no
  instrumentation and is still a real diversity signal.
- Report `covered/total` next to the case count; refuse to record L3 evidence if coverage is
  below a configurable floor (default: all output variants seen at least once).

## 6. Runner orchestration (`core/src/drt/run.rs`)

```rust
pub struct DrtRun { pub req_id: String, pub seed: u64, pub cases: usize, pub timeout_ms: u64 }
pub struct DrtResult {
    pub cases_run: usize, pub divergences: Vec<Divergence>,
    pub coverage: Coverage, pub model_hash: String, pub impl_hash: String,
    pub adapter_hash: String, pub lean_version: String, pub duration_ms: u64,
}
```
Streams cases to both runners concurrently, compares replies, shrinks failures, writes an
evidence record (level L3 if zero divergences and coverage floor met).

## 7. Divergence triage (design §5.3)

A divergence is a finding, never an automatic code bug. Stored as `Divergence { case, model_out,
impl_out, shrunk_from }` with triage state `Untriaged | ImplBug | ModelBug | AdapterBug`.
Triage is a user/agent action recorded as a `Command`; `ModelBug` additionally marks the
requirement↔model link stale so the judge re-runs (T3).

## 8. Wiring
- `service.rs`: `drt_run(req_id, seed, cases)`, `drt_scaffold(req_id, lang)`, `drt_triage(...)`.
- Tauri commands mirroring those; Myth `Verify` mode `d` → `run_drt`.
- CI entry point: `tracelean-trace drt --all` (new bin or subcommand) for the gate.

## 9. Tests
- protocol round-trip; runner death mid-stream; per-case timeout
- generator determinism (same seed → identical case list)
- shrinker reduces a known divergence to the minimal case
- a fixture Lean model + a fixture Rust impl that agree → L3 evidence written
- same fixture with a deliberately broken impl → divergence with the expected shrunk input
- evidence record refuses L3 when coverage floor is unmet
- model hash change invalidates the cached binary

## 10. Order of work
1 protocol → 2 model runner generation + `lake build` → 3 adapter scaffold → 4 generators →
5 orchestration + evidence → 6 shrinking → 7 coverage → 8 triage → 9 wiring.

## Risks
- **No Lean toolchain on the dev machine.** Everything Lean-dependent must degrade to a
  clear "toolchain not configured" state, and the tests must be skippable/gated on `lean`
  being present, so `cargo test --workspace` still passes without it.
- Lean `deriving ToJson` may not exist for every type shape the user writes — the error path
  matters as much as the happy path.
- Adapter authorship is the user-visible friction; the scaffold generator is not optional.

---

# Revisions after review

## R1 — The derived JSON encoding, stated exactly
Lean's `deriving ToJson` (`Lean/Elab/Deriving/FromToJson.lean`) encodes:
nullary constructor → the bare string `"weakPassword"`; one unnamed field → `{"Err": v}`;
n unnamed fields → `{"C": [a, b]}`; named fields → `{"C": {"f": …}}`; structures → a plain
object; a field named `x?` is **omitted** when `none`. The plan's
`{"Err": {"weakPassword": {}}}` cannot occur. Consequences:
- Comparison is over parsed `serde_json::Value`, never strings.
- An absent `x?` field compares equal to `null`.
- A golden fixture records the encoding so a Lean upgrade that changes it is caught.

## R2 — Flush every line, on both sides
Lean's stdout is block-buffered on a pipe: the generated runner must
`let o ← IO.getStdout; o.putStrLn s; o.flush` per case, and the Rust side must write, flush,
then read under a per-case timeout. Without this the very first case deadlocks.

## R3 — No `--emit-schema`; the shape is declared, not reflected
Lean has no runtime type reflection. Emitting a schema needs a `MetaM` metaprogram
(`getConstInfo`, `InductiveVal.ctors`, `getStructureFields`) and is undecidable for type
parameters and dependent types. **v1 declares the input shape in `.tracelean/drt.json`**
using a small whitelisted grammar — `nat | int | bool | string | option<T> | list<T> |
struct{…} | enum{…}` — and hard-errors on anything outside it. Reflection is a later
optimization, not a prerequisite.

## R4 — Number precision
`serde_json` without `arbitrary_precision` collapses a large `Nat` to `f64`, which would make
two different numbers compare equal. Rather than enabling that feature crate-wide (it changes
`Number` representation for every other user of serde_json in this workspace), the comparator
**canonicalizes numbers to their decimal string form** before comparing.

## R5 — Generated Lean package, with legal names and the right binary path
`REQ-AUTH-03Runner` is not a legal Lean module name. Generate a *separate* package under
`.tracelean/drt/` with `require <pkg> from "<path>"` and `lean_exe drtRunner where root := …`,
sanitizing requirement ids to CamelCase. Probe both `.lake/build/bin/` (Lean ≥ 4.3) and
`build/bin/`.

## R6 — Everything is keyed by `(req_id, clause)`
`Link.clause` and `EvidenceKey` are per-clause; a run keyed by requirement alone cannot write
the record the checker reads. `drt_run`, `drt.json` bindings and evidence keys all carry the
clause.

## R7 — `DrtResult` maps onto `EvidenceDetail::Drt` exactly
The existing enum has `seed`, `cases`, `divergences`, `coverage_covered`, `coverage_total`,
`lean_version` — no `adapter_hash` or `duration`. Hashes belong in `input_hashes` under the
keys `model` / `impl` / `adapter` / `clause` that `TraceIndex::current_hashes` produces.

## R8 — Reuse what exists
- `ai/mcp_client.rs::McpConnection` is already a long-lived NDJSON-over-stdio child process:
  copy its shape, but fix its two defects — no read timeout, and `stderr(null)` throws away
  the panic text needed to fill an `error` reply.
- `ai/shell_sandbox.rs::run_process(cmd, timeout_secs, cancel_flag)` already does
  spawn + drained pipes + timeout + cancel: use it for `lake build`.
- `requirements.rs::find_lean_binary()` already probes elan; extend it to `lake`.

## R9 — No proptest for generation
`proptest`'s shrinker is typed and its `proptest-regressions` files fight the "seed → identical
case list" guarantee. Generation and shrinking are hand-rolled over `serde_json::Value` with a
small deterministic PRNG defined inline (no new dependency). `proptest` stays where it is, in
unit tests.

## R10 — Decisions on the underspecified points
- **Equality**: parsed `Value` equality after number canonicalization; absent ≡ `null`.
- **In-flight window**: exactly one case at a time per binding. Simple, and the bottleneck is
  the implementation under test, not the pipe.
- **Restart policy**: at most 3 restarts per run; the case that killed a runner is recorded as
  a divergence candidate and counts as run.
- **Shrink budget**: at most 200 reduction attempts per divergence, always terminating.
- **`op` names**: declared in `.tracelean/drt.json`; the default is `"default"`.
- **Coverage floor**: `drt.json`'s `coverage_floor`, default "every output variant seen".
- **Hash inputs**: exactly the keys `TraceIndex::current_hashes` emits.

## R11 — No Lean source instrumentation
Rewriting `match`/`if` in the user's model would break `termination_by`, derived instances and
proofs, and would change the anchor hash of the very thing being measured. Dropped entirely.
v1 ships **input-constructor and output-variant coverage**, computed on the Rust side from the
declared schema and the observed replies — no instrumentation, and still a real diversity
signal.

## R12 — Testability without a Lean toolchain
There is no `lean`/`lake` on the development machine, so every Lean-dependent step degrades to
a clear "toolchain not configured" error, and the test suite exercises the full subprocess
path using small **Python** runners as stand-ins for both sides. That tests the protocol,
generator, comparator, shrinker, restart and timeout logic for real, and leaves only Lean
codegen itself gated behind a toolchain check.
