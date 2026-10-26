---
id: REQ-ANNOT
title: Annotation grammar
refines: [ARCH-SELFHOST, ARCH-CORE-SHELL, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  comments_only: The system shall recognise an annotation only inside a comment of the host language.
  role_vocabulary: The system shall recognise as roles models, specifies, implements, tests, drt, proves and pins, and shall parse nothing else as a role.
  qualifiers: The system shall recognise the qualifiers partial, exempt, nondeterministic and structural, each attaching to the nearest preceding annotation or carrying its own identifier.
  totality: The system shall yield for every candidate annotation either a parsed annotation or a named problem, and shall never silently drop one.
  unknown_role_named: If a role-shaped token is not a known role, then the system shall report it as an unknown role.
  region_balanced: If a region opened with begin is never closed, or a close has no open, then the system shall report it.
  literals_protected: The system shall not scan text inside string and character literals for annotations.
---

# Annotation grammar

Annotations are the only mechanism linking anything to anything here: no
requirements directory, no filename convention. Rename any directory and every
link survives, because a link exists only where somebody wrote one. So a
scanner that silently dropped a link would report less than the project has,
or a clause as unlinked when somebody did the work.

`models` names the one function computing what a clause talks about;
`specifies` names the `Prop` saying which answers are right (ADR-0014). An
annotation-shaped string in a test fixture is not a claim about the code.
