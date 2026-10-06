import TraceLean.Evidence

/-!
# Discharged strength obligations

A proof annotation says a model has a property, not how much that property rules
out. `REQ-STRENGTH` asks the follow-up question: taking the proved theorems together
as a specification, is this model the *only* thing satisfying them, or is there
slack the proofs never ruled out?

TraceLean generates that obligation and deliberately does not discharge it
(`REQ-STRENGTH.not_proved_by_us`). This file is the other side: obligations a
person has actually proved. Each one is the generated scaffold with the `True`
placeholders replaced by what the theorems really say, and the `sorry` replaced
by a proof.

Two rules each obligation here follows, because breaking either would turn the
column into decoration:

- The specification is **exactly** the theorems annotated `@proves` for the
  clause — no extra conjunct added to make the uniqueness proof go through. An
  added conjunct pins a model the project never proved.
- The model is shown to **satisfy** its own specification. A specification
  nothing satisfies makes `∀ f g, Spec f → Spec g → f = g` vacuously true, which
  would be the easiest possible way to report a model as pinned while proving
  nothing at all.
-/

namespace TraceLean.Pinned

open TraceLean.Evidence

namespace Level

/-- `toNat` tells the four levels apart, so it reflects equality. -/
theorem toNat_inj {a b : Level} (h : a.toNat = b.toNat) : a = b := by
  cases a <;> cases b <;> first | rfl | exact absurd h (by decide)

/-- The order on levels is antisymmetric.

Needed because pinning a minimum is exactly an antisymmetry argument: two
functions that are each below the other are the same function. -/
theorem le_antisymm {a b : Level} (h1 : a ≤ b) (h2 : b ≤ a) : a = b :=
  toNat_inj (Nat.le_antisymm h1 h2)

end Level

/-- Membership in the chain names the bond it came from.

`chain` is `allBonds.map (bondLevel rs)`, so a level in it is some bond's level;
this is that fact in the form the uniqueness proof consumes. -/
theorem mem_chain_gives_a_bond {x : Level} {rs : List Record}
    (h : List.Mem x (chain rs)) : ∃ b, x = bondLevel rs b := by
  -- Walked constructor by constructor rather than through `List.mem_map`, in
  -- the subset of Lean the annotation grammar reads (ADR-0008) and matching how
  -- `assurance_mem_chain` builds the membership in the first place.
  simp only [chain, allBonds, List.map] at h
  cases h
  case head => exact ⟨Bond.requirementModel, rfl⟩
  case tail h1 =>
    cases h1
    case head => exact ⟨Bond.modelImpl, rfl⟩
    case tail h2 =>
      cases h2
      case head => exact ⟨Bond.modelProof, rfl⟩
      case tail h3 => cases h3

/--
The specification `assurance` actually carries.

Both conjuncts are theorems this project proved about it and annotated as
proving `REQ-EVID.weakest_link`: `assurance_le_bond` and `assurance_mem_chain`.
Nothing else is added.

The directive is spelled out in words here rather than quoted. A
directive-shaped token in prose is a directive as far as a scanner is concerned,
and the two kernels read this file differently until it was removed.
-/
def SpecAssurance (f : List Record → Level) : Prop :=
  (∀ rs b, f rs ≤ bondLevel rs b) ∧ (∀ rs, List.Mem (f rs) (chain rs))

/-- The model satisfies its own specification, so the obligation below is not
vacuous. -/
theorem assurance_satisfies_its_spec : SpecAssurance assurance :=
  ⟨assurance_le_bond, assurance_mem_chain⟩

/--
The specification determines the model.

Read plainly: a function that is below every bond *and* is always one of the
bond levels has no freedom left. Being one of them makes it some `bondLevel rs
b`; being below every bond makes any rival at most that; the argument is
symmetric, and antisymmetry finishes it.

So the two theorems this project proved about `assurance` do not merely
constrain it — they pin it. `min` was not an arbitrary choice among aggregates
that satisfy them; it is the only one.

TraceLean's generated obligation for this symbol lists three theorems, the third
being monotonicity under `REQ-EVID.monotone`. It is deliberately **not** a
conjunct of `SpecAssurance`. Leaving a conjunct out makes the specification
weaker, so uniqueness under it is the stronger claim — the model is pinned by
two of its theorems and would still be pinned with the third added. The rule
this file follows is that no conjunct may be *added* beyond what was proved;
proving more with less is not the failure that rule guards against.

@pins REQ-EVID.weakest_link
-/
theorem pins_assurance (f g : List Record → Level)
    (hf : SpecAssurance f) (hg : SpecAssurance g) : f = g := by
  funext rs
  have hfb := mem_chain_gives_a_bond (hf.2 rs)
  have hgb := mem_chain_gives_a_bond (hg.2 rs)
  refine hfb.elim (fun bf hbf => hgb.elim (fun bg hbg => ?_))
  have below : g rs ≤ f rs := by
    rw [hbf]
    exact hg.1 rs bf
  have above : f rs ≤ g rs := by
    rw [hbg]
    exact hf.1 rs bg
  exact Level.le_antisymm above below

end TraceLean.Pinned
