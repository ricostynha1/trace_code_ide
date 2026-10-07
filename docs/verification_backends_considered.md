# Verification backends considered (and not taken)

*Companion to [`formal_traceability_and_lsp_plan.md`](./formal_traceability_and_lsp_plan.md),
which describes the path TraceLean is actually taking: a natural-language requirement, an
executable Lean reference model, and differential testing that binds the model to the
implementation in whatever language it happens to be written in.*

**This document is the research archive behind that decision.** Everything here was
investigated and deliberately *not* adopted, or deferred. Keep it so the decision can be
revisited on evidence rather than re-argued from scratch — each section ends with what
would have to change for the answer to change.

**The decision in one line:** the bond between the model and the code is *behavioural*
(differential testing), not *syntactic* (translation or shared source), because that is the
only bond that works on arbitrary code in any language — and because a spec that lives
inside the implementation is not an artifact a requirement can point at.

---

## The landscape, examined

### 1. Strata — is a Rust front-end coming?

You pushed back on my first draft, so I went into the repository rather than the README.

**What exists** ([`Strata/Languages/`](https://github.com/strata-org/Strata/tree/main/Strata/Languages)):
`B3`, `C_Simp`, `Core`, `Dyn`, `GOTO`, `Laurel`. Across the org
([10 repos](https://github.com/orgs/strata-org/repositories), all active as of today):
[`Strata`](https://github.com/strata-org/Strata), `Strata-CLI`,
[`Strata-Python`](https://github.com/strata-org/Strata-Python),
[`Strata-Boole`](https://github.com/strata-org/Strata-Boole), `Strata-Boogie`,
`Strata-DDM`, `Strata-Generators`, `jverify` (Java program verification), `specimen`.
**No Rust dialect, no Rust repo.** A
[search of the issue tracker for "rust"](https://github.com/strata-org/Strata/issues?q=rust)
returns only coincidental matches on Boole PRs.

**But two signals point at Rust anyway, and they're worth knowing:**

- **`GOTO` is CBMC's IR.** [The file](https://github.com/strata-org/Strata/blob/main/Strata/Languages/GOTO.lean)
  says so outright: *"the GOTO intermediate language
  (CBMC's `CProverGOTO` program model) together with the translations that produce it from
  other Strata languages"*, with `Imperative`/`Lambda`/CFG → GOTO translations and a
  separate `Strata.Backends.CBMC` that serializes to CBMC's JSON and invokes the tool.
  **[Kani](https://arxiv.org/html/2607.01504) — AWS's own Rust model checker, running in
  production CI on Firecracker and s2n-quic — lowers Rust to exactly that
  representation.** A Rust → Kani → GOTO → Strata
  path is therefore *architecturally* available; nobody has announced building it. Treat
  this as a strong hint about direction of travel, not as a shipped feature. (My
  inference.)
- **[`Boole`](https://github.com/strata-org/Strata-Boole) — "a high-level imperative
  specification language for deductive program verification", a friendlier front-end to
  Strata Core — is being extended "for dalek-lite benchmarks"
  ([merged PR #1075](https://github.com/strata-org/Strata/pull/1075)). `dalek` is
  the Rust curve25519 crypto family — so Rust-derived workloads are already being modelled
  in Strata by hand, which is how a front-end usually gets justified.

**So my original claim needs qualifying, not retracting:** there is no Rust front-end you
can use today, and if you adopt Strata now you are committing to model your Rust by hand
in Boole/Laurel. The expansion is real and Rust-adjacent. The right posture is an adapter
plus a quarterly check of `Strata/Languages/`, not a bet.

Also unchanged: Strata is pre-1.0 with an explicit breaking-changes warning, has no LSP,
and its Python front-end lives in a separate repo. Integration is Strata-CLI or the Lean 4
API.

### 2. The answer to "industry is moving to Lean 4, but Rust is where the fast code is"

This is the right tension to feel, and it has a production answer that doesn't require
choosing.

**AWS Cedar's verification-guided development** — sources:
[Lean Powers Secure Software at AWS: Cedar's Journey](https://lean-lang.org/use-cases/cedar/),
[How We Built Cedar: A Verification-Guided Approach](https://arxiv.org/pdf/2407.01688)
([Amazon Science PDF](https://assets.amazon.science/d3/86/99db1aa142ffb6981d86dc849e4c/how-we-built-cedar-a-verification-guided-approach.pdf)),
[Lean Into Verified Software Development (AWS Open Source Blog)](https://aws.amazon.com/blogs/opensource/lean-into-verified-software-development/):

1. Write an **executable model in Lean** — about **10× smaller** than the production code,
   and doubling as mathematical documentation of the intended behaviour.
   ([lean-lang.org/use-cases/cedar](https://lean-lang.org/use-cases/cedar/))
2. **Prove the correctness and security properties about the Lean model** — small, clean,
   tractable proofs, because the model is small.
   ([arxiv 2407.01688](https://arxiv.org/pdf/2407.01688))
3. Bridge to the real implementation with **differential random testing**: generate
   millions of random inputs, run both the Lean model and the production **Rust**, require
   identical output. (Lean executes a case in ~5µs against Rust's ~7µs — fast enough that
   this is a CI job, not a research project.) Add property-based testing for the
   properties that have no model analogue.
   ([lean-lang.org/use-cases/cedar](https://lean-lang.org/use-cases/cedar/))
4. Run **[Kani](https://arxiv.org/html/2607.01504)** on the Rust for panic/memory safety —
   it found a real Cedar bug where `contains_at_least_two` sliced a `&str` across a
   non-character boundary on multibyte input. Kani runs in production CI on Firecracker
   and s2n-quic, and drives the
   [Rust standard library verification effort](https://aws.amazon.com/blogs/opensource/verify-the-safety-of-the-rust-standard-library/).

Why this matters for you specifically:

- **It keeps Lean at the centre** — which is your brand, and where industry momentum
  genuinely is (Cedar, Strata, the AWS Lean push).
- **It works on Rust you did not write for a verifier.** No extraction subset, no
  borrow-checker encoding, no failure on generics-with-trait-bounds or external crates —
  the exact bottlenecks the 2026
  [Rust-to-Lean Verification Pipeline with AI Provers experience report](https://arxiv.org/html/2605.30106)
  lists as its main tooling pain, alongside Lean version drift across
  Aeneas / hax / ArkLib / Mathlib4 and cross-library name collisions. (That report is also
  the best available evidence on AI provers: Aristotle and Aleph handled control-flow
  lemmas, mild linear arithmetic and extraction boilerplate, but **domain-specific
  algebraic identities and loop invariants stayed manual** — "a productivity multiplier",
  not an eliminator. The Lean kernel re-checks every AI proof, so unsound AI output cannot
  compromise the result.)
- **It is implementable by TraceLean now.** The Lean spec is already executable; the
  missing pieces are a generator harness and a runner. That's ordinary engineering, not
  research.
- **It produces evidence with a number** — "12M cases, 0 divergences, seed 0x…, at commit
  X" — which is exactly the kind of artifact the evidence store in the main design wants.

The honest limitation: DRT is falsification, not proof. It buys you L3, not L4. Say so in
the UI. But L3-on-real-Rust beats L4-on-a-subset-you-had-to-rewrite, and it's how the
people furthest ahead on this actually ship.

### 3. Where the other options sit now

| Backend | Role | Why |
|---|---|---|
| **Lean model + DRT/PBT** | **Primary.** The Cedar pattern: Lean holds model + proofs, differential testing binds it to real Rust/Python | Works on arbitrary code; keeps Lean central; shippable now |
| **Verus** | In-code contracts for hot Rust paths that deserve SMT proof | Spec lives in the code ⇒ no translation gap; linear verification time; `verus-analyzer` + proof actions |
| **hax / Aeneas** | Deep track for modules that warrant real proof | `cargo hax into lean`, `hax_lib::requires/ensures`, proof scenarios in `hax.toml`; charon+aeneas auto-managed. Costs: Rust subset, version drift, interactive proof |
| **Kani** | Cheap safety net on Rust | Panic/UB/overflow harnesses; already production-proven at AWS; pure win, low effort |
| **Dafny** | Greenfield spec-first modules; the easiest full-loop demo | 82% LLM vericoding vs 44% Verus vs 27% Lean |
| **Strata / Laurel** | Tracked, adapter-only spike | Lean-native, AWS-backed, MLIR-style dialects — but pre-1.0, no Rust front-end, no LSP |

The vericoding numbers stay worth repeating because they set expectations: Lean is the
hardest target for automated proof, **and adding natural-language descriptions does not
significantly improve vericoding success**. The NL layer earns its keep through
traceability, intent capture and review — not by helping the prover. Plan the pitch
accordingly.

### 4. Where does the formal spec *live*? — the question that decides your middle layer

You asked exactly the right question about Strata-Python: are the contracts inserted into
the Python source, or does Strata build metadata beside the code? I looked it up, and the
answer matters far more than it first appears, because **it decides whether your
"NL → formal spec → code" mapping has a middle layer at all.**

**What Strata-Python actually does** ([Strata-Python](https://github.com/strata-org/Strata-Python)):
contracts are **not** inserted into your production Python. They are written as Python
*decorators* in **separate spec files** (`.pyspec.st.ion`), e.g.

```python
@admit(lambda result: result >= 0)     # unverified assumption, bodyless declaration
def modeled_value() -> int: ...

@ensures(lambda result: ...)           # claim to be VERIFIED against an implementation
def some_service_call(...): ...
```

Inside a PySpec body, `assert P` contributes to the precondition, and `all(P(x) for x in
xs)` / `any(...)` lower to ∀ / ∃ over lists, dict keys, values and items (with `if`
guards). The pipeline is **Python → Laurel → Core → SMT**, entered via `pyAnalyzeLaurel`.
Note the deliberate asymmetry: `@admit` is an assumption you're granted, `@ensures` is an
obligation that must be discharged — Strata rejects a *modeled* `@ensures` with a fatal
`unsupportedPostcondition`, i.e. it refuses to let you quietly assume the thing you were
supposed to prove. That is a good design instinct and worth copying in TraceLean's own
gates.

**So Strata sits in the "sidecar spec" camp, and that is good news for you.** Here is the
taxonomy that should drive the backend choice — not automation percentages, but *where the
spec physically lives*:

| Kind | Examples | Where the spec lives | What it does to your 3-layer mapping |
|---|---|---|---|
| **A — Fused** | Verus, Dafny, `hax_lib::requires/ensures`, Kani harnesses | *inside* the implementation source | **The middle layer collapses.** "NL → formal → code" becomes "NL → code-with-contracts". Spec and code cannot drift (they're the same file) — which is simultaneously the strength and the reason there is nothing left to trace *between* |
| **B — Sidecar** | **Strata-Python `.pyspec` files**, JML/ACSL stubs, Lean spec files | separate artifact, bound to code by name/signature | **Three layers preserved.** Spec and code *can* drift, therefore drift is *detectable* — and detecting it is precisely what TraceLean's checker owns |
| **C — Model + behavioural bond** | **AWS Cedar: Lean model + differential testing** | separate artifact, possibly in a different language and shape | **Maximum independence.** The bond is behavioural equivalence, not syntax; no extraction subset, and the model doubles as executable documentation |

**The conclusion I'd draw, and it's the sharpest thing in this document:**

> The backends that maximize proof automation (kind A) are exactly the ones that delete
> your product's middle layer. The backends that preserve it (B, C) are where TraceLean
> adds value that a compiler cannot.

That reframes the whole backend question. It is *not* "which prover is strongest" — it is
"which arrangement leaves a formal artifact that a requirement can point at, and that can
be shown to have drifted from the code". By that test, your existing instinct — a
standalone Lean spec file per requirement — was right all along, and the thing to fix is
not the artifact but the **missing bond** between it and the code — which is what the
main design's differential testing provides.

**And you can still use kind-A backends without losing the middle layer** — treat the
in-code contract as a **projection** of the sidecar spec, not as the spec:

```
REQ-AUTH-03.post          (natural language, markdown)
   └── @formalizes →  specs/auth_model.lean :: login_returns_token_iff_valid
                            ├── projection → Verus `ensures` on src/auth.rs::login   (L4 evidence)
                            ├── projection → DRT harness model↔impl                 (L3 evidence)
                            └── projection → proptest oracle                        (L3 evidence)
```

Each projection is *generated from* the Lean statement, carries an `@formalizes` annotation
pointing back at it, and is re-checked for agreement with its source. The Lean spec stays
the single traceable node; Verus/Dafny/Strata become evidence producers underneath it. You
get the automation of kind A **and** the traceability of kind B/C, and the projections are
themselves a place where drift is detectable ("the Verus `ensures` no longer matches the
Lean theorem it was generated from").

One caution on Strata specifically, in light of this: its sidecar spec is written in
*Python-decorator syntax*, which means adopting it makes your formal layer Python-shaped
rather than Lean-shaped, even though the engine underneath is Lean. For a project called
TraceLean whose users are being sold a Lean story, that is a real mismatch — another
reason to keep Strata as a tracked adapter rather than the centre of the design.

### 5. "Should the model be a *Strata* model instead of a plain Lean model?"

Your instinct that AWS is pushing Strata as the next big intermediate verification
framework is reasonable — it is Lean-native, MLIR-style, and carries AWS, Google,
Microsoft, Mistral, Nethermind and Galois behind it. But the proposed pipeline
(*NL → Strata model → differential-test the Rust against it*) has a problem that is
structural, not a maturity complaint. Two separate objections, in order of severity.

#### Objection 1 (fatal, and the interesting one): a Strata model is not *independent* of
the code, so differential testing it proves nothing

DRT gets its power from **independence**. Cedar's Lean model is a *re-implementation of
the intended behaviour*, written from the specification, by people working from the spec
rather than from the Rust. When millions of random inputs agree, that is meaningful
precisely because two independent artifacts derived from the same intent agree.

A Laurel/Boole/Core program is a different kind of object: it is **an encoding of the
implementation's semantics for deductive verification**. It is *derived from* the code —
that is its whole job. Differential-testing your Rust against a Laurel transliteration of
your Rust tests **the transliteration**, not the intent. You would get a green dashboard
that means "my encoding matches my code", which is a fact about your compiler, not about
whether the software does what the requirement says.

The two artifacts occupy different slots and are complementary, not substitutes:

| | Reference model (Cedar-style Lean) | IVL program (Laurel / Boogie / Why3) |
|---|---|---|
| Derived from | the **requirement** | the **code** |
| Answers | "does the code do what we meant?" | "does the code satisfy these contracts?" |
| Independence from impl | **total** — that's the point | **none** — that's the point |
| Bound to code by | differential testing / PBT | compilation + SMT |
| Readable by | domain experts | verification engineers |
| Traceable node in the graph | **yes** — this is the middle layer | no — it's machinery under an obligation |

So the Strata model cannot take the middle layer's slot. Put it under the *code* side,
where it belongs.

#### Objection 2 (practical): deep embedding means the model is interpreted, not compiled

Cedar's Lean model is **shallow-embedded** — ordinary Lean functions that Lean's code
generator compiles to real machine code, which is why a DRT case costs ~5µs and millions
of cases fit in CI.

Strata's Core is a **deep embedding**: inductive ASTs (`Statement.lean`, `Expressions.lean`,
`Program.lean`) with separate semantics functions (`StatementEval.lean`, `ProgramEval.lean`,
`CmdEval.lean`, `StatementSemantics.lean`, `InstWellFormedSemanticsEval.lean`). A Strata
"model" is therefore an AST walked by an interpreter that was written for *proof
convenience* — environments as maps, `Option`/`Except` plumbing, well-formedness side
conditions — not for throughput. Expect orders of magnitude slower than compiled Lean, on
the operation you need to run millions of times. Strata's own package page puts it plainly:
it **"emphasizes verification rather than execution"**. It is at **v0.1.0** (Lean v4.29.1,
updated 2026-09-04).

#### The version of your idea that *is* right

Keep the slots separate and Strata becomes a strong long-term bet in the slot it fits:

```
requirement (NL, markdown)
   │  @formalizes
   ▼
Lean reference model  ──── DRT / PBT ────►  Rust / Python implementation   (L3)
(shallow, executable,                              │
 independent, traceable)                           │  contracts projected
   │                                               │  from the model's clauses
   └──────────── proofs about the model ───────►   ▼
                          (L4)                Strata / Verus / Kani / Dafny
                                              discharge them via SMT        (L4)
```

- The **Lean reference model stays the middle layer** — it is the artifact a requirement
  points at, the thing a domain expert can read, and the only piece that is independent
  enough for DRT to mean anything.
- **Strata sits under the code side as an L4 obligation discharger**, behind the
  `VerificationBackend` trait, interchangeable with Verus/Kani/Dafny.

And in that slot Strata has a genuine advantage over everything else in the table:
**it is the only candidate that could cover Python *and* Rust with one adapter** — a Python
front-end exists today, and the GOTO/CBMC path (see part 1 above) is a plausible Rust route. Verus is
Rust-only; Dafny requires a rewrite. For a multi-language IDE, that's the strongest
long-term argument for Strata, and it is an argument about the *backend*, not the model.

#### One cheap hedge worth taking now

Write the Lean reference model in a **restricted style**: first-order, total, no dependent
types in the executable core, proofs kept in separate files from the model itself. That
style is (a) the fastest to execute, so DRT scales; (b) the easiest to property-test; and
(c) **mechanically translatable into Laurel/Boole later** if Strata does become the
standard. It costs nothing today and keeps the Strata door open.

Worth saying explicitly: if Strata wins, your Lean investment is *not* stranded. Strata is
written in Lean, so a future TraceLean could relate your reference model to a Laurel
encoding **in-process, inside Lean**, rather than shelling out to a prover. Plain Lean is
the most portable asset in this entire landscape. That is the real reason to put the model
there rather than in whichever IVL currently looks ascendant.

---

---

## What would change these decisions

| If this happened | Reconsider |
|---|---|
| Strata ships a Rust front-end (watch `Strata/Languages/` for a Rust dialect, or a Kani→GOTO bridge) and reaches 1.0 | Strata as the L4 obligation discharger under the code side — it would then be the only backend covering Python *and* Rust with one adapter |
| hax/Aeneas Lean-backend standard-library extraction becomes complete enough for ordinary application Rust | A per-module L4 deep track: prove the extracted model equals the reference model, upgrading DRT's falsification to proof |
| A project is greenfield and the team accepts writing in a verification language | Dafny for that module — by far the best automation (82% vs 27% in Lean) |
| The codebase is Rust-only and perf-critical | Verus in-code contracts as generated *projections* of the reference model's clauses |
| A Rust project needs cheap memory/panic assurance | Kani harnesses as an extra, language-specific evidence producer alongside DRT |

---

## Sources

**Strata / AWS verification platform**
- [strata-org/Strata](https://github.com/strata-org/Strata) — dialects, Laurel, SMT (cvc5/z3), pre-1.0 warning
- [`Strata/Languages/`](https://github.com/strata-org/Strata/tree/main/Strata/Languages) — B3, C_Simp, Core, Dyn, GOTO, Laurel (no Rust)
- [`Strata/Languages/GOTO.lean`](https://github.com/strata-org/Strata/blob/main/Strata/Languages/GOTO.lean) — CBMC `CProverGOTO` program model; `Strata.Backends.CBMC`
- [strata-org organization repositories](https://github.com/orgs/strata-org/repositories) — Strata-CLI, Strata-Python, Strata-Boole, Strata-Boogie, jverify, specimen
- [Strata-Python](https://github.com/strata-org/Strata-Python) — `.pyspec.st.ion` decorator specs (`@ensures`, `@admit`), `assert`→preconditions, `all`/`any`→∀/∃, Python→Laurel→Core→SMT via `pyAnalyzeLaurel`
- [`Strata/Languages/Core`](https://github.com/strata-org/Strata/tree/main/Strata/Languages/Core) — deep embedding: `Statement.lean`/`Expressions.lean`/`Program.lean` ASTs with separate `StatementEval.lean`/`ProgramEval.lean`/`CmdEval.lean`/`StatementSemantics.lean` interpreters
- [Strata on Reservoir](https://reservoir.lean-lang.org/@strata-org/Strata) — v0.1.0, Lean v4.29.1, updated 2026-09-04; "emphasizes verification rather than execution"
- [Strata-Boole](https://github.com/strata-org/Strata-Boole) — imperative specification language, friendlier front-end to Core
- [Strata issue search: "rust"](https://github.com/strata-org/Strata/issues?q=rust) — no Rust front-end work
- [Lean Into Verified Software Development (AWS Open Source Blog)](https://aws.amazon.com/blogs/opensource/lean-into-verified-software-development/)
- [First-Class Verification Dialects for MLIR (PLDI'25)](https://users.cs.utah.edu/~regehr/papers/pldi25.pdf) — the dialect idea Strata borrows

**The Lean-model + Rust-implementation pattern (the core recommendation)**
- [Lean Powers Secure Software at AWS: Cedar's Journey with Verified Development](https://lean-lang.org/use-cases/cedar/) — 10× smaller Lean model, DRT, 5µs vs 7µs per case
- [How We Built Cedar: A Verification-Guided Approach](https://arxiv.org/pdf/2407.01688) · [Amazon Science PDF](https://assets.amazon.science/d3/86/99db1aa142ffb6981d86dc849e4c/how-we-built-cedar-a-verification-guided-approach.pdf)
- [Kani: A Model Checker for Rust](https://arxiv.org/html/2607.01504) — Firecracker, s2n-quic, the Cedar `contains_at_least_two` bug
- [Verify the Safety of the Rust Standard Library (AWS Open Source Blog)](https://aws.amazon.com/blogs/opensource/verify-the-safety-of-the-rust-standard-library/)

**Rust → Lean**
- [A Rust-to-Lean Verification Pipeline with AI Provers: An Experience Report (arXiv 2605.30106, May 2026)](https://arxiv.org/html/2605.30106) — Charon→LLBC→Aeneas→Lean vs hax; AI provers as multiplier not eliminator; version drift and extraction limits
- [Aeneas: Rust verification by functional translation (ICFP'22)](https://dl.acm.org/doi/10.1145/3547647) · [project site](https://aeneasverif.github.io/) · [AeneasVerif/aeneas](https://github.com/AeneasVerif/aeneas) · [lean-lang.org/use-cases/aeneas](https://lean-lang.org/use-cases/aeneas/)
- [hax (Cryspen)](https://github.com/cryspen/hax) · [hax blog, 2026](https://hax.cryspen.com/blog/archive/2026/) — `cargo hax into lean`, `hax_lib::requires/ensures/prop`, `hax.toml` proof scenarios, charon+aeneas auto-managed
- [Verified-zkEVM/rust-lean](https://github.com/Verified-zkEVM/rust-lean)

**Rust verifiers**
- [Verus: Verifying Rust Programs using Linear Ghost Types](https://dl.acm.org/doi/10.1145/3586037) · [Verus: A Practical Foundation for Systems Verification](https://www.cs.utexas.edu/~hleblanc/pdfs/verus.pdf)
- [verus-analyzer (VS Code Marketplace)](https://marketplace.visualstudio.com/items?itemName=verus-lang.verus-analyzer) · [ProofPlumber: Debugging Automated Program Verification Proofs via Proof Actions](https://link.springer.com/chapter/10.1007/978-3-031-65627-9_17) — 17 proof actions over standard LSP
- [Surveying the Rust Verification Landscape](https://arxiv.org/html/2410.01981v1) — Creusot→Why3, Prusti→Viper, Verus

**LLM ↔ formal specification**
- [A benchmark for vericoding: formally verified program synthesis](https://arxiv.org/html/2509.22908) — 12,504 specs; 82% Dafny / 44% Verus / 27% Lean; NL descriptions don't significantly help
- [Clover: Closed-Loop Verifiable Code Generation](https://arxiv.org/abs/2310.17807) · [ChuyueSun/Clover](https://github.com/ChuyueSun/Clover) — consistency among code/doc/annotation; 87% acceptance, no false positives
- [Can LLMs Transform Natural Language Intent into Formal Method Postconditions? (nl2postcond)](https://arxiv.org/abs/2310.01831) · [project site](https://nl2postcond.github.io/) — correctness + discriminative-power metrics, 64 real bugs
- [Beyond Postconditions: Can LLMs infer Formal Contracts? (NL2Contract)](https://arxiv.org/abs/2510.12702)
- [DafnyBench](https://namin.seas.harvard.edu/pubs/dafnybench.pdf) · [From Natural Language to Verified Code (NL2VC-60)](https://arxiv.org/html/2604.22601v1) · [VERINA: Benchmarking Verifiable Code Generation](https://arxiv.org/pdf/2505.23135)

**Requirements engineering**
- [Formal Requirements Elicitation with FRET (NASA)](https://ntrs.nasa.gov/api/citations/20200001989/downloads/20200001989.pdf) · [Adventures in FRET and Specification](https://arxiv.org/html/2503.24040) — FRETISH = temporal logic + SPS + EARS; the traceability link between English and formal
- [From Natural Language Requirements to the Verification of Programmable Logic Controllers (FRET + PLCverif, NASA)](https://ntrs.nasa.gov/api/citations/20220019339/downloads/FINAL%20NFM23_NASA_FRET_PLCverif.pdf)
- [Leveraging LLMs for Formal Software Requirements: Challenges and Prospects](https://arxiv.org/html/2507.14330v2)

**Lean tooling / LSP**
- [Lean 4 Server source](https://github.com/leanprover/lean4/tree/master/src/Lean/Server) · [The Lean 4 Theorem Prover (system description)](https://lean-lang.org/papers/lean4.pdf) — `lake serve`, watchdog/worker, `LEAN_SERVER_LOG_DIR`
- [`Lean.Data.Lsp.CodeActions`](https://lean-lang.org/doc/api/Lean/Data/Lsp/CodeActions.html) — eager/lazy code actions; `Try this:` quick-fixes from `simp?`/`exact?`/`apply?`
- [lean-lsp-mcp](https://github.com/oOo0oOo/lean-lsp-mcp) · [tool reference](https://github.com/oOo0oOo/lean-lsp-mcp/blob/main/docs/tools.md) · [Numina-Lean-Agent](https://arxiv.org/pdf/2601.14027) — agent-facing LSP: goals, diagnostics, term info as the exclusive formal mediator
