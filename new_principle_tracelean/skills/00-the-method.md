# The method

```
requirement ──@models──► formal model ──@implements──► implementation
 (a clause)              (Lean function)                (code)
                              └──── compared by differential testing ────┘
```

- **Requirement**: a document with an id and named **clauses**, each one
  sentence that can be true or false. Referred to as `REQ-THING.clause`.
- **Model**: a Lean function saying what a clause means, data to data — runnable,
  not a restatement.
- **Implementation**: the real code.
- **Binding**: declares that a model function and a code function answer the
  same question, so generated inputs can be fed to both and compared.

Every arrow is a comment somebody wrote; nothing is inferred.

## Three bonds, aggregated by minimum

| Bond | Question |
|---|---|
| requirement ↔ model | does the function mean what the sentence says? (a person judges) |
| model ↔ implementation | do they agree on every generated input? |
| model property | is there a theorem? |

Never averaged: a strong bond must not hide a missing one.

## Levels

| | |
|---|---|
| `L1` | annotation resolves; nothing checked |
| `L2` | a person judged it (also the cap for `@structural`) |
| `L3` | differential testing agreed **and** met the declared coverage floors |
| `L4` | a theorem discharges it |

A level is **earned** by a run that records what it depended on; change any
input and the record goes stale.

## Unknown stays unknown

`Unmodeled` (no model yet) is information. `Unbound` (model and code exist,
nothing compares them) is the finding that matters most: it looks finished and
is not.

A clause about the tree rather than a value (*nothing here calls the network*)
is `@structural`: answered by a test that reads the repository, capped at L2,
still counted. Structural is not exempt.
