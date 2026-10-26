---
id: REQ-CONTEXT
title: What an agent needs to see to change a requirement
refines: [REQ-SHOW, ARCH-NO-DRIVING]
status: approved
decomposition: complete
clauses:
  neighbourhood_is_closed: When the context for a requirement is gathered, the system shall include every requirement it refines and every requirement that refines it, transitively, each once, and not the requirement itself.
  claims_with_source: When the context for a requirement or clause is gathered, the system shall include every claim on it — implementations, tests, models and proofs — each with its place and the source of the item it sits on.
  affected_tests: When the context is gathered, the system shall list as affected every test that claims a requirement refining the target, and every test that names an item implementing it, that does not already claim the target.
  person_chooses: The system shall let the person include or leave out each part of the context, and shall copy exactly the included parts that have something in them, in one fixed order.
  part_named: The system shall name each part by the label the view shows for it, and shall refuse a label that names no part.
  copied_not_sent: The system shall put the context on the clipboard for the person to carry to an agent, and shall call nothing.
  from_the_shell: When a requirement or clause is named on the checker's command line, the system shall print the same context for it, with parts chosen by label, and shall refuse a requirement or part that does not exist.
  views_from_the_shell: The system shall print from the shell, as the text the editor produces for it, each station and report a person reads in the editor, except one that writes, without changing what the person has open.
---

# What an agent needs to see to change a requirement

An agent asked to change behaviour reads code but not why the code is there:
the clause it answers to, what that clause refines and what refines it, the
tests and the model. From any requirement or clause the person opens its
context, chooses its parts and copies it as a prompt for their agent. Nothing
is sent (`ARCH-NO-DRIVING`), and an agent can gather the same text itself with
`tracelean-trace --context REQ-X.clause [--parts …]`.

The parts, in their fixed order: `requirement`, `refines`, `refined by`,
`code`, `tests`, `models`, `affected tests`.
