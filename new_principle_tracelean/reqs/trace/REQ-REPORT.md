---
id: REQ-REPORT
title: A report a reviewer can read
refines: [ARCH-HONEST, REQ-EVID, REQ-DIFF]
status: approved
decomposition: open
clauses:
  static_page: The system shall write the report as one HTML page that opens from the filesystem without a server or a network.
  card_per_clause: The system shall show each clause of each requirement as a card carrying the clause's text and its four marks.
  why_not_held: For each mark a clause does not hold, the system shall state on its card the first missing step that keeps it from the mark.
  evidence_on_card: The system shall show on a clause's card its model, the code implementing it, its tests, its theorems with Lean's verdict on each, the axioms its proof assumes, its strength, and the judgement with who made it.
  findings_on_card: The system shall show on a clause's card every finding about that clause.
  changes_marked: When the report is made against a base revision, the system shall mark each card whose clause changed and list its changes on the card.
---

# A report a reviewer can read

The editor shows the evidence one clause at a time to someone already inside
the project. A reviewer of a change made by an agent is not; they need one page
to open from a CI artefact or an email, with every promise the project makes on
a card of its own: what the clause says, which of `A J T P` it holds, and for
each it does not hold, the first step missing (no model, nothing comparing model
and code, every theorem refused and why). A mark is a claim; the card puts the
claim next to what earned it, so the reviewer can check the evidence rather than
trust the letter.

Made against a base revision (`--diff main`), the report marks each card the
change touched and lists what happened to it, so review starts at the cards
that moved.

```
tracelean-trace . --report target/report [--diff main]
```
