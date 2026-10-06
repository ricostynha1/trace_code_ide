# Evidence

**A level is earned, never annotated.** This is the rule an agent breaks first,
so it is the one to internalise: there is no annotation that sets an evidence
level, and writing `L3` in a comment claims something you did not establish.

## How a level comes to exist

1. A backend **runs something** — a differential comparison, a proof check, a
   person entering a judgement.
2. It writes a **record** into the project's evidence store (commonly
   `.tracelean/evidence/`). The record names the inputs it depended on: the
   model it was checked against, the implementation it compared, the toolchain
   that accepted it.
3. A **lock** command folds the store into the committed index, dropping
   whatever went stale.

Because each record names its inputs, changing any of them makes the record
stale rather than leaving it standing. That is the whole mechanism: you cannot
edit the code and keep the evidence.

```bash
# run whatever backend earns the level, then:
tracelean-trace . --lock     # or this project's equivalent
```

## The three bonds

Graded separately, aggregated by **minimum**:

| Bond | Earned by |
|---|---|
| requirement ↔ model | a person judging that the function means what the sentence says |
| model ↔ implementation | a differential run that agreed *and* met its floors |
| model property | a theorem the kernel accepted |

A clause with a proof and no judgement is worth what the judgement is worth,
which is nothing until somebody makes one. Do not be surprised when a clause you
just proved still reads low: the proof bond is one of three.

## The levels

| Level | Earned when |
|---|---|
| `L1` | The annotation resolves. Nothing was checked. This is the floor, not a failure. |
| `L2` | A person judged it. Also the ceiling for anything `@structural`. |
| `L3` | Differential testing found no disagreement over a stated case count **that met the floors the binding declared**. |
| `L4` | A theorem discharges the property. |

L3 has two halves and they are usually produced by different processes — a
generation test counts situations, a differential test reports agreement. They
are composed through a pending area, and whichever lands second writes the
record. If a clause you expect to be L3 is not, check that **both** halves ran.

## What you as an agent can and cannot earn

- **You can** run differential suites and proof checks and then lock. Those are
  machine-earned and your run counts the same as anyone's.
- **You cannot** enter the requirement ↔ model judgement. That bond is a
  person's: it is the question of whether a formal function means the same thing
  as an English sentence, and the method deliberately does not let a machine
  answer it. Projects usually provide a way to export the prompt and accept a
  decision. Export it, leave it for a person, and say so.

If you are asked to "raise the evidence", the honest moves are: write a missing
model, add a missing binding, declare missing floors, write a missing proof, run
the suites, lock. The dishonest move is to mark things exempt or structural.

## Strength

Some projects additionally track whether a model is **pinned** — whether
somebody proved the model is strong enough that a wrong implementation could not
satisfy it. An unpinned model is *open* by default, and the obligations are
generated and left unproved. Do not mark something pinned that you did not pin.

## Staleness

When a record's inputs change, the record goes stale and the lock drops it. This
is normal and expected after any real change. A run of the checker after a change
that reports fewer established clauses than before is usually correct, not a
regression to hide.

The one thing to actually check is whether the drop is *larger* than your change
justifies — that usually means something shared moved, and it is worth knowing
what.
