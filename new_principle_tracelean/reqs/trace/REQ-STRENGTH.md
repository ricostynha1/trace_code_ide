---
id: REQ-STRENGTH
title: Proof honesty and spec strength
refines: [ARCH-HONEST, REQ-EVID]
status: approved
decomposition: complete
clauses:
  proof_states_the_model: The system shall give the proved mark only for a theorem whose statement applies the clause's model, and shall refuse any other theorem as stating nothing about the model.
  proof_is_general: If no application of the model in a theorem's statement takes an argument bound by that statement, then the system shall refuse the theorem as an example rather than a proof.
  proof_is_direct: If a theorem's statement, with the model replaced by an arbitrary function, still depends on the model through another definition, then the system shall refuse the theorem and name that definition.
  proof_not_vacuous: If the theorem's own proof still checks with the model replaced by an arbitrary function, or the checker's bounded proof attempt closes its statement for an arbitrary function in place of the model, then the system shall refuse the theorem as holding of every function.
  model_is_a_function: Where a clause's model is a type rather than a function, the system shall refuse that clause's theorems for the proved mark.
  axioms_declared: The system shall accept a theorem only when every axiom it depends on is one of Lean's own (propext, Classical.choice and Quot.sound) or a project axiom whose doc comment carries @assumes, and shall refuse every other axiom, sorryAx and Lean.ofReduceBool among them, by name.
  assumptions_shown: The system shall show, with each proved mark, the declared axioms its proof depends on.
  kernel_decides: The system shall decide every acceptance by running Lean on the built models, and shall not decide it from the text of the source.
  refusal_reported: When a theorem is refused, the system shall report it as a finding naming the theorem, its clause and the reason.
  obligation_generated: The system shall generate, for each model, the obligation that the statements of its accepted theorems, with the model replaced by an arbitrary function, together determine that function to be the model.
  not_proved_by_us: The system shall generate the obligation and shall not attempt to discharge it.
  pinned_by_theorem: The system shall report a model pinned only when Lean accepted a theorem annotated pins for one of its clauses whose statement is that model's generated obligation, under the same axiom rules.
  strength_per_model: The system shall present the strength of each clause as the strength of its model.
  open_is_the_default: Where a model has no pinning attempt, the system shall report it as open, and shall not present open as a pass.
  attempted_distinguished: The system shall distinguish an obligation stated but not accepted from one never attempted and from one discharged.
  nondeterministic_declared: Where a model cannot be pinned, the system shall let it be declared as such with a reason, and shall then exclude it rather than report it as open forever.
  qualifies_proof: The system shall present the proved mark together with its strength, in the way the tested mark is presented together with its coverage.
  verdict_kept: When the model, an accepted theorem about it or the pinning theorem changes, the system shall return the model to attempted.
---

# Proof honesty and spec strength

A green build is not a proof of a clause. A theorem can be true and say nothing
about the model: about another function, about four fixed inputs, about the
model reached through a wrapper whose behaviour the theorem then fixes, or about
anything at all (`dispatch a f = dispatch a' f'` given `a = a'` holds of every
function). Each of these was found among the theorems that earned `P`, so the
checker asks Lean, not the source text, five questions of each `@proves`
theorem: does it apply the model, to something it quantifies over, directly,
non-vacuously, and on which axioms? A proof that fails one earns nothing and is
reported, with the reason, as a finding.

The vacuity test has two halves. First, the theorem's own proof is taken with
the model replaced by an arbitrary function `g`: if it still checks, the proof
never used what the model computes (`cases classify path <;> simp` proves that
any function into four classes returns one of them). Second, a bounded attempt
(`intro`, `trivial`, `rfl`, `simp_all`, `decide`) tries to prove the statement
for `g` directly. Either can miss a vacuous theorem, but neither refuses a true
one wrongly: if the statement holds of every `g`, it says nothing about the
model.

Axioms are allowed where the world is: a sandbox copy or a transcript read has
no Lean definition, so its law is assumed. Such an axiom says so with `@assumes`
in its doc comment, and every `P` that leans on it lists it.
`native_decide` (`Lean.ofReduceBool`) trusts the compiler, not the kernel, and is
refused.

## Strength

A proof says the model has a property, not how much that property rules out:
`discount s ≤ s` is a theorem the constant-zero function also satisfies. The
specification of a model is the set of its accepted theorems, with the model
abstracted to `g`; it is strong when only the model satisfies it:

```
∀ g, S₁ g → S₂ g → … → Sₙ g → g = f
```

The checker writes this obligation from the accepted statements themselves, so
it cannot be weakened by restating it, and checks a `@pins` theorem against it
exactly. A pinned model leaves no room for a wrong implementation that the
differential tests could miss and no room for a wrong model that the judged
clauses would not show: every behaviour of the model is one of its clauses. Any
change to the model or to one of those theorems re-opens the question.
Discharging it is the author's part; until then the model is open, never fine.
