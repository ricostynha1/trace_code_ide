---
id: REQ-DRT-SCHEMA
title: Declared input schemas
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  whitelisted: The schema grammar shall be a closed whitelist of shapes.
  outside_is_error: A type outside the grammar shall be a clear error and never a silently approximated generator.
  declared_not_reflected: The shape shall be declared in the binding rather than extracted from the model.
  json_shape_matches: A schema shall describe the JSON shape both sides actually exchange, including how sum types are encoded.
---

# Declared input schemas

Lean has no runtime type reflection, and extracting a schema from a model needs a
metaprogram and becomes undecidable once type parameters appear. So the shape is
declared, over a closed grammar.

`outside_is_error` does the work. Approximating an unrecognised type produces a
run that reports cases and coverage while testing a shape neither side has — a
false L3. Refusing is the only honest option.
