---
id: REQ-REQDOC
title: Requirement documents and the refinement graph
refines: [ARCH-SELFHOST, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  id_is_identity: The system shall identify a requirement by its declared identifier, never by its filename or directory.
  id_unique: If two documents declare the same identifier, then the system shall report it.
  clause_addressable: The system shall decompose a requirement into clauses and attach links and evidence to a clause, so that the requirement text a clause's evidence rests on is that clause alone, its key, its text and its narrowings, and never its sibling clauses or the prose.
  clauseless_uniform: Where a requirement declares no clauses, the system shall address it as a single implicit clause.
  refines_dag: The system shall require refinement to form a directed acyclic graph, and if there is a cycle, then the system shall report it.
  refines_resolves: If a requirement refines an identifier that does not exist, then the system shall report it.
  decomposition_claimed: The system shall treat a decomposition as unclaimed unless its completeness is claimed, and shall render a percentage over an unclaimed decomposition as a bound.
  malformed_reported: If a document's frontmatter cannot be parsed, whether by a fence never closed, a key the format does not have, a clause block without text or a key an annotation could not name, then the system shall report it and never silently skip it.
  narrowings_nest: Where a clause is written as a block, the system shall take its `text` as the clause and every other key as a narrowing fixing which answer the clause allows, part of its clause and never a clause a link can name.
  clause_unique: If a clause key is declared twice in one document, or a narrowing is given twice in one clause, then the system shall report it.
---

# Requirement documents and the refinement graph

Any markdown carrying an identifier is a requirement, so nothing breaks when a
document moves. The decomposition sets the denominator of every percentage, and
assuming completeness would report 100% whenever somebody was lazy about
enumerating. A requirement that fails to parse is more dangerous than a missing
one, because the project looks as if it has fewer requirements than it has.

Rewording one clause re-opens that clause's judgement, tests and proofs and no
other's, and adding a narrowing re-opens its clause's. A narrowing adds nothing
to a roll-up's denominator. The prose is covered by document review, not by
evidence
([ADR-0016](../../docs/decisions/ADR-0016-narrowings-and-clause-hashes.md)),
and clauses are written in EARS
([ADR-0017](../../docs/decisions/ADR-0017-clauses-in-ears.md)).
