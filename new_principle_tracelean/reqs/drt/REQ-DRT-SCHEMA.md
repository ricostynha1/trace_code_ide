---
id: REQ-DRT-SCHEMA
title: Declared input schemas
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  whitelisted: The schema grammar shall be a closed whitelist of shapes.
  outside_is_error: A type outside the grammar shall be a clear error and never a silently approximated generator.
  declared_not_reflected: The shape shall be declared in the binding, or derived from both signatures as written, rather than extracted from the model by reflection.
  json_shape_matches: A schema shall describe the JSON shape both sides actually exchange, including how sum types are encoded.
  derived_from_both: Where no binding declares a clause, its shape shall be derived by pairing the model's and the implementation's argument types by position, and a pair the grammar does not cover or the two sides disagree on shall be reported and not tested.
---

# Declared input schemas

Lean has no runtime type reflection, and extracting a schema from a model needs a
metaprogram and becomes undecidable once type parameters appear. So the shape is
declared, over a closed grammar.

`outside_is_error` does the work. Approximating an unrecognised type produces a
run that reports cases and coverage while testing a shape neither side has — a
false L3. Refusing is the only honest option.

`derived_from_both` is what `tracelean-trace . --drt` does: both signatures
are read as text, each argument's Lean and Rust types are paired (`Int` with
`i64`, `List` with `Vec`, a `structure` with a serde `struct` field by field),
and anything else — `Float`, a borrowed `&str`, two structures that spell a
field differently — is named and left untested.
