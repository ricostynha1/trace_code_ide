---
id: REQ-LSP
title: Language server integration
refines: [ARCH-CORE-SHELL, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  registry_per_language: The system shall hold a server per language rather than a single server.
  encoding_round_trip: Converting a position between the editor's encoding and the protocol's shall round-trip exactly.
  encoding_declared: The position encoding shall be taken from what the server declared and shall not be assumed.
  edits_become_commands: A workspace edit received from a server shall be lowered into commands.
  edits_ordered: Lowered edits shall be applied so that earlier edits do not invalidate the positions of later ones.
  overlap_refused: Overlapping edits in one workspace edit shall be refused rather than applied in an arbitrary order.
  absent_server_named: The absence of a running server shall be reported as such and shall not be presented as an empty result.
---

# Language server integration

A model in one language beside an implementation in another is the premise of the
project, so a registry keyed by language is the shape from the start.

`encoding_round_trip` is a small clause covering a large class of bugs. The
protocol counts UTF-16 code units, editors count bytes or characters, and every
mismatch appears as an off-by-some on lines containing non-ASCII text — reported
as "hover is wrong sometimes" and nearly unfindable from that. It is also a pure
function with an obvious law.

`absent_server_named` has a sharp instance here: for a proof assistant an empty
goal list means the proof is complete, so rendering "no server running" that way
tells the user they have finished when they have not started.
