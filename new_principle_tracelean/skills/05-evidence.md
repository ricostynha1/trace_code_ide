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
| model ↔ implementation | a differential run that agreed **and** reached its floor ([04](04-models-and-bindings.md#floors-and-waivers)) |
| model property | a theorem the kernel accepted |

Aggregated by minimum, so a proved clause with no judgement still reads low.
L3 needs agreement, classes and (for Rust) lines; if it is missing, check all
three ran — `.tracelean/pending/<op>.json` shows which halves arrived.

## What you can earn

- **Can**: `tracelean-trace . --drt`, the differential suites and proofs, then
  `--lock`; `--stale` must list nothing you could redo.
- **Cannot**: the requirement ↔ model judgement. Export the prompt
  (`tracelean-trace . --judge REQ-X.clause`), leave it for a person, say so.
  The person records it:
  ```bash
  tracelean-trace . --judge REQ-X.clause --verdict agrees --by <their name>
  ```
  `.tracelean/judges.json` lists who may judge and to whom they delegate. Pass
  `--by <delegate> --delegated-by <person>` **only** when that person told you,
  in this session, to judge on their behalf; quote them in `--note`.

To "raise the evidence" honestly: add a missing model, binding, floor or proof;
run; lock. Never exempt, mark structural, or mark pinned what you did not pin.
