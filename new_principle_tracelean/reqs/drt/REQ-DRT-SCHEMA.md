---
id: REQ-DRT-SCHEMA
title: Declared input schemas
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  whitelisted: The system shall describe inputs in a schema grammar that is a closed whitelist of shapes.
  outside_is_error: If a type is outside the grammar, then the system shall report a clear error and never approximate a generator silently.
  declared_not_reflected: The system shall take an input shape as declared for the binding, or derive it from both signatures as written, and never extract it from the model by reflection.
  json_shape_matches: The system shall describe in a schema the JSON shape both sides actually exchange, including how sum types are encoded.
  derived_from_both: Where no binding declares a clause's shape, the system shall derive it by pairing the model's and the implementation's argument types by position, and shall report and not test a pair the grammar does not cover or the two sides disagree on.
---

# Declared input schemas

Lean has no runtime type reflection, and extracting a schema from a model would
need a metaprogram that becomes undecidable once type parameters appear, so the
shape is declared over a closed grammar. Approximating an unknown type would
test a shape neither side has and still report cases and coverage: a false L3.

`tracelean-trace . --drt` reads both signatures as text and pairs each
argument's types (`Int` with `i64`, `List` with `Vec`, a `structure` with a
serde `struct` field by field); anything else is named and left untested.
