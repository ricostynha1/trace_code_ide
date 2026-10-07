---
id: REQ-CONTEXT
title: What an agent needs to see to change a requirement
refines: [REQ-SHOW, ARCH-NO-DRIVING]
status: approved
decomposition: complete
clauses:
  neighbourhood_is_closed: The context for a requirement shall hold every requirement it refines and every requirement that refines it, transitively, each once, and not the requirement itself.
  claims_with_source: The context for a requirement or clause shall hold every claim on it — implementations, tests, models and proofs — each with its place and the source of the item it sits on.
  affected_tests: The context shall list as affected every test that claims a requirement refining the target, and every test that names an item implementing it, that does not already claim the target.
  person_chooses: Each part of the context shall be included or left out by the person, and the text copied shall hold exactly the included parts that have something in them, in one fixed order.
  part_named: A part shall be named by the label the view shows for it, and a label that names no part shall be refused.
  copied_not_sent: The context shall be put on the clipboard for the person to carry to an agent, and nothing shall be called.
  from_the_shell: The project's checker shall print the same context for a requirement or clause named on its command line, with parts chosen by label, and shall refuse a requirement or part that does not exist, so an agent can gather the context itself.
---

# What an agent needs to see to change a requirement

An agent asked to change behaviour reads code. What it does not see is why
the code is there: the clause it answers to, what that clause refines, what
refines it in turn, the tests that pin it and the model that says what it
means. It changes the function and breaks a test in a file it never opened.

The editor knows all of that — it is the trace. So from any requirement or
clause the person opens its **context**: the neighbourhood an agent needs,
parted into what it says, what it refines, what refines it, the code, the
tests, the models, and the tests that may break. The person chooses which
parts go (a large tree's refinements may not all be wanted) and copies the
result, as a prompt, to give their agent.

Nothing is sent anywhere: the person carries it, as with the judge's prompt
(`ARCH-NO-DRIVING`). An agent may also gather it itself:
`tracelean-trace --context REQ-X.clause [--parts …]` prints the same text.

The parts, in their fixed order: `requirement`, `refines`, `refined by`,
`code`, `tests`, `models`, `affected tests`.
