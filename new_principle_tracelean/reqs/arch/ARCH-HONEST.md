---
id: ARCH-HONEST
title: Unknown is reported as unknown
status: approved
decomposition: open
clauses:
  weakest_link: The system shall hold a kind of evidence in an aggregate only when every one of its parts holds it, never by average or majority.
  lower_bound_marked: If a percentage is over an incompletely decomposed parent, then the system shall render it marked provisional, saying it can only fall, and never as an exact figure.
  untraced_visible: The system shall show code that nothing claims in every view that reports coverage.
  absence_is_not_pass: If a check did not run, then the system shall report it as not run, and never as passing.
  confidence_carried: The system shall show a derived or inferred relationship as distinct from an asserted one wherever it shows it.
  named_findings: The system shall report a problem under a name that says what it is, not as a generic error.
---

# Unknown is reported as unknown

An assurance tool fails by reporting comfort it has not earned. A file serving
one proved and one merely annotated requirement is an annotated file, so a mark
is held only when every part holds it, and the system says it does not know
whenever that is true.
