# TraceLean — Requirement traceability with an executable Lean model

*Design document. Written 2026-09-11. Describes the path TraceLean is taking; the options
considered and rejected are archived in
[`verification_backends_considered.md`](./verification_backends_considered.md).*

---

## 0. The decision

TraceLean connects three artifacts, and owns the two bonds between them:

```
  natural-language requirement            (markdown, any file, any folder)
            │
            │  bond A — an LLM judge with an executable witness          [§6]
            ▼
  executable Lean reference model         (shallow, first-order, total)  [§4]
            │
            │  bond B — differential testing, language-independent       [§5]
            ▼
  the implementation                      (Rust, Python, anything)       [§3]
```

with proofs about the model hanging off the middle layer, and **annotations in code
comments** — never filenames, never directory layout — carrying every link. [§2]

Four commitments follow from this, and everything in the document is downstream of them:

1. **The model is independent of the code.** It is written from the requirement, not
   derived from the implementation. That independence is the only reason differential
   testing means anything: two artifacts built from the same intent agreeing is evidence;
   an encoding of the code agreeing with the code is not.
2. **The bond to the code is behavioural, not syntactic.** No extraction, no translation,
   no verifier-specific dialect. Consequently TraceLean works on Rust, Python, or anything
   else that can be driven from a test harness — including `unsafe`, generics, FFI and
   third-party crates, which defeat every translation-based tool.
3. **Nothing links by filename.** Requirements are identified by an immutable id in their
   frontmatter; code, models and tests are bound by comment annotations. TraceLean imposes
   no project structure on the projects that adopt it.
4. **Every link is graded and goes stale automatically.** A link carries evidence at a
   level, and the evidence is invalidated by a content hash the moment either side
   changes. A green link that can silently go wrong is worse than no link.

**The chain is only as strong as its weakest bond.** Proving a theorem about a model that
has drifted from the requirement proves nothing; a faithful model that is never
differentially tested against the code proves nothing either. The evidence ladder [§7]
exists to make that visible rather than to hide it behind a checkmark.

---

## 1. What exists today, and what of it survives

Verified against the code before writing this:

| Today | Where | Fate |
|---|---|---|
| Requirement → spec by **filename equality** | `core/src/trace_graph/scan.rs:129` | **deleted** — replaced by annotations [§2] |
| Spec → code (`link_spec_to_code`, never called) | `core/src/trace_graph/mod.rs:104` | **deleted** — replaced by annotations + DRT |
| Code → test by path `tests/unit/test_<src>.rs` | `scan.rs:~145` | **deleted** — tests annotate themselves [§2.1] |
| Requirement → test by filename `req_01_*.rs` | `scan.rs:~165` | **deleted** |
| Trace graph: nodes + edges + queries (petgraph) | `core/src/trace_graph/` | **kept** — new node/edge kinds, same engine |
| `lean --run` on a spec file | `core/src/requirements.rs:182` | **fixed** — `--run` executes `main`; use `lake env lean` [§4.4] |
| Requirement status `Draft/Approved/Linked` | `core/src/requirements.rs:11` | **kept as workflow**, joined by an orthogonal evidence level [§7] |
| Tree-sitter symbol table + grammars (rust, python, cpp, lean4, markdown) | `core/src/parser.rs`, `grammars/` | **kept** — the basis of anchors [§2.2] |
| Embeddings index behind `find_semantic` | core | **kept** — glossary binding and hierarchy discovery [§9.4] |
| Command/undo tree | `core/src/undo_tree.rs` | **kept** — the provenance substrate everything records into |

---

## 2. Annotations — the linking language

Links live in comments, in the file they describe, reviewed in the same diff as the code
they talk about. That is the only form of link that survives a refactor by someone who has
never heard of TraceLean.

### 2.1 Roles

An annotation is a **typed edge with a role**, not an ID mention. The role is what makes
progress computable: "mentions REQ-03" tells you nothing; "implements clause `post` of
REQ-03, and that test over there exercises it" tells you exactly what is missing.

| Role | Written on | Means |
|---|---|---|
| `@models` | the Lean model | this definition is the formal model of that requirement/clause |
| `@implements` | production code | this code realizes it |
| `@tests` | a test | this test exercises it |
| `@drt` | a DRT harness | this harness binds model ↔ implementation for it |
| `@proves` | a Lean theorem | this theorem discharges a property of the model |
| `@refines` | a requirement | this requirement is a child of that one |
| `@partial` | anywhere | claims *part* of it; requires `reason=` |
| `@exempt` | anywhere | deliberately not done; requires `reason=` and `by=`, optional `until=` |

```rust
// @implements REQ-AUTH-03.post
pub fn login(c: Credentials) -> Result<AuthToken, AuthError> { … }

// @tests REQ-AUTH-03.err
#[test] fn every_failure_maps_to_one_variant() { … }
```

```python
# @implements REQ-SEARCH-02
# @partial REQ-SEARCH-02.perf reason="latency budget not enforced yet"
def search(index, query): ...
```

```lean
-- @models REQ-AUTH-03.post
def login (c : Credentials) : Except AuthError AuthToken := …

-- @proves REQ-AUTH-03.err
theorem login_error_total : ∀ c, (login c).isError → ∃! e : AuthError, … := …
```

`@partial` and `@exempt` are what make the model survive contact with reality. Without
them people write `@implements` on half-working things and every number downstream becomes
fiction. With them, "we consciously chose not to do this, here is who signed off and when
it expires" becomes first-class, auditable data.

**Tests are annotated, never inferred from paths.** A test in `src/lib.rs`, in
`tests/`, in `spec/`, or in a doctest is equally findable, because the annotation is
inside it. This is the rule that keeps TraceLean structure-agnostic.

### 2.2 Anchors — surviving edits

Parsing the comment is easy; making the link survive editing is the hard part.

The scanner walks **tree-sitter comment nodes** (grammars already exist for rust, python,
cpp, lean4, markdown) and applies one pattern — `@(\w+)\s+([A-Z][\w-]*(?:\.\w+)?)` — inside
them. `//`, `#`, `--`, `/* */`, docstrings and Lean's `/-- -/` all work with no
per-language rules; a language without a grammar degrades to a line scan, still usable,
just without anchor resolution.

Each annotation binds to an anchor, in this order of preference:

1. **Declaration anchor (default)** — the next named declaration, resolved to a *symbol
   path* (`src/auth.rs :: impl AuthService :: login`). Never a line number: line numbers
   rot on the first edit above them, symbol paths survive reordering and reindentation.
2. **Region anchor** — `// @implements REQ-X begin` … `// @end` for logic that is not a
   declaration (a match arm, a config block, a migration).
3. **File anchor** — an annotation before any declaration binds to the whole file.

For every anchor, store a hash of the **normalized body** — comments and whitespace
stripped, so reformatting does not churn. That hash is what drives staleness [§6].

---

## 3. The implementation layer — any language

Nothing is required of the implementation except that it can be **driven from a harness**:
call a function, feed it an input, observe an output. Rust, Python, TypeScript, a CLI, an
HTTP endpoint — all are bindable.

This is the direct consequence of choosing a behavioural bond. TraceLean never parses the
implementation for verification purposes; it only ever *runs* it. So the tool inherits
none of the restrictions that make translation-based verification hard: `unsafe`,
trait-object dispatch, generics with complex bounds, external crates, C FFI, async — all
irrelevant, because none of it is being translated.

What the implementation layer *does* owe the system: an `@implements` annotation on the
entry points that realize a requirement, so coverage and drift have an anchor [§2.2].

---

## 4. The Lean reference model

The model is the middle layer, and the only artifact that both a domain expert and a
prover can read. It is a **re-implementation of the intended behaviour, written from the
requirement** — deliberately not derived from the code.

### 4.1 Style rules (these are load-bearing, not taste)

- **Shallow embedding.** Ordinary Lean functions over ordinary data, compiled by Lean's
  code generator. Not an AST plus an interpreter. This is what makes a differential test
  case cost microseconds instead of milliseconds, and millions of cases fit in CI.
- **First-order and total.** No dependent types in the executable core, no partial
  functions, no `sorry` on the model path. Totality is what lets the harness run it on
  adversarial random input without hanging or crashing.
- **Small.** AWS's Cedar model is ~10× smaller than the production implementation. Smallness
  is the point: it is what makes the model reviewable as documentation and its proofs
  tractable.
- **Proofs live in separate files from the model.** The model is executable code; theorems
  about it are a different artifact with a different lifecycle. Mixing them makes the model
  slow to compile and hard to read.
- **No implementation detail.** Caches, pools, concurrency, IO buffering, retry logic — the
  model must not mirror them. If the model starts to look like the code, independence is
  gone and the differential test degenerates into testing a transliteration of itself.

### 4.2 What the model is allowed to leave out

Not every requirement clause is modelable, and pretending otherwise produces vacuous
models. "Failed login attempts shall be logged" has no meaningful denotation in a pure
model of authentication. Three honest outcomes, all first-class:

- **modeled** — the clause has a `@models` anchor.
- **`@partial`** — partially modeled, with a stated reason.
- **`@exempt`** — deliberately outside the model, with reason and approver; covered by an
  ordinary `@tests` link instead, and reported as such rather than as a gap.

Which of these applies is a decision the judge [§6.2] surfaces but never makes alone.

### 4.3 Project layout

The model lives wherever the project wants it — it is found by its annotations, not its
path. A Lean *toolchain* is still needed: a `lakefile` so the model can import Mathlib and
be compiled by `lake build`. TraceLean detects the lake project; if there isn't one, it
offers to create a minimal one rather than failing.

### 4.4 Running Lean correctly

The current `lean <file> --run` is wrong for this purpose: `--run` executes `main`, which a
model file does not have. Use `lake env lean <file>` for type-checking inside the project's
dependency environment, and `lake build` plus a generated `main` for the DRT runner [§5.2].
Detect the toolchain via `elan`, and when it is missing show a first-class "Lean toolchain
not configured" state with install instructions — the difference between *broken* and *not
set up*.

---

## 5. Bond B — differential testing, model ↔ implementation

This is the Cedar pattern: generate large numbers of random inputs, run both the Lean model
and the real implementation, require identical results. AWS runs it in CI against Cedar's
production Rust at roughly 5µs per case on the Lean side versus 7µs on the Rust side — fast
enough to be a build step, not a research project.

### 5.1 The conformance protocol — how the bond stays language-independent

Both sides are wrapped as processes speaking **line-delimited JSON** on stdin/stdout:

```
→  {"case": 41, "op": "login", "input": {"username": "ana", "password": "hunter22"}}
←  {"case": 41, "output": {"Err": {"weakPassword": {}}}}
```

- The **model runner** is generated: `lake build` produces a binary whose `main` decodes a
  case, calls the `@models` function, and encodes the result. Encoding is derived from the
  Lean types (`ToJson`/`FromJson` deriving), so it tracks the model automatically.
- The **implementation runner** is a thin adapter written once per entry point in the
  implementation's own language — the only code a user must hand-write for a new binding,
  and the honest cost of the approach. It is small (decode, call, encode) and it is itself
  annotated (`@drt REQ-AUTH-03`), so it is anchored and drift-checked like everything else.
- TraceLean owns the **generator** and the **comparator**, and therefore never needs to
  understand the implementation language. Adding Go or TypeScript support means writing an
  adapter, not touching TraceLean.

The protocol is the reason bond B is language-independent in a way translation never is.

### 5.2 Generating inputs that mean something

"12,000,000 cases, 0 divergences" is a worthless number if every case took the same branch.
Three generator sources, combined:

1. **Type-directed generation** from the model's input ADTs — free, broad, shallow.
2. **Model-branch-directed generation.** The model is deliberately small [§4.1], which makes
   *branch coverage of the model* an achievable and meaningful target. Drive the generator
   until every branch of the model has been exercised, and **report model-branch coverage
   next to the case count**. This is the number that makes the evidence honest, and it is
   only affordable because the model is small.
3. **Seeded corpora** — witnesses produced by the judge [§6.4], past divergences, and
   regression cases, all replayed on every run.

Failures **shrink** to a minimal input before being reported.

### 5.3 A divergence is a finding, not a verdict

When the two sides disagree, three things could be wrong, and the UI must not assume:

| Cause | Signal | Action |
|---|---|---|
| **Implementation bug** | model matches the requirement on this input | the highest-value output the system produces — file it against the code |
| **Model bug** | implementation is right, model is wrong | fix the model; the requirement link becomes stale and re-judged [§6.2] |
| **Adapter bug** | encode/decode asymmetry, not a behavioural difference | fix the runner; no requirement impact |

Triage is a human/agent decision, recorded as a `Command`, so the choice is attributable
later. Never auto-file a divergence as a code bug.

### 5.4 The evidence record

Each DRT run writes one record: requirement + clause, model anchor hash, implementation
anchor hash, adapter hash, seed, case count, model-branch coverage, divergence count,
shrunk witnesses, Lean toolchain version, commit, and the undo-tree node of the command
that launched it. Reproducible by construction — the seed and the hashes are enough to
replay it exactly.

---

## 6. Drift — the mechanism that keeps this from rotting

Every traceability matrix in history has decayed into decoration because links were
asserted once and never re-checked. Drift detection is what makes this one different.

### 6.1 The hash mechanism

For each anchor, store the normalized-body hash [§2.2]. Then, on every scan:

- **hash unchanged** → evidence stands, badge keeps its level.
- **hash changed** → the link is **not broken, it is stale**: rendered grey with "changed
  since last verified", and requeued for whichever check produced its evidence.
- **symbol gone** → **dangling anchor**, reported with the last known body so the code can
  be found where it moved to.
- **renamed through LSP** → the rename is lowered into a `Command` [§11.4], so the anchor
  follows it and no staleness is generated at all.

Staleness is cheap to detect and expensive to resolve, which is the right way round: the
scan is instant, and only the affected links pay for re-verification.

### 6.2 What re-verifies each bond

| Stale bond | Re-verified by | Cost | Automatable |
|---|---|---|---|
| **Model ↔ implementation** (bond B) | **rerun differential testing** on the affected entry points, with the stored seed plus fresh cases | seconds to minutes | fully |
| **Requirement ↔ model** (bond A) | **LLM judge with an executed witness** [§6.3] | one or two model calls | proposes; a human or agent confirms |
| **Theorem ↔ model** (`@proves`) | re-run `lake build`; Lean's own recompilation tells you | seconds | fully |

Bond B is machine-checkable, so it re-verifies itself. Bond A crosses from English into
mathematics, where there is no oracle — so it gets a judge, and the judge gets an oracle
bolted on [§6.4].

### 6.3 Bond A — the requirement↔model judge

The judge answers exactly one question: **does this Lean model still faithfully formalize
this requirement clause?** It is explicitly *not* asked whether the code is correct, whether
the requirement is a good requirement, or whether the Lean is stylish.

Four design rules, each earning its place:

1. **Ask the two directions separately.** "Is this faithful?" is a question models answer
   agreeably. "Does the model permit anything the requirement forbids?" and "does the model
   forbid anything the requirement permits?" are questions with a discoverable answer.
2. **Demand a concrete witness.** The judge must produce an input on which the two disagree,
   or explicitly state that it found none. A judge forced to exhibit a witness hallucinates
   far less than one allowed to assert a conclusion.
3. **Show the diff, not just the current state.** Most judgements are about a *change*.
   Passing both sides' before/after makes the task local, cheap and far more accurate than
   re-deriving the whole judgement from scratch.
4. **Abstention is a valid answer.** `unclear` with a reason is a useful output — it is how
   unmodelable clauses [§4.2] get discovered instead of being forced into a bad model.

#### The prompt

```
SYSTEM
You check whether a formal Lean model still faithfully formalizes one clause of a
natural-language requirement.

You are NOT judging whether the implementation is correct, whether the requirement is
well-written, or whether the Lean is idiomatic. Only: does the model say the same thing
as the clause?

Answer in two directions, separately:
  (a) UNDER-CONSTRAINED — the model permits behaviour the clause forbids.
  (b) OVER-CONSTRAINED  — the model forbids behaviour the clause permits.

Rules:
- If you claim drift, you MUST give a concrete witness input, the outcome the clause
  requires, and the outcome the model produces. The witness will be executed against the
  model; if your claim about the model's behaviour is wrong, your verdict is discarded.
- If the clause cannot be expressed in this model at all (it is about logging, latency,
  deployment, or anything the model has no notion of), answer "unmodelable" — do not
  invent a formalization.
- If you cannot tell, answer "unclear" and say what is missing. Guessing is worse than
  abstaining.
- Judge only the clause given. Other clauses of the same requirement are out of scope.

USER
REQUIREMENT {req_id}, clause "{clause_key}"

Clause text (current):
{clause_text}

Clause text (previous, if changed):
{clause_text_before}

Lean model (current):
{model_source}

Lean model (previous, if changed):
{model_source_before}

Glossary (requirement term → model symbol):
{glossary}

Previous verdict: {previous_verdict}

Return JSON:
{
  "verdict": "agrees" | "under_constrained" | "over_constrained" | "both" |
             "unmodelable" | "unclear",
  "witness": { "input": <json>, "clause_requires": "<text>", "model_produces": "<text>" }
             | null,
  "explanation": "<=2 sentences, cite the specific words of the clause",
  "suggested_patch": "<Lean snippet or null>",
  "confidence": "high" | "medium" | "low"
}
```

#### Worked examples

**1 — `agrees`.** Clause: *"passwords must be at least 8 characters."*
Model: `structure Password where val : String; min_length : val.length ≥ 8`.
→ `{"verdict":"agrees","witness":null,"explanation":"The structure's invariant is exactly
the clause's threshold, in the same direction.","confidence":"high"}`

**2 — `under_constrained`** (the common real case: the requirement was tightened and the
model was not). Clause changed to *"passwords must be at least 12 characters and contain at
least one digit."* Model still says `val.length ≥ 8`.
→ `{"verdict":"under_constrained","witness":{"input":{"password":"hunter22xy"},
"clause_requires":"rejected — 10 characters, below the 12 required",
"model_produces":"accepted — length 10 satisfies ≥ 8"},"explanation":"The clause now
requires 12 characters and a digit; the model still encodes the old threshold of 8 and has
no digit condition.","suggested_patch":"min_length : val.length ≥ 12\\n  has_digit :
val.any Char.isDigit","confidence":"high"}`

**3 — `over_constrained`.** Clause: *"the account shall be locked after 5 consecutive failed
attempts."* Model locks at 3.
→ `{"verdict":"over_constrained","witness":{"input":{"consecutive_failures":4},
"clause_requires":"not locked — 4 is fewer than 5",
"model_produces":"accountLocked"},"explanation":"The clause sets the threshold at 5
consecutive failures; the model triggers at 3, locking accounts the clause says must stay
open.","confidence":"high"}`

**4 — `unmodelable`** (from this project's own REQ-01). Clause: *"failed login attempts
shall be logged."*
→ `{"verdict":"unmodelable","witness":null,"explanation":"The clause constrains an
observable side effect — log emission — and this model is a pure function from credentials
to a token or error, with no notion of logging.","suggested_patch":null,
"confidence":"high"}`
→ TraceLean's response: propose `@exempt REQ-01.log reason="side effect, not in the pure
model" by=<user>` **plus** a required `@tests` link, so the clause is covered by an ordinary
test and reported as *test-covered, not modeled* rather than as a silent gap.

### 6.4 Executing the witness — how the judge gets an oracle

This is what separates this design from "ask an LLM if it looks right":

1. The judge returns a witness input.
2. TraceLean **runs the witness through the compiled model** using the same runner as DRT
   [§5.1] — the model is executable, so this costs microseconds.
3. Compare the model's actual output to the `model_produces` the judge claimed.
   - **Claim confirmed** → the disagreement is real, and the witness is added to the DRT
     seed corpus permanently. The finding goes to the user with a concrete, reproducible
     example rather than an opinion.
   - **Claim falsified** → the judge misread the Lean. The verdict is **discarded**, logged
     against the prompt version, and retried once; a second falsification escalates to the
     user as "judge unreliable here" rather than as a requirement finding.

So a `drifted` verdict is never taken on the judge's word — only its *reasoning about
English* is trusted, and its *claims about Lean* are executed. That is a much narrower and
more defensible use of an LLM than "is this spec right?".

`agrees` cannot be machine-confirmed the same way (absence of a witness is not proof of
absence). It therefore restores the link's evidence level only to **L2** [§7], never
higher, and never on its own converts an untested link into a verified one.

### 6.5 Keeping the judge honest over time

- **Temperature 0**, and every verdict records model id, prompt version, and both input
  hashes. A verdict is evidence, so it must be attributable and reproducible.
- **Second opinion by backtranslation** for `agrees` on high-value requirements: a separate
  call renders the Lean into English *without seeing the clause*, and a third compares the
  two English texts. This removes the anchoring bias of a judge that has already read the
  requirement and is looking for reasons to agree.
- **A calibration suite**, versioned with the prompt: pairs known to agree, plus pairs
  deliberately mutated to drift (threshold changed, quantifier flipped, an error case
  dropped, a condition negated). Changing the prompt requires the suite to pass, with
  **zero false `agrees` on the mutated pairs** — the asymmetry that matters, since a missed
  drift is silent and a false alarm is merely annoying.
- **Judge findings are proposals.** Acceptance or rejection is a `Command` with an author,
  so the requirement's history shows who decided what a clause meant, and when.

---

## 7. The evidence ladder

An annotation is a claim. The ladder grades what that claim is worth, and the checker
records the rung each link reached.

| Level | Name | Earned by | Bond |
|---|---|---|---|
| **L1** | Annotated | the annotation resolves: requirement exists, clause exists, anchor exists | — |
| **L2** | Consistent | the judge finds no drift between clause and model, with witness-execution passing [§6.4] | A |
| **L3** | Conformant | differential testing passes with stated case count and model-branch coverage [§5] | B |
| **L4** | Proved | a Lean theorem about the model discharges the clause's property (`@proves`) | — |

Two rules keep the ladder honest:

**Rule 1 — the chain governs.** L4 is a property of the *model*; it says nothing about the
implementation until bond B carries it across. A requirement showing `L4` with a stale or
missing DRT is displaying a proof about a document, not about software. The UI therefore
shows assurance as the **weakest link in the chain** — `L4 model · L1 code` reads
correctly; a single "L4" badge would be a lie.

**Rule 2 — L2 does not stack upward on its own.** The judge agreeing restores consistency
evidence only; it can never promote a link to conformant or proved, because absence of a
witness is not proof of absence.

---

## 8. The checker

One command, one machine-readable report, one CI gate. Findings get distinct names, because
"traceability error" is useless in a diff:

| Class | Meaning | Severity |
|---|---|---|
| `dangling` | annotation names a requirement or clause that does not exist | **error** |
| `broken-anchor` | the annotated symbol no longer exists | **error** |
| `stale` | anchor body changed since the evidence was recorded | warning, auto-requeues |
| `unmodeled` | clause with no `@models` and no `@exempt` | info — this is progress, not a fault |
| `unimplemented` | modeled but no `@implements` | info |
| `unbound` | model and implementation both exist, but no `@drt` harness binds them | **warning** — this is the one that matters most, because it is a model nobody is checking against reality |
| `untested` | implemented but no `@tests` | warning |
| `judge-drift` | the judge found confirmed drift on bond A | **error** once witness-confirmed |
| `divergence` | differential testing found a mismatch | **error** until triaged [§5.3] |
| `contested` | two anchors exclusively claim the same clause | warning |
| `unsound-exemption` | `@exempt` without reason/approver, or past its `until=` date | **error** |

The CI gate is a **policy file, not hardcoded rules**: block on `dangling`,
`broken-anchor`, `divergence`, `unsound-exemption`; require `REQ-SEC-*` at L3 or better;
everything else advisory. A team adopting this on a codebase at 0% coverage must be able to
start with "report, don't block".

---

## 9. Hierarchy, zoom, and progress

### 9.1 Requirement identity and structure

```markdown
---
id: REQ-AUTH-03          # immutable; the filename and folder are irrelevant
refines: [REQ-AUTH, REQ-SEC-PASSWORD-POLICY]
decomposition: complete  # are my children exhaustive? see §9.3
clauses:
  pre:  credentials are well-formed
  post: returns a token iff the credentials are valid
  err:  every failure maps to exactly one AuthError variant
  log:  failed attempts are logged
---
The system shall authenticate users via username and password.
```

The index is "every `.md` in the project carrying an `id:`", found anywhere in the tree.

**It is a DAG, not a tree.** A leaf legitimately refines both a feature parent and a
cross-cutting security parent. Forcing a tree makes people duplicate requirements, and
duplicates drift apart. The cost is that roll-up must compute over the *set* of reachable
leaf clauses rather than summing children.

### 9.2 Two numbers, never one

- **Coverage** — the fraction of reachable leaf clauses carrying any evidence. Aggregates by
  set union over the DAG.
- **Assurance** — the **minimum** rung across those clauses, and within a clause the minimum
  across the chain [§7]. Never an average.

`coverage 100% · assurance L1` is instantly readable: everything is claimed, nothing is
checked. An average would have printed "L2.7" and buried the one unchecked clause. Weakest
link is the only honest aggregator; averages are how a green dashboard ends up sitting on a
red leaf.

### 9.3 The denominator problem

You cannot say "80% done" without knowing what 100% is, and a parent's children are not
guaranteed to exhaust it. Hence the mandatory `decomposition:` field:

- `complete` — children fully cover the parent; percentages are meaningful.
- `open` (default) — more children may exist; the UI shows **"≥ 62%"**, never "62%", and the
  parent can never render as finished.

Requiring that claim explicitly is a small friction that buys the metric its credibility,
and "which parents are still `open`?" is itself a useful worklist. `@exempt` clauses leave
the denominator (listed separately, with approver and expiry); `@partial` caps the numerator
below 100%.

### 9.4 Zoom — one model, five altitudes

| Zoom | Unit | Question it answers |
|---|---|---|
| **Z1** | capability / top-level group | "Is authentication in good shape?" |
| **Z2** | requirement | "What's left on REQ-AUTH-03?" |
| **Z3** | clause | "The error contract is the unchecked part" |
| **Z4** | symbol | "`login` implements it; `refresh` claims it too" |
| **Z5** | region / line | "*This* branch has no test" |

Four coordinated views, one selection:

1. **Requirement outline** — the DAG collapsed to the current depth; each row shows
   `coverage · assurance · stale count · open|complete`. Collapsing a parent *aggregates* its
   children's problems rather than hiding them, so a collapsed tree never looks healthier
   than an expanded one. That property deserves a test.
2. **Coverage map** — a treemap of the project, rectangles sized by lines and coloured by
   which requirement they serve, **grey for untraced**. Select a requirement, its footprint
   lights up across the codebase; select a file, its requirements light up in the outline.
   This is the picture that sells the product: the whole project partitioned by intent, and
   exactly how much is grey.
3. **Editor gutter** — a chip on each annotated declaration coloured by its chain state;
   stale links get a distinct marker, so you can *watch* an edit invalidate evidence as you
   type.
4. **Progress over history** — §9.5.

### 9.5 Progress derived from history

Coverage and assurance are pure functions of a repository state, so they can be evaluated at
any node of the undo tree or any commit:

- a **real burn-down** of modeling and conformance over time — recomputed from artifacts, not
  maintained by hand, and therefore not fudgeable;
- **regression alerts**: "assurance of REQ-SEC-01 dropped L3 → L1 at command 3f9a", with the
  diff and the author (human or model) attached;
- **velocity that means something**: "9 clauses reached L3 this week; 4 went stale".

Cheap to compute incrementally, because evidence records already carry input hashes: replay
means re-running the scanner (instant) and only re-running DRT where hashes changed.

### 9.6 Discovering the hierarchy

The analyser proposes structure rather than demanding it; every proposal is accepted or
rejected as a `Command`, so the hierarchy itself has provenance.

1. **Structural containment** — if requirement A's anchor set ⊇ the union of B, C and D's,
   propose A as their parent. Pure set inclusion, and the justification is showable: "A
   covers all 11 symbols B, C and D cover, plus 3 more."
2. **Semantic clustering** — the embeddings index behind `find_semantic` already gives
   requirement-to-requirement similarity; cluster the unparented ones and have the model
   draft a parent whose text genuinely generalizes its members. Surfaced as a diff.
3. **Co-change** — symbols that always change together across the undo tree and git history
   evidence a shared, unstated requirement. A co-changing cluster spanning several
   requirements suggests a missing parent; one spanning none suggests a missing requirement.

Two findings fall out and are worth surfacing loudly: **implicit parent** (a cluster of
sibling-ish leaves with no parent) and **incomplete parent** (a parent whose text contains
obligations no child covers — "your decomposition has a hole", the most valuable thing an
analyser can say about a requirement set).

---

## 10. Storage

- **Source of truth is the files**: annotations in comments, requirements in markdown with
  frontmatter, the model in Lean, adapters in their own languages. Everything a human writes
  stays in the project and is reviewed in the normal diff.
- **Derived: `.tracelean/trace.lock.json`** — resolved graph, anchor symbol paths, body
  hashes, and evidence records (judge verdicts with prompt version and model id; DRT seeds,
  case counts, branch coverage, divergences; Lean toolchain version; undo-node ids).
  Deterministic and committed, so CI and teammates see the same graph and stale links show
  up in review.
- **Never a hidden database as the source of truth.** The moment the graph is not
  reconstructible from the repository, the tool stops being adoptable.

---

## 11. LSP

### 11.1 Client and servers

TraceLean is the **client**, so `async-lsp` (tower-composable, ordered notification
handling), not `tower-lsp`. A registry keyed by language, because the whole premise is a
Lean model and an implementation open side by side: `lake serve` for Lean, `rust-analyzer`
for Rust, `pyright`/`ruff` for Python.

### 11.2 Lean specifics a generic client gets wrong

- Lean diagnostics are *elaboration* results arriving slowly and incrementally. Use
  `textDocument/waitForDiagnostics`; a client that reads "no diagnostics yet" as "file is
  fine" will show green on an unchecked model.
- The features that make Lean editing worth anything are custom extensions:
  `$/lean/plainGoal` and `$/lean/plainTermGoal` (goal state at the cursor) and
  `$/lean/rpc/connect` for the InfoView. **Goal-state-at-cursor before hover, before
  symbols** — it is what makes writing the model and its proofs bearable.
- Code actions arrive through the standard `codeAction` channel (`Lean.Data.Lsp.CodeActions`,
  eager and lazy), including the `Try this: simp only [...]` suggestions from
  `simp?`/`exact?`/`apply?`. Build Myth's action surface against `codeAction` once and it
  works for Lean and rust-analyzer alike.
- `LEAN_SERVER_LOG_DIR` dumps every stream to disk. Use it while building.

### 11.3 Give the LSP to the agent

Expose `lsp_query` with modes `hover | definition | references | diagnostics | symbols |
code_actions | goal`. This is how Lean agents are actually built — `lean-lsp-mcp` exists
precisely so an agent can ask for goal state and diagnostics instead of shelling out, and in
Numina-Lean-Agent it is the exclusive mediator for all formal interaction. An agent writing
a Lean model or proof without goal state is guessing. It is also a large token saving: a goal
at a cursor is ~200 tokens and replaces re-reading a file that costs thousands.

### 11.4 LSP edits must become Commands

`WorkspaceEdit` from rename and code actions must be lowered into the existing invertible
`Command`s and pushed through the undo tree. **Blocking, not nice-to-have**: it is what makes
anchors survive renames [§6.1], and what keeps "one history of everything" true.

---

## 12. Myth

Current state: `ui_settings/keymap.json` defines `Main`, `Options`, `File` as flat
`key → action | →Mode` maps; which-key and the mode machine cover the Editor and File Tree
only; `FileTreeSurface` re-parses its whole content per view build.

What this design needs from it, in order:

1. **Dynamic action providers.** `bindings.json` maps a capture to a *static* list, but LSP
   code actions and trace actions are computed *at the cursor*. Introduce
   `fn actions_at(&self, cx: &CursorCx) -> Vec<Action>`, with the static map as one provider
   beside an LSP provider and a trace provider. LSP's `codeAction` is literally "named
   operations valid at this location" — Myth's model verbatim.
2. **Titles, groups and priority on actions**, not just names; LSP's `kind` gives grouping for
   free.
3. **A real mode stack.** Today `Escape` in `File` jumps to `→Main` while the README documents
   "back to Options" — exactly the kind of mismatch that makes the system feel unreliable.
   Push on entry, pop on `Escape`, and generate the README table from `keymap.json` (the
   startup test that rejects unknown actions should also assert the documented table matches).
4. **Incremental re-parse**, before live diagnostics; don't let it block go-to-def, symbols or
   code actions, which need no new capability.
5. **New modes:**

```jsonc
"Main":  { "C-Space": "→Options", "g": "→Goto", "v": "→Verify", "t": "→Trace" }

"Goto":  { "d": "lsp_definition", "r": "lsp_references", "s": "lsp_symbols",
           "e": "next_diagnostic", "E": "prev_diagnostic" }

"Verify":{ "g": "show_goal",        // Lean goal state at cursor
           "d": "run_drt",          // differential-test this binding
           "j": "run_judge",        // re-judge requirement ↔ model
           "w": "replay_witness",   // run the last witness through the model
           "b": "lake_build",
           "a": "→CodeActions" }    // dynamic: LSP quick-fixes + Try-this

"Trace": { "r": "goto_requirement", "m": "goto_model",
           "c": "goto_code", "t": "goto_tests", "h": "goto_harness",
           "e": "show_evidence",    // the record + the command that produced it
           "u": "goto_provenance",  // jump the undo tree to who wrote this line
           "a": "annotate",         // add an @implements/@tests/@models here
           "1".."5": "zoom_level", "+": "zoom_in", "-": "zoom_out",
           "M": "coverage_map",
           "?": "explain_gap" }     // why is this requirement not green?
```

`Trace` mode is the uniquely-TraceLean surface: put the cursor on a line, press `t u`, land
on the moment it was written and the requirement it came from.

---

## 13. Sequencing

**Weeks 1–2 — the annotation spine.** No new dependencies.
Scanner over tree-sitter comment nodes; roles and anchors; symbol-path resolution; body
hashing; requirement index by frontmatter `id`; the checker with named failure classes;
`.tracelean/trace.lock.json`. Delete `link_by_convention` and the four filename conventions.

**Weeks 3–4 — the bond that proves the idea.**
Lean toolchain detection and the `lake env lean` fix; the JSON conformance protocol; a
generated model runner; one hand-written adapter for the example project; type-directed
generation with shrinking; the evidence record. **One requirement end-to-end at L3** is the
milestone that makes everything else worth building.

**Weeks 5–6 — the judge.**
Prompt, witness execution against the compiled model, calibration suite, verdicts as
evidence records with prompt version and model id. Wire `stale` → requeue for both bonds.

**Weeks 7–9 — hierarchy and the picture.**
`refines` DAG, `decomposition`, coverage/assurance roll-up, zoomable outline, gutter chips,
the coverage treemap, coverage-over-history from the undo tree.

**Weeks 10+ — LSP and Myth.**
`async-lsp` client, Lean goal state, `lsp_query` for the agent, `WorkspaceEdit` → `Command`,
dynamic action providers, mode stack, `Goto`/`Verify`/`Trace` modes.

**Later, optional:** model-branch-directed generation; `@proves` theorems and L4; the
hierarchy analyser.

---

## 14. Risks

- **Adapter cost.** Every new entry point needs a hand-written runner. It is small, but it is
  the friction users will feel first — keep adapters tiny, generate skeletons, and annotate
  them so they are drift-checked like everything else.
- **Model/implementation drift in *shape*.** If the implementation's interface changes, the
  adapter breaks rather than the model. Adapter hashes are in the evidence record for exactly
  this reason.
- **Weak generators.** A million trivial cases prove nothing. Model-branch coverage is the
  guard; report it or the case count becomes theatre.
- **Judge over-trust.** The mitigation is structural: only its reasoning about English is
  trusted, its claims about Lean are executed, and false `agrees` on the calibration suite is
  the metric that gates prompt changes.
- **Model drift into implementation.** If the model starts mirroring the code's structure,
  independence is lost and DRT degenerates into testing a transliteration. Review the model
  for size and abstraction, not just correctness.
- **Unmodelable clauses accumulating as exemptions.** Track the ratio; a requirement set that
  is 70% exempt is telling you the model is in the wrong place.
- **Metric theatre.** Open denominators, weakest-link assurance, and exemptions with
  approvers and expiry are what keep the numbers honest. Do not trade them for prettier
  dashboards.

---

## 15. Sources

**The pattern this design is built on**
- [Lean Powers Secure Software at AWS: Cedar's Journey with Verified Development](https://lean-lang.org/use-cases/cedar/) — executable Lean model ~10× smaller than production, differential random testing against the Rust, ~5µs vs ~7µs per case
- [How We Built Cedar: A Verification-Guided Approach](https://arxiv.org/pdf/2407.01688) · [Amazon Science PDF](https://assets.amazon.science/d3/86/99db1aa142ffb6981d86dc849e4c/how-we-built-cedar-a-verification-guided-approach.pdf)
- [Lean Into Verified Software Development (AWS Open Source Blog)](https://aws.amazon.com/blogs/opensource/lean-into-verified-software-development/)

**Specification consistency and LLM judging**
- [Clover: Closed-Loop Verifiable Code Generation](https://arxiv.org/abs/2310.17807) · [ChuyueSun/Clover](https://github.com/ChuyueSun/Clover) — correctness checking reduced to consistency checking among code, doc and formal annotation; 87% acceptance with zero false positives on adversarial cases
- [Can LLMs Transform Natural Language Intent into Formal Method Postconditions? (nl2postcond)](https://arxiv.org/abs/2310.01831) · [project site](https://nl2postcond.github.io/) — correctness and discriminative-power metrics; 64 real historical bugs discriminated
- [Beyond Postconditions: Can LLMs infer Formal Contracts? (NL2Contract)](https://arxiv.org/abs/2510.12702)

**Requirements engineering**
- [Formal Requirements Elicitation with FRET (NASA)](https://ntrs.nasa.gov/api/citations/20200001989/downloads/20200001989.pdf) · [Adventures in FRET and Specification](https://arxiv.org/html/2503.24040) — structured NL as the traceability link between English and formalization; the tool proposes, the human decides
- [Leveraging LLMs for Formal Software Requirements: Challenges and Prospects](https://arxiv.org/html/2507.14330v2)

**Lean tooling / LSP**
- [Lean 4 Server source](https://github.com/leanprover/lean4/tree/master/src/Lean/Server) · [The Lean 4 Theorem Prover (system description)](https://lean-lang.org/papers/lean4.pdf) — `lake serve`, watchdog/worker, `LEAN_SERVER_LOG_DIR`
- [`Lean.Data.Lsp.CodeActions`](https://lean-lang.org/doc/api/Lean/Data/Lsp/CodeActions.html) — eager/lazy code actions, `Try this:` quick-fixes
- [lean-lsp-mcp](https://github.com/oOo0oOo/lean-lsp-mcp) · [tool reference](https://github.com/oOo0oOo/lean-lsp-mcp/blob/main/docs/tools.md) · [Numina-Lean-Agent](https://arxiv.org/pdf/2601.14027) — agent-facing LSP as the exclusive mediator for formal interaction

*Backends considered and not taken — Strata/Laurel, Verus, hax/Aeneas, Dafny, Kani — with
the evidence behind each decision: [`verification_backends_considered.md`](./verification_backends_considered.md).*
