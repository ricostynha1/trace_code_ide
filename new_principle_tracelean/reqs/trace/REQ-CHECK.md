---
id: REQ-CHECK
title: Findings
refines: [ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  named_kinds: The system shall report every problem under a named kind rather than as a generic error.
  progress_not_fault: The system shall distinguish a finding describing work not yet done from one describing something broken.
  unbound_reported: If a clause has both a model and an implementation but no binding between them, then the system shall report it.
  exactly_once: The system shall produce exactly one finding for a condition, and shall not report the same condition under two kinds.
  severity_policy: The system shall make which kinds block a matter of policy, with a small default blocking set.
  derived_from_evidence: The system shall derive a finding about drift or divergence from an evidence record, and shall not assert it independently.
  qualifier_soundness:
    text: If an exemption has no reason or no approver, or is past its expiry, then the system shall report it.
    today: The system shall judge expiry against a date the check is given, never a clock it reads, and where the check is given none, shall judge no exemption expired.
  structural_is_not_exempt: Where a clause is marked structural, the system shall require a test but not a model, keep the clause in the coverage denominator, and never give it the tested mark.
  structural_rejects: The system shall show a structural check rejecting a tree that violates it, as well as accepting the project's own tree.
  one_of_each_role: If a clause is claimed by more than one models, more than one specifies, or more than one pins declaration, then the system shall report it under a kind naming that role, naming every declaration involved.
---

# Findings

"Traceability error" is not actionable; *this clause has a model and an
implementation and nothing binds them* is a task. Most findings on a real
codebase are where the work is, and a tool reporting them as failures gets
disabled in the first week.

An unbound clause is the most important finding: a model and an implementation
with nothing comparing them is a project that believes it is verified and is
not, and every artefact on its own looks healthy. A second model leaves which
one is meant to be guessed (ADR-0014); those kinds warn rather than block.
