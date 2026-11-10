---
id: REQ-FIGURE
title: The design is drawn, not only listed
refines: [REQ-LOOK, REQ-SHOW]
status: approved
decomposition: open
clauses:
  card_on_hover: When the pointer rests on a requirement or a clause — a chip, its name in a buffer, a node, a row — the system shall show a card with its title, the words of each clause with the marks it holds, and every place that claims it, read from the index and the lock and from nothing else.
  graph_filtered: When the reader filters the drawn design, the system shall draw exactly the requirements that are under the chosen root, match every word typed, and are of the kind asked for — every one, those with a mark missing, those with a clause unimplemented, or those an unsaved edit touches.
  treemap_areas: When the design is drawn as a treemap, the system shall give each requirement a rectangle whose area is its share of the clauses drawn, the rectangles tiling the figure with none overlapping and none left out.
  impact_before_saving: While a file has unsaved edits, the system shall list every clause that file claims, once per file, with the marks the clause holds now, and shall list nothing when nothing is unsaved.
  runs_listed: The system shall list each compared operation with a differential run on record — how many clauses it carries, its seed, its number of cases and when it was recorded — read from the evidence records, and shall offer to run them again through the same actions the keys reach.
  overview_as_text: The system shall write the overview as Markdown — the counts of requirements, clauses and findings, how many clauses hold each of the five marks, the requirements with the most marks left first, and, only when there is something to say, the runs on record, the unsaved edits, the agent's waiting changes and the modules — from the same figures the window draws.
  agent_steps_listed: While a sandboxed agent has changes waiting, the system shall list one step per file it changed, in the order its first change arrived, with whether the file is created, deleted or modified, the lines added and removed, and each clause the file claims with the marks it holds now, and shall list nothing when no changes wait.
  exported_as_markdown: When the overview is exported, the system shall give the same Markdown the overview station shows, as a string to copy from the window and as a file from the command line.
---

# The design is drawn, not only listed

The design buffer says everything about the requirements, as text. A person
looking for *where the work is* reads shapes faster than rows, so the window
also draws the same nodes (`Editor::overview`, the ones `design_buffer` is
produced from): the share of clauses holding each mark, the refinement graph,
a treemap and a matrix (`web/src/overview.ts`).

Nothing drawn is computed twice. The figures take the nodes the editor hands
over; the card (`Editor::card`), the edit impact (`Editor::impact`,
`impact_of`) and the runs (`Editor::runs`) are queries over the index, the
lock and the evidence records. The filters, the treemap's layout and the
graph's layering are pure functions of those nodes and are tested as such
(`web/test/overview.mjs`).

The terminal has no figures yet; it keeps the design buffer. That gap is the
reason `decomposition` is open.
