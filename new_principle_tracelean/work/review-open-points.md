# Review points (2026-10-10) — resolved

All approved by ricostynha on 2026-10-10 ("I approve everything"); recorded as
such, and `tracelean-trace . --stale` lists nothing.

- **Judgements:** 98 re-recorded by delegation earlier; the remaining 46 (14
  rewritten models, 31 moved annotations, `neighbourhood_is_closed`) recorded
  `agrees` by ricostynha. The aggregate functions a binding drives stay the
  clauses' models; `neighbourhood` lists a requirement in a refinement cycle
  in both directions, as modelled.
- **Documents:** six confirmed earlier; `06-editor → REQ-SHOW, REQ-ACT,
  REQ-ROLLUP` hashes recorded.
- **Palette:** the ten One Dark pairs below WCAG contrast keep their waivers in
  `assets/theme.json` (`contrast`); `contrast_sufficient` reports any that a
  later edit fixes.
- **REQ-LOOK:** approved as written (seven clauses). `roles_drawn_in_theme_colours`
  is checked on the terminal; the page colours some roles by place (a path in
  the explorer wears `ui.sidebarText`), which no check holds it to yet.

## Open (evening of 2026-10-10)

Clauses added to approved requirements in answer to your feedback, and the
models written for them. Not judged: you have not read the clauses yet, so
no verdict is beyond doubt (`work/feedback-2026-10-10.md`).

| Clause | Model | Why left open |
|---|---|---|
| `REQ-OBS.only_what_the_tool_changed` | `Mirror.changedSince` | the clause is new; also "a copy whose start is unknown offers nothing" is a choice (the alternative: offer by mtime) |
| `REQ-LINECOV.requirement_summary` | `RequirementView.total` | new clause |
| `REQ-LINECOV.lines_listed` | `CoverageView.coverageView` | new clause; the page's layout is a choice |
| `REQ-STALE.current_not_rerun` | `Hash.sourcesHash` | the model covers coverage only; the `--drt` half (`auto::still_agreed`) is claimed but not modelled |
| `REQ-CONTEXT.views_from_the_shell` | none | reads the disk through the editor: no value function answers it |

Also yours to decide: whether the sandbox brief (a fixed `CLAUDE.md` above the
copy) is within `REQ-OBS.no_instruction_channel`.
