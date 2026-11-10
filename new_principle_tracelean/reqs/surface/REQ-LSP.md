---
id: REQ-LSP
title: Language server integration
refines: [ARCH-CORE-SHELL, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  registry_per_language: The system shall hold a server per language rather than a single server.
  column_to_offset: The system shall convert a column in the server's encoding to the byte offset at which a prefix of the line spanning that many units ends, and to none when no prefix ends there.
  offset_to_column:
    text: The system shall convert a byte offset to the number of the server's units in the prefix of the line ending at that byte, and to none when no prefix ends there.
    round_trip: When a byte offset is converted to a column and the column back, the system shall return the offset, and the same the other way round.
  encoding_declared: The system shall take the position encoding from what the server declared, and shall not assume it.
  edits_become_commands: When a server sends a workspace edit, the system shall lower it into commands.
  edits_ordered: The system shall apply lowered edits so that earlier edits do not invalidate the positions of later ones.
  overlap_refused: If edits in one workspace edit overlap, then the system shall refuse them rather than apply them in an arbitrary order.
  absent_server_named: If no server is running, then the system shall report that, and shall not present it as an empty result.
---

# Language server integration

A model in one language beside an implementation in another is the premise of
the project, so servers are kept per language. The protocol counts UTF-16 code
units and editors count bytes or characters, and every mismatch shows as an
off-by-some on lines holding non-ASCII text; the two conversions read one
relation, so their round trip is a law proved over the pair
(`SpecLsp.offset_round_trip`, `column_round_trip`). For a proof assistant an
empty answer means the proof is complete, so showing a missing server that way
would tell a person they have finished when they have not started.
