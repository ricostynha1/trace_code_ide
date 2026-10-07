# Evidence

**A level is earned, never annotated.** Writing `L3` anywhere claims what you
did not establish.

1. A backend **runs** something (differential test, proof check, a person's
   judgement).
2. It writes a **record** (`.tracelean/evidence/`) naming its inputs: model,
   implementation, toolchain.
3. A **lock** folds the records into the committed index, dropping stale ones
   (`tracelean-trace . --lock`).

Change any input and the record goes stale. A drop in established clauses after
a real change is usually correct — check only that it is no larger than your
change justifies.

| Bond | Earned by |
|---|---|
| requirement ↔ model | a **person's** judgement |
| model ↔ implementation | a differential run that agreed **and** met its floors |
| model property | a theorem the kernel accepted |

Aggregated by minimum, so a proved clause with no judgement still reads low.
L3 needs both the agreement run and the floor count; if it is missing, check
both ran.

## What you can earn

- **Can**: run differential suites and proofs, then lock.
- **Cannot**: the requirement ↔ model judgement. Export the prompt
  (`tracelean-trace . --judge REQ-X.clause`), leave it for a person, say so.

To "raise the evidence" honestly: add a missing model, binding, floor or proof;
run; lock. Never exempt, mark structural, or mark pinned what you did not pin.
