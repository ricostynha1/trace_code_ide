---
id: ARCH-HONEST
title: Unknown is reported as unknown
status: approved
decomposition: open
clauses:
  weakest_link: An aggregate assurance shall be the minimum over its parts, never an average.
  lower_bound_marked: A percentage over an incompletely decomposed parent shall be rendered as a lower bound and never as an exact figure.
  untraced_visible: Code that nothing claims shall appear in every view that reports coverage.
  absence_is_not_pass: A check that did not run shall be reported as not run, and never as passing.
  confidence_carried: A derived or inferred relationship shall be distinguishable from an asserted one wherever it is shown.
  named_findings: A problem shall be reported under a name that says what it is, not as a generic error.
---

# Unknown is reported as unknown

Every clause is a refusal, because the failure mode of an assurance tool is not
missing problems — it is reporting comfort it has not earned, after which the
team believes something false and stops looking.

Averaging is the most effective way to produce that. A file serving one proved
and one merely-annotated requirement is an annotated file; "75% assured" is a
number with no referent that somebody will act on anyway. The minimum is the only
aggregate that preserves the meaning of its inputs.

The cost is a system that often says "unknown". That is the true state of most of
most projects, and only a tool that can say it can show progress out of it.
