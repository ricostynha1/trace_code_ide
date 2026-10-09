# Judging advice — REQ-CONTEXT

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `neighbourhood_is_closed` | agrees, with a note |
| `part_named` | agrees |
| `person_chooses` | agrees, with a note |

## `neighbourhood_is_closed`

*Note.* Both directions are transitive closures. `seen` and `eraseDups` keep
each name once, and the start is filtered out even when it sits on a cycle. The
fuel (nodes + edges + 1) is enough. A `refines:` name that no document declares
still appears among the ancestors. That is harmless, but strictly it is not a
"requirement".

## `person_chooses`

*Note.* `partsShown` models which parts the copied text holds, and in what
order: the fixed `allParts` order, whatever order they were chosen in. It does
not model the text itself.
