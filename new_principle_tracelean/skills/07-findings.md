# Findings

The checker reports problems under a **named kind**, never a generic error, and
a condition produces exactly one finding — the same condition is not reported
twice under two names.

## The vocabulary

| Kind | Means | Severity |
|---|---|---|
| `Dangling` | An annotation names a requirement or clause that does not exist. | error |
| `DanglingRefines` | A `refines:` entry names a requirement that does not exist. | error |
| `RefinesCycle` | The refinement graph has a cycle. | error |
| `DuplicateId` | Two documents declare the same identifier. | error |
| `Contested` | Two links exclusively claim the same clause. | error |
| `Malformed` | Something wrong in a document or an annotation. | error |
| `UnsoundExemption` | An exemption with no reason or no approver, or past its expiry. | error |
| `Unbound` | Model and implementation both exist and **nothing compares them**. | warning |
| `UnsoundQualifier` | A `partial` or `nondeterministic` qualifier with no reason. | warning |
| `Imprecise` | A file did not parse cleanly, so annotations in or after the unparsed region anchor to the whole file and are capped at the lowest level. | warning |
| `Unmodeled` | A clause has no model and is not exempt. | information |
| `Unimplemented` | Modelled, and nothing claims to implement it. | information |
| `Untested` | Implemented, and no test. | information |

## Progress is not fault

The last three describe **work not yet done**. They are information, not errors,
and a project with some of them is a project being honest about where its
specification stops. Do not try to make them go away by marking things exempt —
that is worse than the finding.

The middle group are warnings: real, worth fixing, not necessarily blocking.

The first group are faults. Something names something that does not exist, or
two things contradict.

## The one to care about most

**`Unbound`.** A clause with a model and an implementation and nothing comparing
them is the finding that looks like success. Every other finding announces that
something is missing. This one announces that two things exist and have never
been put in the same room — which is the exact state a project drifts into when
nobody is watching.

If you add a model and an implementation and stop, you have created an
`Unbound`. Finish it.

## What blocks

Which kinds block is **policy**, and the default blocking set is small — usually
malformed documents, dangling annotations, refinement cycles, duplicate
identifiers, contested clauses and unsound exemptions. Look for the project's
policy file (commonly `.tracelean/trace_policy.json`) to see what it has chosen.

`Imprecise` deliberately does not block: an unsupported language construct
should not fail a build that is otherwise fine.

## Reading a run

Read the tail first — the counts and whether anything is blocking — then the
findings themselves. Compare against what the checker said *before* your change.
The delta is the useful signal; the absolute numbers are context.

New findings you should expect, and what they mean about your work:

| You see | It means |
|---|---|
| `Dangling` | You named a clause that does not exist. Check the spelling against the document. |
| `Unmodeled` on a clause you added | Expected. It is the work you just signed up for. |
| `Unbound` on a clause you added | You wrote both sides and no binding. |
| `Imprecise` on a file you just wrote | The grammar could not parse a declaration. Ask the checker where — there is normally a flag — and write it more plainly. |
| `Contested` | Two links exclusively claim one clause. Decide which is right. |
| A count that dropped | Usually correct: records went stale because their inputs changed. Check the drop is no larger than your change justifies. |

## Do not silence a finding

Every mechanism for making a finding disappear is itself checked:

- `@exempt` without a reason and an approver is `UnsoundExemption`, which
  blocks.
- `@partial` and `@nondeterministic` without a reason are `UnsoundQualifier`.
- `@structural` on a clause that does have a function caps it at L2 — the
  project's own figures get worse while the finding count improves.

A finding you cannot fix right now is something to **report**, not something to
suppress. Say what it is and why it is open. That is what the method is for.
