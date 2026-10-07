# Findings

Each condition is reported once, under a named kind.

| Kind | Means | Severity |
|---|---|---|
| `Dangling` | an annotation names a missing requirement or clause | error |
| `DanglingRefines` | `refines:` names a missing requirement | error |
| `RefinesCycle` | the refinement graph has a cycle | error |
| `DuplicateId` | two documents share an id | error |
| `Contested` | two links exclusively claim one clause | error |
| `Malformed` | a broken document or annotation | error |
| `UnsoundExemption` | exemption without reason/approver, or expired | error |
| `Unbound` | model and code exist, **nothing compares them** | warning |
| `UnsoundQualifier` | `@partial`/`@nondeterministic` without reason | warning |
| `Imprecise` | file did not parse; claims anchor to the whole file | warning |
| `Unmodeled` | no model yet | information |
| `Unimplemented` | modelled, nothing implements | information |
| `Untested` | implemented, no test | information |

Information is **work not yet done**, not a fault. `Unbound` matters most: it
looks finished. Which kinds block is policy (`.tracelean/trace_policy.json`);
`Imprecise` never blocks.

## Reading a run

Read the tail first, then compare with the run before your change — the delta
is the signal. A clause you added shows `Unmodeled`: expected. A dropped count
is usually stale records: expected.

## Never silence one

Exempting without approval blocks (`UnsoundExemption`); qualifiers without
reasons warn; `@structural` on a modelled clause lowers the project's figures.
A finding you cannot fix now is **reported**, not suppressed.
