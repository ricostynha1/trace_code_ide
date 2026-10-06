# Making a change

The loop, in order. Steps you skip cost more later than they save now.

## 0. Know what clause you are changing

Before writing anything, name the clause. `grep` for it — that returns the
model, the implementation, the tests and the binding at once, because in this
method that is what a clause *is*.

If there is no clause for what you were asked to do, you have a decision to
make, and it is not a silent one:

- The change is within an existing clause → proceed.
- The change is a new obligation → write the clause first
  ([03-requirements.md](03-requirements.md)), and expect the new findings that
  come with it.
- You were asked for something the requirements forbid → say so before
  building it.

## 1. Fix the thing

Ordinary work. Match the surrounding code: its naming, its comment density, its
idioms. A TraceLean tree usually has unusually explanatory comments — comments
that say *why*, and name the failure the code prevents. Write in that register
if the tree does; a terse patch in a discursive codebase reads as unfinished.

Put the decision in a function that can be called, not in the middle of an
effect. That is what makes it modellable, and it is what the next step needs.

## 2. Annotate what you wrote

Every function you added that realises part of a clause gets an `@implements`.
Every test gets a `@tests`. See [02-annotations.md](02-annotations.md).

## 3. Model it, if it is a value

If the clause is about a value and has no model, this is when you write one
([04-models-and-bindings.md](04-models-and-bindings.md)). Build the model
package. Add the binding. Declare the floors.

If the clause is about the tree, write the test that reads the tree, and mark it
`@structural` with a reason.

## 4. Write the test that would have caught the bug

This is the step that separates a fix from a repair.

Ask: **why did the existing suite not catch this?** The answer is almost always
that every test called the function at the tail of one arrow, and the failure
was in a join nothing called. Whatever that join is — a translation between two
vocabularies, a configuration two files have to agree about, a sequence rather
than a single step — write the test that exercises *it*.

Two rules for that test:

- **Do not supply the value the code was supposed to produce.** A test that
  hands the system the correct intermediate form can never discover that nothing
  produces it. This is the single most common way a whole-system bug survives a
  green suite.
- **Read the answer from the outside.** Where you can observe what the system
  actually did — the bytes it wrote, the screen it painted, the file it left —
  prefer that to asking the system what it thinks it did. A component that
  reports one thing and does another passes the first check and fails the
  second.

Then **verify the test has teeth**: reintroduce the bug, watch the new test
fail, and put the fix back. A test you have never seen fail is a test you have
not written.

## 5. Run the checker

```bash
tracelean-trace .        # or this project's equivalent
```

Compare to what it said before you started. New `Unmodeled` from a clause you
added is expected. New `Dangling` means you named something that does not exist.
New `Unbound` means you wrote a model and an implementation and no binding —
finish it.

## 6. Run the suites

The fast one always. The slow one — differential, behind a flag — when you
touched a model, an implementation under a binding, or a schema.

## 7. Earn and lock

Run whatever backend earns evidence for what you changed, then lock
([05-evidence.md](05-evidence.md)). Expect records to have gone stale.

## 8. Update the documentation that describes it

Documents declare what they describe and carry a hash of it. Changing a
requirement puts its documents into review — that is the mechanism telling you
to reread them, not a nuisance. Find them and update them.

If the project keeps a progress or status document, update it. What you left
undone belongs there, written plainly.

## 9. Say what you did and what you did not

Report honestly. If a suite failed, say so and show it. If you skipped a step,
say which and why. If part of the scope turned out to be blocked, finish
everything else and name what you left.

## The short version

```
name the clause  →  fix it  →  annotate it  →  model it  →
write the test that would have caught it  →  see it fail  →
check  →  test  →  lock  →  document  →  report
```
