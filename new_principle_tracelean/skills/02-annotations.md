# Annotations

Annotations are the **only** mechanism linking anything to anything. There is no
requirements directory that is special by being called that, no specification
directory, no filename convention. Moving a file changes nothing about what it
claims.

An annotation is recognised only inside a comment of the host language, and
never inside a string or character literal.

## The six roles

| Role | Goes on | Says |
|---|---|---|
| `@models` | a function in the formal model | this function is what the clause means |
| `@implements` | a function in the implementation | this code claims to realise the clause |
| `@tests` | a test | this test exercises the clause |
| `@drt` | the entry point of a differential comparison | this clause is compared by generated cases |
| `@proves` | a theorem | this theorem discharges the clause as a property |
| `@pins` | a proof about a model | this model is pinned — see the project's strength rules |

Nothing else parses as a role. A role-shaped token that is not one of these is
reported as an unknown role rather than ignored.

## Writing one

```rust
/// Celsius to Fahrenheit.
///
/// @implements REQ-CONVERT.scales_round_trip
pub fn to_fahrenheit(degrees: f64) -> f64 { … }
```

```lean
/-- Converting between the scales.

@models REQ-CONVERT.scales_round_trip -/
def toFahrenheit (degrees : Float) : Float := …
```

```rust
/// @tests REQ-CONVERT.scales_round_trip
#[test]
fn converting_back_gives_what_went_in() { … }
```

The identifier is `REQ-ID.clause_key`, spelled exactly as the requirement
document spells it. A clause that does not exist is reported as `Dangling`, and
dangling annotations usually block the build.

## Where it attaches

An annotation attaches to the **nearest following declaration**. That means a
function, a type, a theorem — whatever the host language's grammar calls a
declaration. Put it in the doc comment of the thing it is about.

If the file does not parse cleanly, annotations in or after the unparsed region
anchor to the whole file instead and are reported as `Imprecise`. That is not an
error; it caps what those claims are worth, because a claim about "somewhere in
this file" is weaker than a claim about a function. If you hit it, the usual fix
is to write the declaration in a form the grammar handles — not to change what
the code means.

## Qualifiers

A qualifier attaches to the nearest preceding annotation, or carries its own
identifier.

| Qualifier | Requires | Means |
|---|---|---|
| `@partial` | `reason=` | this claim covers part of the clause |
| `@nondeterministic` | `reason=` | this model cannot be pinned, and here is why |
| `@structural` | `reason=` | the clause is about the tree, not about a value |
| `@exempt` | `reason=`, `by=`, optionally `until=` | the clause is out of the denominator |

```rust
/// @tests ARCH-THING.no_network_calls
/// @structural ARCH-THING.no_network_calls reason="a constraint on what the source may mention; a function that made a call would satisfy any model of itself"
#[test]
fn nothing_here_reaches_the_network() { … }
```

**`@structural` is for clauses that genuinely have no function.** *Nothing here
calls a model.* *The index this tool produces for its own tree equals the one
the bootstrap tool produces.* These are checked by a test that reads the
repository. A structural clause needs a `@tests` and nothing else, stays in the
coverage denominator, and caps at L2.

**Do not reach for `@structural` to silence a finding on a clause that does have
a function.** It caps the clause's evidence, so applying it to a modelled clause
makes the project's own figures worse while looking like progress.

**Do not reach for `@exempt` at all** unless a person has actually approved it.
An exemption without a reason and an approver, or past its expiry, is itself
reported as `UnsoundExemption` and usually blocks.

## What two annotations on one clause mean

Several functions may `@implements` the same clause — a clause is often realised
by more than one place. That is fine and common. What is not fine is two links
*exclusively* claiming the same clause, which is reported as `Contested`.

## Languages without a grammar

A project's checker understands a fixed set of languages. Code in a language it
has no grammar for cannot carry an annotation, because there is nothing to
anchor it to. The convention is:

1. Write the claim in prose in the file's header comment — "Realises
   `REQ-X.clause`" — so a reader of that file knows.
2. Put the actual annotation on the **test** that checks it, in a language the
   grammar does handle, and say in that test's comment that it carries the claim
   on the other file's behalf.

Look for how the project already does this before inventing your own way.

## The rule to remember

If you wrote code and did not write an annotation, you have added something the
project cannot account for. Either annotate it, or you are writing the wrong
thing — go and write the clause first.
