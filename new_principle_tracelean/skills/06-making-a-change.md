# Making a change

```
name the clause → gather its context → fix → annotate → model →
test that would have caught it → see it fail → check → test → lock → document → report
```

0. **Name the clause** and gather what it touches:
   ```bash
   tracelean-trace . --context REQ-THING.clause            # default parts
   tracelean-trace . --context REQ-THING --parts all       # whole requirement, both directions
   ```
   Read the *affected tests* and *not yet met* sections before editing. No
   clause for the task? Write one first ([03](03-requirements.md)). Asked for
   something the requirements forbid? Say so before building it.
1. **Fix it**, matching the surrounding style. Put the decision in a callable
   function, not inside an effect.
2. **Annotate**: `@implements` on what realises the clause, `@tests` on each
   test ([02](02-annotations.md)).
3. **Model it** if it is a value and has none ([04](04-models-and-bindings.md));
   build, bind, declare floors. A clause about the tree gets `@structural`.
4. **Write the test that would have caught the bug** — usually at a join
   nothing exercised. Never hand it the value the code should produce; observe
   the real output. **Reintroduce the bug and watch it fail.**
5. **Check**: `tracelean-trace .`, compared with before. New `Dangling` = a
   misspelt clause; new `Unbound` = a missing binding.
6. **Run the suites**: fast always; slow (differential) when you touched a
   model, a bound implementation or a schema.
7. **Earn and lock** ([05](05-evidence.md)): `tracelean-trace . --drt`, then
   `tracelean-trace . --lock`.
8. **Update documents** that describe what changed ([03](03-requirements.md#documents-that-describe-requirements)),
   and the progress document with what is left.
9. **Before reporting**: `tracelean-trace .` ends `blocking: false`, and
   `tracelean-trace . --stale` lists only what is a person's (judgements).
10. **Report honestly**: failures shown, skipped steps named, what is left for a
    person listed.
