# Open Questions & Risks – TraceLean IDE

## 1. Formal Verification Gap

Converting Lean specs to runnable conformance checks is non-trivial.

- Lean `#eval` tests properties on concrete inputs, but full proof is expensive.
- Translation-validation requires per-language extractors.
- **Mitigation**: Start with property-based testing. Iterate toward full verification.

## 2. Performance at Scale

Real-time spec-check on large codebases may be slow.

- Lean compilation is not instant.
- **Mitigation**: Incremental checking (only changed symbols). Async validation (don't block typing). Parallel parsing on startup (Rayon).

## 3. Language Coverage

Tree-sitter must have grammars for all target languages.

- Most popular languages covered; niche ones may lack grammars.
- **Mitigation**: Initially support Python, Rust, C++, Lean 4. Plugin mechanism for adding grammars.

## 4. Lean ↔ Code Semantic Gap

Lean specs must model side effects explicitly (IO, concurrency, races, deadlocks, resource lifetimes).

- Lean 4's `IO` monad and `Task` primitives can model effectful/concurrent behaviour.
- Specs express: ordering constraints, mutual exclusion, absence of data races, liveness.
- Harder than pure functional specs but Lean's type system handles it.
- **Mitigation**: Monadic specs for effectful code. Property-based tests exercise concurrent scenarios (randomised scheduling). Full formal proofs of concurrency = stretch goal.

## 5. Security

- API keys must be encrypted at rest.
- Source code sent to AI requires explicit consent.
- **Mitigation**: Encrypted keyring storage. Consent dialogs. Option to run local models. Docker network isolation.

## 6. User Adoption

- Lean is unfamiliar to most developers.
- Strict mode may frustrate users.
- **Mitigation**: AI handles most Lean writing. Suggestion mode as default. Clear error messages. Guided onboarding.

## 7. AI Accuracy

- LLMs produce incorrect specs and code.
- Wrong formalisation could give false confidence.
- **Mitigation**: Human-in-the-loop review at every step. AI suggestions are proposals, never auto-committed without user approval.

## 8. Command Log Growth

- Every keystroke is a command. Long editing sessions produce large logs.
- **Mitigation**: Periodic checkpoints (full state snapshot). Prune commands before last checkpoint. Configurable retention policy. Compression of command log on disk.
