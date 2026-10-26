---
id: REQ-DRT-BIND
title: Bindings describe a call and nothing more
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  no_adapter: No project-written code shall run between the implementation and the comparator.
  call_only: A binding shall describe which function to call and how its arguments are spelled, and shall not transform behaviour.
  rename_only: A binding's parameter map shall rename fields and shall not compute them.
  binding_is_the_bond: The existence of a binding, not a comment, shall be what binds a model to an implementation.
  derived_unless_written: Where a clause has a Lean model and a Rust implementation whose signatures pair, the system shall derive the clause's binding from them, and a written binding shall only replace the fields it names.
---

# Bindings describe a call and nothing more

The argument against a hand-written adapter is not duplication. It is that an
adapter is an untrusted participant in the comparison it exists to enable:
arbitrary code between the implementation and the comparator can make a
divergence disappear, and this is the one place where hiding one would be both
easy and invisible.

`binding_is_the_bond`: with a runner calling the function directly there is no
adapter file to carry a `@drt` annotation, and inventing one so a comment exists
somewhere would be a filename convention of exactly the kind this project
refuses.

`derived_unless_written`: a binding written by hand is a second copy of what the
two annotations already say, and a copy goes wrong when the code moves. The
written file keeps only what cannot be read off the code: coverage floors,
waivers, renamed parameters and further clauses or implementations.
