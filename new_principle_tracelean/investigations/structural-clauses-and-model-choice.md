# Two problems explained: clauses no model can check, and which model a clause gets

## 1. Clauses about the shape of the code, not about a value

A Lean model is a function: inputs in, output out. A model can check a clause
when the clause says something about **outputs** — "the aggregate is the
minimum of the levels": give it levels, look at the answer.

Some clauses say something about **who calls what**:

> REQ-ACT.one_path — A key and a button shall reach the same resolver.

Write any function `dispatch`. Does it satisfy "a key and a button reach it"?
The question is not about `dispatch` at all — it is about the rest of the code:
is there some *other* function a button calls instead? No input you give
`dispatch` can show that, so any `dispatch` "agrees", including a wrong one.
That is what *unmodelable* meant in the review. The same holds for:

| Clause | Really asks |
|---|---|
| ACT.one_path | do keys and buttons both go through `dispatch`? |
| CMD.single_path | does every workspace change go through `apply`? (`Workspace.set` is public, so something could bypass it) |
| SCREEN.one_arrangement_path | does every layout change go through `Arrangement`? |
| SHOW.core_produces | is a `Buffer` built only in the core crate? |
| SHOW.producer_is_pure | does no producer touch the disk? (every Lean function is pure, so a Lean model says nothing) |
| PERSIST.append_only | is the log only ever opened for appending? |

**The fix already exists**: ADR-0012's `@structural(reason=...)`. A structural
clause is checked by a test that *reads the repository*, like
`crates/core/tests/architecture.rs` already does for "only `drt/` may start a
process". For example, `CMD.single_path` becomes: a test that fails if any file
outside `cmd/` calls `Workspace::set`. These six clauses should drop their Lean
models, take `@structural`, and get such a test. They cap at L2, stay in the
denominator.

## 2. A clause with several models: which one is "the" model?

A clause can have several `@models` declarations — after pinning, at least
two: the specification (a `Prop`) and the function. Tools then need one of
each, and today they choose by **position**:

- `--judge` shows the reviewer the *first* model in file order, only that one;
- `--pins` takes the first `Prop` as the spec, the first non-`Prop` as the
  function, and the first `@pins` theorem;
- files are visited in sorted name order, declarations top to bottom.

So the choice depends on where files happen to sit, and nothing warns when it
is wrong. Cases the review hit:

| Clause | Intended model | Chosen instead | Effect |
|---|---|---|---|
| SBX.real_tree_untouched | `copyViolations` (checks the real tree) | `copyCheck` (checks the copy) — `Effects.lean` sorts first | judged and tested against the wrong function |
| PROV.base_is_honest | `origin` | `BackStep` — the annotation on a constructor binds to the next declaration | the judge read a type with no base case |
| EVID.weakest_link | the new pin in `Evidence.lean` | — | the old `@pins` in `Pinned.lean` is silently ignored |
| SBX.escape_is_not_silent | `Policy` | `Effects.escapeViolations` (`Effects.lean` sorts before `Policy.lean`) | a spec would have to be written against whichever sorts first |

**Fix** ([action plan](../work/action-plan.md) §7): say the role instead of relying on
position — a spec is annotated `@specifies REQ-X.c` (not `@models`), and the
checker reports a clause with two `@models` functions, two specs or two `@pins`
as a finding ("which one?") unless one is marked primary. `--judge` shows all of
them.
