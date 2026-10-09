# Judging advice — REQ-ACT

Produced by claude-review, simulating the human approver: an assistant asked to
read each clause against its models as a person would. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**. The
verdicts below were entered with `--judge … --verdict … --by "claude-review
(simulated human)"`.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-ACT.<clause>`
and `--context REQ-ACT.<clause> --parts requirement,models`. Reviewed 2026-10-09.

| Clause | Verdict |
|---|---|
| `action_to_intent` | agrees, with a note |
| `focus_is_carried` | drift |
| `one_path` | unmodelable |
| `unknown_is_refused` | agrees, with a note |
| `missing_target_is_refused` | drift |
| `edits_are_commands` | drift |
| `dispatch_is_pure` | drift |

`everything_is_offered` was not in this review.

## `focus_is_carried`: drift

`file.definition` and `file.references` resolve to
`display (record "definition")` and `display (record "references")` whatever
the focus is. The intent carries no target. The editor reads the cursor again
afterwards (`go_to_definition`, `identifier_at(&self.buffer().text, self.offset)`
in `crates/editor/src/lib.rs`). These two actions name a target, and resolution
does not give them one.

`opening_follows_the_cursor` checks only `file.open`, which does carry its
target.

## `missing_target_is_refused`: drift

This is the same pair of actions seen from the other side. `dispatch
"file.references" {under := none}` is `display (record "references")`, not
`refuse (needsTarget …)`. The missing name is reported by the shell ("no name
here"), outside the model and outside the `Blocked` type.
`a_missing_target_is_named` covers `file.open` only.

## `edits_are_commands`: drift

`observe.accept` resolves to `observe accept`, and `observe.accept_file` to
`observe (acceptFile path)`. Neither carries a command, but accepting puts the
sandbox's changes into the workspace. The commands do exist: `Editor::accept`
pushes the commands held in `pending`. They come from editor state after
resolution, though, so the intent does not carry them. The implementation
keeps the spirit of the clause and the model does not state it. `history.undo`
and `history.redo` (`travel`) also change the workspace with no command in the
intent. That case is defensible, because they replay inverses, but the clause
as worded does not exempt them.

## `dispatch_is_pure`: drift

The clause says resolution is "a function of the action and the focus and shall
read nothing else". `dispatch` also takes `w : Workspace` and reads it in
`deleteFocus` and `approveUnder`. Take the same action `file.delete` and the
same focus on file `a.rs`. With `{}` the answer is
`refuse (needsTarget "file.delete" "a file that exists")`. With a workspace
holding `a.rs` the answer is `edit (deleteFile "a.rs" content)`. The workspace
is needed for the witness (`REQ-CMD.witness_carried`), so the fix is most
likely in the words: say "the action, the focus and the workspace".

## `one_path`: unmodelable

Whether a key and an affordance reach the same resolver depends on the call
sites. Given the same arguments, any deterministic function gives the same
answer, so no input to `dispatch` can falsify this. It needs an architectural
check, of the kind `tests/architecture.rs` already does elsewhere. There is
evidence that the path is in fact not single. `Editor::act_in_bar` handles
`screen.show` from a strip row without calling `dispatch` (see
`REQ-SCREEN.strip_is_the_opened_set`), so a click on a strip row and a key
carrying `screen.show` do not take the same path.

## `action_to_intent`: agrees, with a note

`dispatch` is total. Every case answers with an `Intent`, and every other name
answers with `refuse`. The second half, "nothing shall act on an action name
directly", depends on call sites in the same way `one_path` does, and the model
cannot express it. The `act_in_bar` bypass above is a place where the shell
acts on a name (`action == "screen.show"`) directly.

## `unknown_is_refused`: agrees, with a note

The set of names `dispatch` handles is exactly the set of action names in the
shipped `assets/keymap.json`, with the six `screen.station.*` names reached
through the prefix. An unknown station also refuses. "No keymap" is read as
the shipped keymap. `dispatch` takes no keymap, so a user keymap that dropped a
binding would leave its action resolvable.

## Evidence that survives this review

Before this review, `focus_is_carried` and `missing_target_is_refused` each had
an `agrees` record by `independent-review-agent` (2026-09-21). Both records are
stale ("the requirement changed"). A drift verdict "records no evidence", so
the old files remain and `--stale` still lists them.
