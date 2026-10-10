# Orienting in a tree you have not seen

Before changing anything:

1. **Run the checker** (`tracelean-trace .`, or what the README says). Read the
   tail — counts, blocking or not — then the findings
   (`tracelean-view . findings`).
2. **Read the requirement index**: `tracelean-view . requirements` (levels,
   claims) and `tracelean-view . design` (what refines what), then the two or
   three documents your task touches. A document's body says *why* its
   clauses exist.
3. **Read the decisions** (`docs/decisions/` or similar). A change against an
   accepted decision gets reverted.
4. **Find the toolchains and suites**: implementation and model (`formal/`,
   `lake build`). Differential tests are slow and usually behind a flag
   (`cargo test -- --ignored`). Run the fast suite once so you know it was green.
5. **Read the state**: the progress or handoff document. Never hand-edit the
   lock file (`.tracelean/trace.lock`).
6. **Gather the clause's context** — this replaces guessing at files:

   ```bash
   tracelean-trace . --context REQ-THING.the_clause
   ```

   It gives the clause, its neighbours by `refines`, every claiming item with
   source, the tests likely to break, and what the clause still lacks. A
   plain `grep -rn "REQ-THING.the_clause" .` finds the same annotations.

Every view of the editor is reachable from the shell (`tracelean-view`, see
the [README](README.md)); prefer it to asking the person what they see.
