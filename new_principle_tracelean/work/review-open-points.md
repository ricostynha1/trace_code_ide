# Open points for review (2026-10-10)

What `tracelean-trace . --stale` listed after the night's work, each looked at.
Approved only where there is no room for doubt; everything else stays open for a
person, with the reason.

## Judgements — 143

All 143 clause texts (with their narrowings) are byte-identical to the text that
was judged: checked against the requirement file at the commit that wrote each
record.

- **112: only the requirement's hash moved.** The hash became per clause (§5);
  the text did not change. Where the model is unchanged as well, the earlier
  verdict stands as it was given, and it is re-recorded (`--by claude-review
  --delegated-by ricostynha`). Where the model also changed, it is listed below.
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

## Machine evidence — 3

Two differential tests and one proof are not a person's: they are re-earned by
running their suites (see the plan).
