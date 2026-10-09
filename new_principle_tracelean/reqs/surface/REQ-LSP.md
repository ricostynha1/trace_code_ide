---
id: REQ-LSP
title: Language server integration
refines: [ARCH-CORE-SHELL, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  registry_per_language: The system shall hold a server per language rather than a single server.
  column_to_offset: A column in the server's encoding shall convert to the byte offset at which a prefix of the line spanning that many units ends, and to none when no prefix ends there.
  offset_to_column:
    text: A byte offset shall convert to the number of the server's units in the prefix of the line ending at that byte, and to none when no prefix ends there.
    round_trip: Converting a byte offset to a column and the column back shall return the offset, and the same the other way round.
  encoding_declared: The position encoding shall be taken from what the server declared and shall not be assumed.
  edits_become_commands: A workspace edit received from a server shall be lowered into commands.
  edits_ordered: Lowered edits shall be applied so that earlier edits do not invalidate the positions of later ones.
  overlap_refused: Overlapping edits in one workspace edit shall be refused rather than applied in an arbitrary order.
  absent_server_named: The absence of a running server shall be reported as such and shall not be presented as an empty result.
---

# Language server integration

A model in one language beside an implementation in another is the premise of the
project, so a registry keyed by language is the shape from the start.

`column_to_offset` and `offset_to_column` are small clauses covering a large
class of bugs; both read one relation between a prefix's bytes and its units,
so together they round-trip exactly. The protocol counts UTF-16 code units,
editors count bytes or characters, and every mismatch appears as an
off-by-some on lines containing non-ASCII text — reported as "hover is wrong
sometimes" and nearly unfindable from that. Each is a pure function, and the
round trip is a law proved over the pair (`SpecLsp.offset_round_trip`,
`column_round_trip`).

`absent_server_named` has a sharp instance here: for a proof assistant an empty
goal list means the proof is complete, so rendering "no server running" that way
tells the user they have finished when they have not started.
