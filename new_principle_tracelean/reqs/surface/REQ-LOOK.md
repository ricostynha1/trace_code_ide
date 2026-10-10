---
id: REQ-LOOK
title: What is shown can be seen and used
refines: [REQ-VIEW, REQ-SCREEN]
status: draft
decomposition: open
clauses:
  contrast_sufficient: Every pair of colours the theme declares as drawn together shall have a contrast ratio, by WCAG's relative luminance and computed from the theme itself, of at least the minimum the pair declares — 4.5 for text, 3 for marks that are not text — or shall carry the theme's reason for keeping it below; a reason on a pair that meets its minimum shall be reported.
---

# What is shown can be seen and used

[REQ-VIEW](REQ-VIEW.md) checks *what* every frontend shows: the text, the spans
and the actions on them. Nothing checked *how* it is shown — whether a role's
colour can be read against the background it is drawn on. Only a person looking
at the window noticed. This requirement makes that decidable, starting where it
needs no frontend at all: the theme.

`assets/theme.json` declares, beside its colours, the pairs drawn together
(`contrast`), each with the least ratio it needs. The ratio is WCAG's, so the
thresholds are the ones accessibility tooling uses. A pair kept below its
minimum names why, which keeps a palette decision visible instead of silently
failing or silently passing; a reason left on a pair that has since been fixed
is reported, so reasons do not outlive their cause.

Which pairs a frontend really draws is the next clause's to check, from the
cells and elements each frontend renders (`work/visual-testing-proposal.md`).
