---
id: REQ-DRT-BIND
title: Bindings describe a call and nothing more
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  no_adapter: The system shall run no project-written code between the implementation and the comparator.
  call_only: The system shall let a binding describe which function to call and how its arguments are spelled, and shall not let it transform behaviour.
  rename_only: The system shall let a binding's parameter map rename fields, and shall not let it compute them.
  binding_is_the_bond: The system shall bind a model to an implementation by the existence of a binding, not by a comment.
  derived_unless_written: Where a clause has a Lean model and a Rust implementation whose signatures pair, the system shall derive the clause's binding from them, and shall let a written binding replace only the fields it names.
---

# Bindings describe a call and nothing more

An adapter between the implementation and the comparator is an untrusted
participant in the comparison: it can make a divergence disappear, easily and
invisibly. A runner calls the function directly, so there is no adapter file to
carry a `@drt` annotation.

A binding written by hand repeats what the two annotations already say and goes
wrong when the code moves, so the written file keeps only what cannot be read
off the code: coverage floors, waivers, renamed parameters and further clauses
or implementations.
