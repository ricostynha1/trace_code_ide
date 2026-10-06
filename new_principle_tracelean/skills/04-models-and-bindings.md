# Models and bindings

A model says what a clause means as a computation. A binding declares that a
model function and an implementation function answer the same question, so
generated inputs can be put through both.

## Before you write one: check it does not exist

The mistake an agent makes here is not writing a bad model. It is writing a
second model of something the tree already has. Adding compiles, adding passes,
and a duplicate reads as thorough — nothing about the immediate feedback tells
you that you have just created two accounts of one concept that will now drift
apart forever.

So before writing:

```bash
grep -rn "the concept, in the words the tree would use" <the model directory>
```

and read the module list. If something already computes what you are about to
compute, **reuse it** — import it and call it.

If you decide the existing one is genuinely different, say so **in the new
model's doc comment**, naming the one you looked at and why the two are not the
same thing. That sentence is the deliverable. A reviewer can check it, and its
absence is how a tree ends up with one concept under two names and no compiler
that will ever object.

If the project ships a report for this (see its ideas document), run it.

## What makes a good model

**A model is a function, not a restatement.** If you find yourself writing a
Lean definition whose body is the clause's sentence turned into an `if`, you
have written a tautology that proves nothing. The test is: could the
implementation disagree with it? If not, it is not a model.

**Decision, not effect.** A function that reads a clock, walks a directory or
consults a cache cannot be compared against anything, because its answer depends
on things the question did not contain. So separate the part that *decides* from
the part that *does*:

- *"Which paths may be written"* becomes a function from a path to a
  classification — modelled, compared.
- *"Write the file"* is the shell around it, and carries no decision.

If the code you were asked to change has a decision buried inside an effect,
pulling the decision out is the first half of the work.

**Effects that carry a law.** Some things genuinely are effects and still have
something to say — "reading back what was written gives what was written". The
convention is to declare them as opaque constants with the law written once over
a witness, in one designated file. Find how the project already does this rather
than inventing a second way.

**Total.** A model that can fail on some inputs cannot be compared on those
inputs. Prefer a total function answering with an explicit "nothing" over one
that is undefined.

**Share, do not duplicate.** If your new model needs what another model already
computes, import it. A second account of the same thing is a second thing to
keep in step, and the two will drift.

## Writing one

```lean
import Project.Keymap

namespace Project

/-- What a sequence of keys does.

@models REQ-DRIVE.session_is_a_value
@models REQ-DRIVE.walk_follows_the_machine -/
def drive (keymap : Keymap) (mode : String) : List String → List Step
  | [] => []
  | key :: rest => …
```

Then build the model package (`lake build` for Lean) before anything else. A
model that does not compile is not a model.

### Watch for constructs the grammar cannot parse

The checker anchors annotations using a grammar for the model's language, and
that grammar is usually less capable than the compiler. A declaration it cannot
parse makes every annotation in or after it `Imprecise` — still recorded, capped
at the lowest level.

If you get `Imprecise` on a file you just wrote, ask the project's checker where
it stopped parsing (there is normally a flag for this — look for something like
`--unparsed <file>`), then rewrite that declaration in a plainer form. In
practice this means preferring ordinary signatures and simple tactic blocks over
exotic notation. Changing the shape of a proof is fine; changing what it states
is not.

## Adding a binding

A binding is a declaration in a configuration file (commonly
`.tracelean/drt.json`) that names a call and nothing more:

```json
{
  "req_id": "REQ-DRIVE",
  "clause": "session_is_a_value",
  "floors": [
    { "situation": "a session of no keys", "atLeast": 20 },
    { "situation": "a key that entered a mode", "atLeast": 50 }
  ],
  "also_checks": ["walk_follows_the_machine", "menu_is_the_bar_there"],
  "model": {
    "import": "Project.Drive",
    "function": "Project.drive",
    "arguments": ["keymap", "mode", "keys"]
  },
  "implementation": {
    "language": "rust",
    "entry": "src/surface/drive.rs::drive_of"
  }
}
```

- **`also_checks`** names other clauses the same call checks. They are as much a
  claim as the primary one and are held to the same rule: annotated, or not
  claimed.
- **`floors`** are what makes a passing run mean something. See below.
- The runners for both sides are **generated and built by the tool**. You do not
  write them. If you find yourself writing a conformance runner by hand, you
  have missed the mechanism.

Both sides usually need a wrapper in the shape the runner calls — taking owned
arguments matching the declared names, in order. Follow the existing wrappers'
naming convention in that project.

The annotation and the binding must agree: a `@drt` annotation claims a clause
is differentially tested, and a binding is what makes that true. Projects
normally have a test asserting the two sets are identical, so adding one without
the other fails.

## Declaring the input schema

The suite generates inputs from a declared schema. Two things matter:

- **Reach the interesting cases.** Generating keymaps with three modes and two
  bindings each finds more than generating one enormous one. Keep the sizes
  small and the *shapes* varied.
- **Seed the examples with the values that have been wrong before.** If a key
  name was mishandled once, put it in the examples.

## Coverage floors: the half people skip

Agreement alone is not evidence. If every generated case took the same trivial
branch, the run agrees with an implementation that does nothing.

So a binding **declares the situations it must reach and how often**, and a run
is judged against them. A situation is a predicate over generated values, which
cannot live in JSON — so the binding names it and the generation test counts how
often the name was reached. Neither half alone is enough: agreement without a
met floor is not L3.

Write floors for:

- each branch of the outcome the function can produce;
- the empty and the degenerate case;
- the case that was actually wrong before.

A binding with no declared floors answers "undeclared" and stays at the lowest
level, however many cases it ran. That is honest, and it is also a to-do.

## Running it

Differential runs build a model package and a runner package, so they are slow
and usually behind a flag. Find the project's incantation — in a Rust project it
is typically:

```bash
cargo test --workspace -- --ignored
```

Run the one binding you added first, then the suite.

## When they disagree

A divergence is shrunk to its smallest case and reported. Read it before
deciding which side is wrong. Roughly half the time the model is right and the
code is wrong; the rest of the time the model said something the requirement did
not ask for. Both are useful findings. What is never right is to change the
model so it agrees with the code without checking the clause — that converts a
real finding into a silent one.
