---
id: REQ-ANNOT
title: Annotation grammar
refines: [ARCH-SELFHOST, ARCH-CORE-SHELL, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  comments_only: An annotation shall be recognised only inside a comment of the host language.
  role_vocabulary: The roles shall be models, implements, tests, drt, proves and pins, and nothing else shall parse as a role.
  qualifiers: The qualifiers shall be partial, exempt, nondeterministic and structural, each attaching to the nearest preceding annotation or carrying its own identifier.
  totality: Every candidate annotation shall yield either a parsed annotation or a named problem, and shall never be silently dropped.
  unknown_role_named: A role-shaped token that is not a known role shall be reported as an unknown role.
  region_balanced: A region opened with begin shall be reported when it is never closed, and a close without an open shall be reported.
  literals_protected: Text inside string and character literals shall not be scanned as annotations.
---

# Annotation grammar

Annotations are the only mechanism linking anything to anything here. No
requirements directory, no specification directory, no filename convention:
rename any directory and every link survives, because a link exists only where
somebody wrote one.

That makes `totality` load-bearing. A scanner that can silently drop a link
reports less coverage than the project has — or reports a clause as unlinked when
somebody did the work. Every candidate either parses or is named as a problem.

`literals_protected` is the same concern inverted: an annotation-shaped string in
a test fixture is not a claim about the code.
