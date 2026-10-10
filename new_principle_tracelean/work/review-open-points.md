# Open points for review (2026-10-10)

What `tracelean-trace . --stale` listed after the night's work, each looked at.
Approved only where there is no room for doubt; everything else stays open for a
person, with the reason.

## Judgements — 143

All 143 clause texts (with their narrowings) are byte-identical to the text that
was judged: checked against the requirement file at the commit that wrote each
record.

- **98 re-recorded.** `--stale` now names the inputs that moved. 72 had only
  the requirement's hash move (per clause, §5); 26 more had the model's hash
  move with the model declaration byte-identical to the one judged (compared
  at the commit that wrote each record). Each was `agrees`, with no note, and
  is re-recorded as `agrees` (`--by claude-review --delegated-by ricostynha`,
  with a note saying why).
- **14: the model itself was rewritten** since it was judged (§7 and the
  coverage, judge and keymap work: about 800 changed lines across
  `Coverage`, `Judge`, `Keymap`, `Requirement`, `Policy`, `Mirror`). **Left
  open**: whether the rewritten model still says the clause is a judgement.
  REQ-DRT-COVER.floor_stated, REQ-JUDGE.caps_at_judgement,
  REQ-JUDGE.proposal_not_mutation, REQ-MYTH.actions_defined,
  REQ-MYTH.actions_reachable, REQ-MYTH.keymap_is_data,
  REQ-REQDOC.clauseless_uniform, REQ-REQDOC.id_is_identity,
  REQ-SBX.classification_total, REQ-SBX.protected_never_mirrored,
  REQ-SELFWRITE.content_matched, REQ-SELFWRITE.no_deadlock,
  REQ-SELFWRITE.own_writes_ignored, REQ-SELFWRITE.unmatched_is_external.
- **31: the model annotation moved** (the one-model-per-clause cleanup, §7).
  The single model left is now the aggregate function a binding drives
  (`runScript`, `replayReport`, `conformance`, `facts`, …) rather than the
  helper that says the one thing the clause is about. Whether that aggregate
  *says* the clause is a judgement, not a formality — e.g. `REQ-UNDO.reachable`
  is now modelled by a report of every node's jump outcome. **Left open.**
  Decision for a person: keep the aggregate as the model, or model the clause
  by the specific function again (`push` for `reachable`) and bind that.

  ARCH-DETERMINISM.replay_exact, REQ-ACT.action_to_intent,
  REQ-ACT.unknown_is_refused, REQ-ANNOT.role_vocabulary,
  REQ-CHECK.progress_not_fault, REQ-CMD.batch_reverses, REQ-CMD.inverse_exists,
  REQ-CMD.round_trip, REQ-COST.price_is_per_model,
  REQ-DRT-RUST.params_from_source, REQ-MIRROR.diff_is_pure, REQ-MIRROR.minimal,
  REQ-MIRROR.ordering_defined, REQ-PERSIST.checkpoint_equivalent,
  REQ-PERSIST.replay_exact, REQ-PERSIST.truncated_is_reported,
  REQ-REQDOC.refines_dag, REQ-SBX.real_tree_untouched,
  REQ-SCREEN.opened_outlives_shown, REQ-SCREEN.screen_is_a_value,
  REQ-STRENGTH.attempted_distinguished, REQ-STRENGTH.open_is_the_default,
  REQ-UNDO.jump_equivalence, REQ-UNDO.no_loss_on_branch,
  REQ-UNDO.path_via_ancestor, REQ-UNDO.preview_is_pure, REQ-UNDO.reachable,
  REQ-VIEW.everything_is_a_buffer, REQ-VIEW.frontend_adds_nothing,
  REQ-VIEW.rendering_is_total, REQ-VIEW.structure_over_text

  Note for the two REQ-ACT ones: `Act.dispatch` changed on 2026-10-10 — it
  now puts a target carried in the action (`history.filter Saved`) under the
  focus before dispatching the name (`dispatchNamed`), as the Rust always
  did; widening the generator found the gap. Judge against that model.

  New: `REQ-CONTEXT.neighbourhood_is_closed` is now modelled and bound by
  `neighbourhood` (both directions) instead of `ancestors` alone. **Left
  open:** in a refinement cycle a requirement is both above and below and
  appears in both lists; whether that is "each once" is a reading of the
  clause, not a formality.

## Documents in review — 9

Each read against what changed in the requirement since it was confirmed:

- **Confirmed (6):** `00-methodology → ARCH-HONEST` (lower bound → provisional,
  already worded so), `02-scope`, `05-view` and `09-using-the-tui → REQ-MYTH`
  (escape and parent rules sharpened; the documents say nothing against them),
  `04-coverage → ARCH-HONEST`, and `04-coverage → REQ-DRT-COVER` after adding a
  short section on classes, lines and waivers, which the clauses now require.
- **Left open (3):** `06-editor → REQ-SHOW, REQ-ACT, REQ-ROLLUP`. The review
  findings already named this document as needing a person's read against
  twenty SHOW clauses and the new roll-up clauses (`counted_once`,
  `partial_capped`); that is not a formality.

## Palette — 10 pairs below WCAG contrast

`REQ-LOOK.contrast_sufficient` (draft, to approve) computes every declared pair
in `assets/theme.json`. Ten One Dark values are below their minimum and are
waived there for now: `syntax.comment` and `ui.gutterText` 2.32,
`ui.contextMenuKeys` 2.16, `ui.covered` 2.77 (needs 3), `ui.error` 3.89,
`ui.muted` 3.95, `roles.levelL1`, `roles.removed`, `syntax.property`,
`syntax.heading` 4.38. Decision for a person: lighten them (e.g. comment
`#9097a3` reaches 4.76) or keep the palette and its waivers.

## REQ-LOOK clauses — draft, to approve

`roles_drawn_in_theme_colours`, `row_fits`, `regions_present` and
`focus_visible` were added with the visual checks (2026-10-10). The checks
found and fixed: the side panel focused with nothing marking it; the TUI's
stations row wrapping at 100 columns and scrolling the screen; three stations
off the edge; the status line's reverse video never shown; a click on the
cursor's own character sending the cursor to the line's end (page).

Decision for a person: `roles_drawn_in_theme_colours` holds for the terminal.
The page recolours some roles by place on purpose (a path in the explorer wears
`ui.sidebarText`, not `roles.path`). Either the clause names the place-specific
looks (and `look_of` takes the pane's kind), or it stays terminal-only.

## Machine evidence — done

Pinnings re-earned (`--pins`); `REQ-SHOW.producer_is_pure` given the
`@models` its binding drives (`reviewBuffer`), without which its record could
never be current; the records of `REQ-DRT-PROTO.line_delimited` and
`op_dispatch`, clauses split into others in `31d0177`, removed.

What `--stale` lists now is a person's only: 45 judgements (31 moved
annotations, 14 rewritten models) and the three `06-editor` links.
