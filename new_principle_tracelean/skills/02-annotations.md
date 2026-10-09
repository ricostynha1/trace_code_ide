# Annotations

The **only** link between things. Recognised only inside comments; attached to
the **nearest following declaration** (put it in that item's doc comment); in a
Lean constructor's or field's doc comment, to the enclosing inductive or
structure. Moving a file changes nothing.

| Role | On | Says |
|---|---|---|
| `@models` | the one function computing what the clause talks about | this is what the clause means |
| `@specifies` | a `def … : Prop` beside the model | which answers are right |
| `@implements` | code | this realises the clause |
| `@tests` | a test | this exercises the clause |
| `@drt` | a differential comparison's entry | compared by generated cases |
| `@proves` | a theorem | discharges the clause as a property |
| `@pins` | a proof that the spec pins the model | the model is pinned |

At most one `@models`, one `@specifies` and one `@pins` per clause (ADR-0014);
more is reported as `SeveralModels`/`SeveralSpecs`/`SeveralPins`.

Unknown role-shaped tokens are reported, not ignored.

```rust
/// @implements REQ-CONVERT.scales_round_trip
pub fn to_fahrenheit(degrees: f64) -> f64 { … }
```

The id is `REQ-ID.clause_key`, spelled as the document spells it; a wrong one
is `Dangling` (usually blocks). Several items may implement one clause; two
*exclusive* claims are `Contested`.

If a file does not parse, annotations after the unparsed point anchor to the
whole file (`Imprecise`, capped at L1). Rewrite the declaration more plainly;
do not change its meaning.

## Qualifiers

| | Needs | Means |
|---|---|---|
| `@partial` | `reason=` | covers part of the clause |
| `@nondeterministic` | `reason=` | the model cannot be pinned |
| `@structural` | `reason=` | the clause is about the tree, not a value; needs only `@tests`; caps at L2 |
| `@exempt` | `reason=`, `by=`, `until=`? | out of the denominator — only with a person's approval |

Never use `@structural` on a clause that has a function, nor `@exempt` without
approval, to silence a finding.

## No grammar for the language?

State the claim in prose in the file header, and put the annotation on a test
in a supported language that checks it. Copy how the project already does it.
