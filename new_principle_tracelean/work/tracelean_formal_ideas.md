# TraceLean — formal ideas: proof routes and target languages

*Written 2026-11-11 from a design discussion. Nothing here is built. Status of each claim about an outside tool: from memory or from one reading of its README, not from running it. Verify before building on it.*

## 1. The question

TraceLean has two confidence gaps:

```
requirement ──(1) judged──► Lean model ──(2) tested (DRT)──► implementation
                              ▲ proved + pinned
```

- Gap 1, requirement → formal model. Closed by a person's judgement.
- Gap 2, model → implementation. Closed today by differential testing (DRT): the model and the code agree on generated inputs, with measured coverage.

The DeepMind Signal paper (formal verification where the executable model is the compiled program, over a modelled hardware semantics) has no gap 2, at the price of proofs at machine level. The question is how to make gap 2 stronger than testing without paying that price, and for which languages. The product must support **C and Python** (embedded domain) besides Rust and TypeScript.

## 2. What must not be lost

Whatever closes gap 2, these stay in Lean:

1. **One specification for every language.** One Lean model per clause; every implementation is compared with it.
2. **The strength check (pinning).** Proven means the properties leave one function only: `∀ i o₁ o₂, Props i o₁ ∧ Props i o₂ → o₁ = o₂`. This is a statement about the specification. No code verifier checks it: `ensures true` verifies.
3. **Kernel-checked evidence with declared axioms** for the proved mark.

So a tool that verifies code against contracts written in its own language does not replace Lean. It can only be another way to earn the model → implementation bond.

## 3. The routes

| Route | What it shows | Languages | Evidence kind | Cost |
|---|---|---|---|---|
| **DRT** (today) | code = model on generated inputs, coverage measured | any (a runner each) | tested | low, flat per clause |
| **Bounded model checking**: CBMC (C), Kani (Rust) | code = model for *all* inputs up to a bound; a concrete counterexample otherwise | C, Rust | exhaustive within a bound | low to medium; no invariants to write |
| **Deductive contracts**: Frama-C/WP (C), Verus (Rust) | code meets a contract for all inputs, unbounded | C, Rust | proved by SMT | medium to high: loop invariants, ghost code |
| **Translate and prove in Lean**: Aeneas (Rust → Lean) | translated code = model, as a Lean theorem | Rust (safe subset) | proved by Lean's kernel, trusting the translator | high per function |
| **Semantic framework in Lean**: Strata | code's formal semantics lives in Lean; SMT, model checking or Lean proof on it | Python today; no C front end found | depends on back end | unknown; young, breaking changes announced |
| **Machine level** (Signal paper) | compiled program meets the spec | any, in principle | proved, smallest trusted base | very high; ruled out for agent/LLM cost |

Naming note: the Rust model checker is **Kani** (built on CBMC). "Kami" is a different tool (hardware, in Coq).

## 4. Per language

### C — the weakest point of DRT, so the first place to go beyond it

Why DRT is weak for C: two runs agreeing says nothing when the code has undefined behaviour that happens to behave; memory errors, aliasing and overflow are exactly what random inputs miss; host runs hide word size, endianness and integer promotion of the target.

Proposed order:

1. **DRT, hardened.** Sanitizers (ASan, UBSan) on the C side, a sanitizer report counted as a divergence. Branch coverage (gcov/llvm-cov). The record names the target (host, QEMU, board).
2. **CBMC as "exhaustive DRT".** Same harness shape as DRT: call the function on a nondeterministic input, assert the output equals the model's. CBMC checks every input up to the bound and also checks memory safety, overflow and UB on the way. A failure is a concrete input, which fits the existing divergence/shrink flow. Open problem: the Lean model must be available to CBMC, as C. Options: compile the model to C (Lean already emits C, but with its runtime; unclear if CBMC can digest it), or restate the model as a C reference function and DRT *that* against Lean.
3. **Frama-C for what bounds cannot reach.** Eva (abstract interpretation) for absence of runtime errors on whole programs; WP with ACSL contracts for unbounded functional correctness. E-ACSL can execute a contract, so a contract can itself be tested against the Lean model on DRT's inputs.

### Rust

1. **DRT** stays the default.
2. **Kani** is the preferred next step (the user's leaning, and it is sound reasoning): no invariants to write, precise counterexamples, same harness shape as DRT, so an agent can produce it. It is bounded: loops and recursion need an unwinding bound, and the record must state it.
3. **Verus** when an unbounded proof is needed and someone will write invariants. SMT evidence, Rust-specific spec language.
4. **Aeneas** when the proof should be a Lean theorem about the model itself. Safe-Rust subset only; each equivalence is real proof work. Worth it only for pinned models.

### Python

DRT only, realistically. Strata has a Python front end (through its Laurel dialect); maturity and supported subset unknown.

### TypeScript

DRT only.

## 5. The contract ↔ model link

For CBMC, Kani, Frama-C and Verus the property is written in the tool's language, not in Lean. Someone must show it says what the Lean model says. Cheapest first:

1. **Generate it from the model.** For pure functions over integers and bounded arrays the tool writes the harness or `ensures`, so nothing is compared by hand. For CBMC/Kani the harness is "output equals model's output", which needs the model callable from C/Rust.
2. **Test it against the model.** Run the contract as an executable predicate on DRT's generated inputs and check it accepts exactly the model's outputs. DRT applied to the spec.
3. **Judge it**, like gap 1.

Without one of these, a verified contract proves the code meets *a* contract, not the requirement.

## 6. Spec-only route (no executable Lean model)

Idea: keep only definitions and theorems in Lean, translate the Rust with Aeneas, prove the theorems on the translation. No second implementation, no gap 2.

- Gain: P becomes "the property holds of the real code".
- Loss: pinning has no function to compare with; it must be restated over a relation `Spec i o` (code satisfies it, and it is functional). The obligation generator would be rewritten.
- Loss: no executable oracle, no independent second writing of the behaviour; judging a relational spec is harder than judging a short function.
- Rust only.

Verdict: a second route per clause, good where the natural spec is a relation; keep the executable model where the clearest spec is an algorithm.

## 7. Strata, and "what if Strata supported C and Rust well?"

What Strata is (README, 2026-11): an AWS open-source platform in **Lean 4**. Source language → Laurel → Core → verification conditions → SMT (cvc5, z3), with a path from Core to CBMC. Because the semantics is in Lean, Lean proofs about programs are possible in principle. Python front end exists. No real C front end found (there is a small C-like dialect and a CBMC *back end*). Active development, breaking changes announced.

If Strata had solid C and Rust front ends, it would be close to the ideal shape:

- one Lean semantics per language, so the implementation becomes a Lean object next to the model;
- gap 2 could be closed by whichever technique fits the clause: SMT, bounded model checking, or a kernel-checked Lean proof, all on the same object;
- the model ↔ contract problem of section 5 disappears, because the model and the code's semantics are in the same logic.

What it costs to get there:

- A C front end with trustworthy semantics is a multi-year effort (see CompCert, VST, Frama-C's kernel). UB, the memory model, integer promotion and the preprocessor are the work. Rust needs MIR-level semantics, ownership and traits.
- The trusted base becomes "the front end's translation and semantics are faithful", the same kind of trust as Aeneas or as the paper's hardware model, one level up.
- Building on it now means following an API that announces breaking changes.

So: contributing C to Strata is a research programme, not a product step. Track it; do not depend on it.

## 8. Is there a holy grail?

The ideal is: **one specification, proved strong, in one logic; every implementation in any language given a formal semantics in that same logic; each clause discharged by the cheapest technique that suffices, with the technique recorded.** Strata-with-every-language is that ideal. The Signal paper is the same ideal one level lower (machine code), with the smallest trusted base and the highest cost.

Nobody has it for C + Rust + Python today. What can be built now is the same *shape* with separate tools behind one bond:

```
clause ──► Lean model (judged, proved, pinned)          single source of truth
              │
              ├─ DRT                         any language   tested, coverage measured
              ├─ CBMC / Kani                 C / Rust       exhaustive within a stated bound
              ├─ Frama-C WP / Verus          C / Rust       proved by SMT, unbounded
              └─ Aeneas (later Strata)       Rust (Python)  proved in Lean
```

The honest product claim: *the specification is proved strong in Lean; each implementation is bound to it by the strongest technique available for its language, and the mark says which one, with which tool, version, bound and target.* This is close to back-to-back testing plus formal methods supplements as safety standards describe them (ISO 26262, DO-178C/DO-333), which matters for the embedded market.

Design consequence for TraceLean: the model → implementation bond needs **grades**, not a single T. Proposed, weakest first: tested (DRT) · exhaustive to bound *k* (CBMC/Kani) · proved by SMT (Frama-C/Verus) · proved in Lean (Aeneas/Strata). Each record carries tool, version, bound, target and the way the contract was tied to the model (generated, tested, judged). A tool upgrade makes the record stale, like a toolchain change today. `REQ-EVID.marks` and `REQ-STRENGTH.kernel_decides` would need new clauses so that an SMT or bounded verdict is never shown as a kernel proof.

## 9. Experiments, smallest first

1. **C runner for DRT** with sanitizers, on one pure function. Decides the input encoding (pointers, lengths, out-parameters). Needed for everything else in C.
2. **CBMC on the same function**: harness "output = reference", reference being the Lean model restated in C and itself DRT'd against Lean. Measures how far bounds go and whether counterexamples feed the divergence flow.
3. **Kani on two or three Rust decision functions** of this repo (`earns_proof`, `setting::zoomed`, `quick::matches`), same harness shape. Compare effort and findings with their existing DRT.
4. **Frama-C WP + E-ACSL** on the C function: write the ACSL contract, prove it, and test the contract against the Lean model with E-ACSL.
5. **Aeneas** on the same Rust functions: how much of `crates/core` it accepts, and what one equivalence proof costs.
6. **Strata, one day**: one small pure Python function through Strata; can a clause's property be stated against the result in Lean? That decides whether Strata is ever the Lean-side home for implementations.

Experiments 1–3 are the ones an agent can do cheaply and that change the product soonest.

## 10. Open questions

- Can the Lean model be made callable from a C/Rust harness without restating it by hand (Lean's C output under CBMC; a generated reference)?
- State machines and time: embedded logic is stateful. DRT and BMC both fit a step function driven by input *sequences*; timing and interrupts stay outside every route above and the tool must say so.
- Where does the pinning obligation live if a clause takes the spec-only route (section 6)?
- How are grades shown without implying an order where there is none (bounded-exhaustive versus SMT-unbounded are not strictly comparable)?

## Sources

- Strata: https://github.com/strata-org/Strata (README read 2026-11-11)
- Signal paper: https://deepmind.google/documents/36/Formal_Verification_of_Signal_4.pdf (as described by the user; not read in this session)
