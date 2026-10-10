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

## Decided under autonomy (night of 2026-10-10)

Given by ricostynha ("I give you autonomy to decide"); judgements recorded by
`claude-review`, delegated by ricostynha.

- **Five new clauses:** `only_what_the_tool_changed`, `requirement_summary`,
  `lines_listed` judged `agrees`. `REQ-STALE.current_not_rerun` split into
  `agreed_not_rerun` (`Staleness.rerun`) and `measured_not_retaken`
  (`Hash.retake`), both modelled, bound and judged. `views_from_the_shell`
  stays Unmodeled: it reads the disk.
- **Sandbox brief:** within `REQ-OBS.no_instruction_channel`, now said by its
  `environment` narrowing: a fixed text naming tools and skills, from the host
  alone, written before the tool starts.
- **Design station:** folds to its roots; marks unfold one level or all under
  a node; the Requirements station is dropped (the user's call).
- **Suite caching:** done after all, in `tools/differential-all.sh` rather than
  in each test: a suite whose every `@drt` clause has a current agreed run is
  skipped (`--drt-due`; `--again` runs all). 2m29 → 32s with nothing changed.
- **Early stop for differential cases:** declined. Measured, 2000 cases cost
  ~1.3s of a ~4s test; knowing "every path covered" per case needs a profile
  export per case, dearer than the case; and an L3 record names a seed and a
  case count that the floors are calibrated to.
- **Module-comment anchoring:** kept; a claim in `//!`/`/-!` anchors to the
  first item. Re-anchoring to the file re-targets ten Lean `@models`; claims
  move onto functions when their file is next touched.
- **Trace station** ("same every time"): closed, not reproduced — it follows
  the file the document shows (`crates/editor/tests/trace_station.rs`).
