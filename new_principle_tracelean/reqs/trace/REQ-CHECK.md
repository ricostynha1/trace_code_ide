---
id: REQ-CHECK
title: Findings
refines: [ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  named_kinds: Every problem shall be reported under a named kind rather than a generic error.
  progress_not_fault: A finding describing work not yet done shall be distinguished from one describing something broken.
  unbound_reported: A clause with both a model and an implementation but no binding between them shall be reported.
  exactly_once: A condition shall produce exactly one finding, and the same condition shall not be reported under two kinds.
  severity_policy: Which kinds block shall be policy, and the default blocking set shall be small.
  derived_from_evidence: A finding about drift or divergence shall be derived from an evidence record and shall not be asserted independently.
  qualifier_soundness: An exemption without a reason and an approver, or past its expiry, shall itself be reported.
  structural_is_not_exempt: A clause marked structural shall require a test but not a model, shall remain in the coverage denominator, and shall not reach the level differential testing establishes.
---

# Findings

Naming is the feature. "Traceability error" is not actionable; *this clause has a
model and an implementation and nothing binds them* is a task.

`progress_not_fault` keeps it usable — most findings on a real codebase are where
the work is, and a tool reporting them as failures gets disabled in week one.

`unbound_reported` is the most important finding in the system: a model and an
implementation with nothing comparing them is the exact shape of a project that
believes it is verified and is not. It is invisible without this check, because
every individual artefact looks healthy.
