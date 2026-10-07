# Spec strength in Lean: porting `|A(i,o)| = 1`

The idea, from Dafny: a contract is *strong* when, for a given input `i`, the
set of outputs `o` satisfying it has exactly one element. `|A(i,o)| = 1` means
the specification determines the answer; anything larger means the specification
permits behaviours you did not intend to permit.

It ports to Lean. But the thing it should be pointed at here is not the model.

---

## 1. Why the direct port is vacuous for TraceLean

Dafny contracts are **relations**: `ensures` clauses constrain an output that the
body computes. Two different bodies can both satisfy the same `ensures`, so
asking how many outputs satisfy it is a real question.

TraceLean's models are **functions**:

```lean
def price (o : Order) : Priced :=
  let discount := discountCents o.subtotalCents
  ...
```

`price o` is a value. `|A(i,o)| = 1` holds by construction for every total
function, so measuring it on the model tells you nothing. The model is already
maximally strong in that sense — that is what makes it executable, and executable
is what differential testing needs.

## 2. Where the question is real: the `@proves` set

The relational specification in TraceLean is not the model. It is the set of
theorems proved *about* the model — the `@proves` role. Those are exactly Dafny's
`ensures` clauses, written as separate lemmas rather than attached to a body.

So the ported question is:

> Do the theorems we proved determine the function, or merely constrain it?

Formally, with `Spec` the conjunction of the proved properties as a predicate
over candidate functions:

```lean
theorem spec_pins : ∀ f g, Spec f → Spec g → f = g
```

Provable ⇒ the property set is strong: anything satisfying it *is* the model, up
to `funext`. Not provable ⇒ there is slack, and the slack is precisely the
behaviour your proofs do not rule out.

## 3. Three mechanisms, cheapest first

### (a) Mutation, and rebuild the proofs

The cheapest useful approximation, and it needs no new theory. Mutate the model,
run `lake build` on the proof file, and see whether the proofs still go through.

- Proofs **break** ⇒ they were sensitive to that behaviour. Good.
- Proofs **still compile** ⇒ the mutant is a second inhabitant of `Spec`. Your
  property set does not distinguish the model from the mutant, and the mutant is
  a concrete witness of the weakness.

This is mutation testing pointed at proofs instead of tests, and TraceLean
already has the machinery: it is the same procedure used to show that boundary
edge values matter for differential testing.

A mutant that survives **both** the proofs and the differential run is the most
interesting artifact the system can produce: a behaviourally distinct model that
the entire evidence chain cannot tell from the real one.

### (b) Exhibit a second witness

Far easier than proving uniqueness, and it is what you want anyway when the
answer is "weak":

```lean
def Spec (f : Nat → Nat) : Prop := ∀ s, f s ≤ s      -- the proved property

def zero : Nat → Nat := fun _ => 0
example : Spec zero := fun s => Nat.zero_le s        -- satisfied
example : zero ≠ discountCents := by                 -- and different
  intro h; have := congrArg (· 20000) h; simp [discountCents] at this
```

Two lines, and the property set is refuted as a specification.

### (c) The literal count, for finite domains

The closest port of `|A(i,o)|` itself. When the output type is a `Fintype`, or
the search space is bounded, the cardinality is computable:

```lean
def solutions (i : Input) : Finset Output :=
  Finset.univ.filter (fun o => decide (valid i o))

example : (solutions sampleInput).card = 1 := by decide
```

`decide` turns the strength question into a kernel computation. This works for
small enumerable outputs and does not scale to `Nat`, but it is the honest
version when it applies. Mathlib's `Plausible` (property-based counterexample
search) covers some of the middle ground: ask it to find `o₁ ≠ o₂` both
satisfying `valid i`, and let it hunt.

## 4. What this measures on *our own* example — and it is not flattering

`example/formal/CheckoutProofs.lean` proves three things:

| Theorem | Constrains |
|---|---|
| `discount_le_subtotal` | `discountCents s ≤ s`, for all `s` |
| `free_above_threshold` | `shippingCents a false = 0` when `a ≥ 10000` |
| `remote_surcharge_survives_free_shipping` | `shippingCents a true = 400` when `a ≥ 10000` |

Measured by strength:

- `fun _ => 0` satisfies `discount_le_subtotal`. The constant-zero discount
  function — no tiers, no rounding, no discount at all — is indistinguishable
  from the model by this property.
- Below 10,000 cents the two shipping theorems say **nothing**. Any function
  whatsoever agrees with them there.

So the example's L4 evidence is real (the proofs are machine-checked and the
first one rules out a genuine `Nat` underflow), and its *strength* is close to
zero. Both of those are true at once, and only the second is currently invisible.

## 5. Why TraceLean should care

The evidence ladder qualifies L3 and does not qualify L4.

- **L3** requires a coverage floor: a run that found nothing must also have been
  diverse enough to have had a chance of finding something. A million cases down
  one branch establish nothing, and the system says so.
- **L4** requires only that *a* theorem exists and compiles. Proving
  `discount ≤ subtotal` and proving the function is what it is both read as L4.

Spec strength is the missing L4 qualifier — the exact analogue of the coverage
floor, one rung up. The cheap version (mutation + rebuild) is implementable now
and reports in the same shape as everything else: not a score, but a witness.
"These three mutants survive your proofs" is actionable in a way that a number
is not.

## 6. Caveats worth stating

- **Uniqueness is not correctness.** A specification can uniquely determine the
  wrong output. Strength says the spec leaves no freedom; it says nothing about
  whether the thing it pins down is what the requirement asked for. That is the
  `req ↔ model` bond, and it is judged, not proved.
- **Uniqueness is the wrong goal for a deliberately partial spec.** A clause
  marked `@partial` is *supposed* to leave freedom. The right question there is
  whether the spec is determined on the domain the requirement covers.
- **Undecidable in general**, which is why (a) and (b) — falsification with a
  witness — are the practical forms, and (c) applies only to finite domains.

---

## 7. What is built

`tracelean/core/src/trace/strength.rs`.

**The obligation is generated; the proof is not.** For each `@models`
declaration with `@proves` theorems attached, TraceLean abstracts those
theorems' statements over the model symbol, assembles them into a `Spec`
predicate, and states the uniqueness obligation with `sorry` where a proof goes.
Grouping is by *declaration*, not by clause: `shippingCents` models three
clauses of REQ-SHIPPING, and the strength question is about the function, so it
is asked once and every theorem about it counts.

The generated file also proves that the model satisfies its own abstracted
specification. That is a guard on the generation: the predicate is built by
rewriting theorem statements textually, and if the rewriting went wrong, this
stops elaborating instead of quietly producing a weaker question.

**Four states, and `open` is not a pass:**

| State | Means |
|---|---|
| `pinned` | A `@pins` theorem exists and the kernel accepted it with no `sorry`. |
| `attempted` | The obligation is written; a `sorry` remains inside it. |
| `open` | Nobody has said anything. The default. |
| `nondeterministic` | Declared unpinnable, with a reason. |

`@pins` is a new role, alongside `@proves`. `@nondeterministic reason="…"` is a
new qualifier, in the same family as `@partial` and `@exempt`: some functions
genuinely are not determined by their declared inputs — anything drawing on
randomness, a clock, or the order of concurrent events — and saying so once with
a reason beats an obligation that stays open forever and teaches everyone to
ignore the column.

A claim is checked, never trusted: `strength::check` runs `lake env lean` on the
file holding each `@pins` theorem and maps the `sorry` warnings back to
declaration spans by line. Without a Lean toolchain the answer is "claimed, not
checked" rather than a pass — the same rule the judge and the differential
runner follow.

## 8. Measured on the example

Running it produced exactly the split this document predicted, then changed the
example:

```
REQ-SHIPPING.free    -> pinned
REQ-SHIPPING.flat    -> pinned
REQ-SHIPPING.remote  -> pinned
REQ-DISCOUNT.tiers   -> attempted
REQ-DISCOUNT.rounding-> attempted
REQ-CHECKOUT.total   -> open
REQ-RECEIPT.lines    -> open
```

`shippingCents` started weak: `free_above_threshold` and
`remote_surcharge_survives_free_shipping` constrain nothing below 10,000 cents,
so any function at all agreed with them there. The obligation made that visible,
two more theorems were written (`flat_below_threshold`, `remote_below_threshold`),
and `shippingCents_pinned` now goes through — the four properties partition the
input space, so two functions satisfying them agree everywhere. That is the loop
working: a measurement that changed what got proved.

`discountCents` is still `attempted`, and it will stay that way until somebody
strengthens it, because `discountCents s ≤ s` is satisfied by `fun _ => 0`. The
obligation is not merely unproven — it is *false*, and the useful next move is
the refutation rather than the proof.

## 9. Still to do

- Show it live beside coverage in the traceability panel. The state is computed;
  nothing renders it yet.
- A finding when a clause is `L4` and `open`, so the gap is reported rather than
  waiting to be looked for.
- Mutation as a cheaper approximation: perturb the model, rebuild the `@proves`
  file, and report the mutants that survive. It needs no uniqueness proof and
  gives a witness, which makes it the right first answer for a large model.
