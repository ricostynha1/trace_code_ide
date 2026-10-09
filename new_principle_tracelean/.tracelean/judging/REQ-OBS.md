# Judging advice — REQ-OBS

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `workspace_is_a_copy` | drift |

### `workspace_is_a_copy` — drift

`copyCheck` checks one direction: each mirrored file of the real tree is in the
workspace with the same bytes. A workspace holding an extra file the project
does not have reports no violation, nor does one missing `.git`/`.tracelean`
(which the SBX doc says tools legitimately read). That is a subset check, not a
copy. The "real tree unmodified" half is checked as a before/after snapshot over
every path, so a write reverted before the tool exits is not seen. The proof
`an_empty_tree_copies_cleanly` covers only the empty witness.
