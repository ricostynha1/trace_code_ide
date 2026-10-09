---
id: REQ-REQDOC
title: Requirement documents and the refinement graph
refines: [ARCH-SELFHOST, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  id_is_identity: A requirement shall be identified by its declared identifier, never by its filename or directory.
  id_unique: Two documents declaring the same identifier shall be reported.
  clause_addressable: A requirement shall decompose into clauses, and a clause shall be the unit that links and evidence attach to, so the requirement text a clause's evidence rests on shall be that clause alone — its key, its text and its narrowings — and never its sibling clauses or the prose.
  clauseless_uniform: A requirement declaring no clauses shall be addressable as a single implicit clause.
  refines_dag: Refinement shall form a directed acyclic graph, and a cycle shall be reported.
  refines_resolves: Refining an identifier that does not exist shall be reported.
  decomposition_claimed: Completeness of a decomposition shall default to unclaimed, and a percentage over an unclaimed decomposition shall be rendered as a lower bound.
  malformed_reported: A document whose frontmatter cannot be parsed — a fence never closed, a key the format does not have, a clause block without text, a key an annotation could not name — shall be reported and shall never be silently skipped.
  narrowings_nest: A clause may be written as a block whose `text` is the clause and whose every other key is a narrowing fixing which answer the clause allows; a narrowing shall be part of its clause and never a clause a link can name.
  clause_unique: A clause key declared twice in one document, or a narrowing given twice in one clause, shall be reported.
---

# Requirement documents and the refinement graph

`id_is_identity` makes the system portable: any markdown carrying an identifier
is a requirement, and nothing breaks when a document moves.

`decomposition_claimed` sets the denominator of every percentage the system will
display. Defaulting to unclaimed forces an author to assert that the clauses
exhaust the requirement before a number is presented as exact; until then the
honest rendering is "at least this much". Assuming completeness instead reports
100% whenever somebody was lazy about enumerating.

`malformed_reported`: a requirement that fails to parse is more dangerous than a
missing one, because the project looks like it has fewer requirements than it
has.

`clause_addressable` and `narrowings_nest` together: rewording one clause
re-opens that clause's judgement, tests and proofs and no other's; adding a
narrowing re-opens its clause's. A narrowing adds nothing to a roll-up's
denominator. The prose is covered by document review, not by evidence
([ADR-0016](../../docs/decisions/ADR-0016-narrowings-and-clause-hashes.md)).
