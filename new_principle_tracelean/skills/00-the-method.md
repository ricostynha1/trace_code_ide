# The method

A project using TraceLean makes a specific claim about itself, and ships the
machinery to check it. Four things exist, and three arrows join them:

```
requirement ──@models──► formal model ──@implements──► implementation
   (a clause,              (a function                   (a function
    in prose)               in Lean)                      in code)
                                │                              │
                                └──────── compared ────────────┘
                                     differential testing
```

- A **requirement** is a document with an identity and a set of named
  **clauses**. A clause is one sentence that can be true or false of the system.
  It is referred to as `REQ-THING.clause_name`, and that string is what every
  annotation writes.
- A **model** is a function in Lean that says what a clause means as a
  computation from data to data. Not a restatement of the sentence — a function
  you can run.
- An **implementation** is the real code, in whatever language the project is
  written in.
- A **binding** is a declaration that a particular model function and a
  particular implementation function answer the same question, so that generated
  inputs can be put through both and the answers compared.

Nothing in that diagram is inferred. Every arrow is a comment somebody wrote.

## Why it is built this way

The failure this prevents is not "missing tests". It is a project where the
documentation, the specification and the code have each drifted from the others
and nothing notices, because nothing ever compared them. Prose cannot be
compared to code. A function can.

So the specification is executable, and "the code does what the spec says" stops
being a claim and becomes a run: generate inputs, feed both, compare answers,
shrink any disagreement to its smallest case.

## Three bonds, never averaged

A clause's evidence has three independent parts:

| Bond | Is the question |
|---|---|
| requirement ↔ model | does this function mean what the sentence says? |
| model ↔ implementation | do these two agree on every input we tried? |
| model property | is there a theorem about it? |

They are graded separately and aggregated by **minimum**. A clause with a
beautiful proof and no connection to its requirement is worth what the weakest
bond is worth. Averaging would let a strong bond hide a missing one, which is
exactly the self-flattery the method exists to prevent.

## Four levels

| Level | Means |
|---|---|
| `L1` | The annotation resolves. Nothing was checked. |
| `L2` | A person judged it and said so. |
| `L3` | Differential testing found no disagreement, over a stated number of cases that **reached the situations the binding declared**. |
| `L4` | A theorem discharges the property. |

L3 needs both halves: agreement *and* a met coverage floor. A run where every
generated case took the same trivial branch agrees with an implementation that
does nothing, so agreement alone is not evidence.

**A level is earned, never annotated.** Something runs, writes a record naming
what it depended on, and a lock command folds the records into a committed
index. Change the model, the implementation or the toolchain, and the record
goes stale rather than standing.

## Honest about what is unknown

The checker reports what is missing, under named kinds, and distinguishes *work
not yet done* from *something broken*. A clause with no model is `Unmodeled`,
which is information. A clause with a model and an implementation and nothing
comparing them is `Unbound`, which is the finding that matters most — it is the
one that looks finished and is not.

Unknown is reported as unknown. Nothing is rounded up.

## What a clause about the tree does

Some clauses are not about a value at all: *nothing here calls a model*, *a link
exists only where somebody wrote one*. There is no function to model and no
second implementation to compare. Those are marked **structural**, with a
reason, and are answered by a test that **reads the repository**. They stay in
the coverage figures and they cap at L2 — a check of this tree is not a law over
all inputs.

Structural is not exempt. An exemption removes a clause from the denominator and
needs a reason and an approver; a project that quietly dropped its own
architectural constraints out of its figures would be flattering itself.
