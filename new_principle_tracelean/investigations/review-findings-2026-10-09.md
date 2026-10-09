# What the first review found wrong, and which side should change

The 2026-10-09 review (simulated, see [action plan](../work/action-plan.md) §1) judged 148
clauses: 70 drift, 5 unmodelable. Details per clause are in
`.tracelean/judging/<REQ>.md`. This is my opinion on each: fix the **code**
(the model and Rust are wrong), fix the **text** (the requirement is wrong or
vague), **split** (one clause says two things, only one is modelled), or
**structural** (see [structural clauses and model choice](structural-clauses-and-model-choice.md)).

Clauses marked † were pinned after being judged drift; their spec needs a
person's look too.

## Bugs — the code does something wrong

| Clause | What happens | Fix |
|---|---|---|
| ROLLUP.min_not_mean †, deterministic_order, ARCH-HONEST.weakest_link † | a diamond loses a child under one parent | [action plan](../work/action-plan.md) §3 |
| SCREEN.resize_has_a_floor | a neighbour at 0 stays at 0 (subtraction clamps) | refuse the resize when a neighbour would drop below 1 |
| SCREEN.strip_is_the_opened_set | a strip row's action carries its text, not a buffer id; the editor bypasses `dispatch` | row action `screen.show <id>`, drop the bypass |
| SBX.passthrough_not_mirrored | `src/build/mod.rs` is never mirrored; `.cache` is | match pass-through names at the root only; add the cache dirs |
| SELFWRITE.suppression_is_consumed | `remaining` is both an age and a use count | separate fields: `age`, consumed on first match |
| MIRROR.binary_handled | a changed binary file is never reported | opaque state carries a hash |
| LOCK.deterministic_bytes | evidence written in input order | sort evidence by key before writing |
| REQDOC.malformed_reported | unclosed frontmatter, misspelt key: silent | report both |
| DRT-TS.params_from_source | `// function f(b, a)` in a comment wins; `this:` counted | skip comments, drop `this`, refuse when ambiguous |
| STRENGTH.kernel_decides † | `sorryAx` wrapped onto a later line of `#print axioms` is missed | read the whole axiom list, not line by line |
| MYTH.escape_terminates, modes_defined | a mode with a missing parent passes `validate`; Escape then loops | `validate` reports missing or absent parents |
| MYTH (side finding) | `reachableFrom` fuel spent by bad `enter`s reports real modes unreachable | fuel = number of modes, not of bindings |
| JUDGE.human_decides | an empty `--by` records L2 | refuse it; delegation per [action plan](../work/action-plan.md) §1 |
| PROV.base_is_honest | the scanner binds a constructor's doc comment to the next declaration | fix the binding; put `@models` on `origin` |
| ANNOT.totality | `@Models` (capital) is silently dropped | report an unknown role |
| CHECK.qualifier_soundness | an expired exemption is not reported | give the check today's date as input |

## The model is right, the text is wrong

| Clause | Why | New text (proposal) |
|---|---|---|
| ACT.dispatch_is_pure | `file.delete` must know whether the file exists | "a function of the action, the focus and the workspace, reading nothing else" |
| MYTH.escape_pops_one | which-key convention: an unbound key cancels the whole menu | "Escape moves one level toward the root; any other unbound key returns to the root" |
| SCREEN.close_collapses_the_pane | closing the last buffer would leave nothing to show | "…except the last opened buffer, which stays" |
| SCREEN.buffer_goes_home | pages a person reads (requirement, search, context) belong in the document pane | name the kinds: menus to the side panel, pages to the document pane |
| SCREEN.station_produces_a_buffer | prose says four stations; there are six | fix the count; the model should also call the producer (code) |
| ROLLUP.open_is_lower_bound, ARCH-HONEST.lower_bound_marked | an unwritten clause can only lower the fraction, so the shown figure is an upper bound | "an open figure shall be marked as provisional (it can only fall)"; render the mark (code) |
| MIRROR.apply_reproduces | protected paths are deliberately not mirrored | "reproduces the observed tree on mirrored paths"; report refused commands (code) |
| ROLLUP.untraced_counted | files are counted, not items | say "per file" now; items later, with line coverage |
| CHECK.severity_policy | "small" cannot be checked | list the blocking kinds; take the policy as input (code) |
| DRT-COVER.law_coverage | its own prose says laws are not designed yet | mark `@partial` until they are |
| STRENGTH.obligation_generated | the old `--strength` generator emits `True`; `pinning::statement` replaced it | re-point the clause at `pinning::statement`, delete the old generator |

## The model covers part of the clause — split, extend or fix

Rule ([action plan](../work/action-plan.md) §7): **split** when the parts can
be met independently, preferred; **extend** the model when one mechanism
serves every input; **fix** when the code or an annotation is simply wrong.

| Clause | Do | Missing part, or the split |
|---|---|---|
| ACT.focus_is_carried, missing_target_is_refused | fix | definition/references ignore the focus in `dispatch` — move the cursor read into `dispatch` |
| ACT.edits_are_commands | fix | sandbox accept resolves to no commands — resolve it to the diff as a batch |
| SHOW.menu_from_keymap | fix | build entries from the keymap mode in core |
| CHECK.named_kinds | fix | split the `malformed` finding into its kinds |
| JUDGE.advice_is_not_evidence | fix | its model is the prompt; it should be `record` |
| UNDO.filtered_view † | fix | a node touching two files carries both |
| REQDOC.clause_addressable | fix | hash per clause — [action plan](../work/action-plan.md) §5 |
| VIEW.screen_is_readable | split | the frontend draws it · it can be read back without trusting the frontend (`pointer.mjs`) |
| ROLLUP.exempt_leaves_denominator | split | exempt clauses leave the denominator · partial clauses count capped |
| DOCLINK.decisions_exempt | split | decision records are exempt · they must declare `affects:` |
| LINECOV.clause_summary † | split | coverage per implementing item · how items combine for the clause |
| STALE.inputs_identified | split | one clause per record kind: judgement, DRT (names the test), proof (names the theorem) |
| STRENGTH.nondeterministic_declared | split | shown as excluded, not a pass · a reason is required |
| ANCHOR.stable_under_move | split | inserting above keeps the identity (symbol path, not offsets) · moving across scopes (new text) |
| ANNOT.qualifiers | split | attaches to the preceding annotation · an orphan qualifier is reported |
| CHECK.structural_is_not_exempt | split | needs a test, not a model · stays in the denominator · capped below L3 |
| DRT-PROTO.op_dispatch † | split | op names are unique · a case dispatches on its `op` |
| REQDOC.decomposition_claimed | split | open by default · rendered as provisional |
| ARCH-DETERMINISM.stable_ordering | split | one clause per collection written out (or structural) |
| TRANSCRIPT.absent_is_fine | split | a missing transcript reads as empty (`Option` input) · the workspace alone suffices |
| DRT-GEN.seed_reproduces | extend | the other schemas (struct, list, option, bool, int, enum) |
| DRT-PROTO.failure_named | extend | start failure, timeout, death |
| DRT-PROTO.line_delimited | extend | writing a case as one line |
| DRT-PROTO.runner_shared | extend | one process per language |
| DRT-RUST.types_inferred, DRT-SCHEMA.derived_from_both | extend | model `derive` and the generated call site |
| DRT-COVER.vacuous_named | extend | floor 0 is vacuous; report every short floor |
| SHOW.review_from_change | extend | multi-file, add and delete changes |
| SHOW.graph_from_refinement | extend | requirements with a missing parent or in a cycle; chains deeper than 4 |
| SHOW.producers_are_total | extend | a kind → producer table with a totality proof |
| VIEW.affordances_named | extend | span actions checked against the actions the keymap knows |
| VIEW.presentation_may_be_symbolic †, text_is_the_content | extend | conformance per region, not per row; a level colour checked against its text |
| ANCHOR.imprecise_capped † | extend | tie `precise` to "a grammar read this file" |
| STALE.no_silent_revalidation | extend | only a fresh record from the owning backend revalidates |
| OBS.workspace_is_a_copy † | extend | extra files in the copy; missing `.git`/`.tracelean` |

## Structural — no function can show it

ACT.one_path, CMD.single_path, SCREEN.one_arrangement_path, SHOW.core_produces,
SHOW.producer_is_pure, PERSIST.append_only. See [structural clauses and model choice](structural-clauses-and-model-choice.md).

Also wrongly attached though judged *agrees*: SBX.real_tree_untouched (its
model is `copyCheck`; the real check is `copyViolations`), CMD.round_trip (holds
only for sorted workspaces).
