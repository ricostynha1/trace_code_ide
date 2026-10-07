# T4 — Hierarchy, roll-up, zoom, and the requirements UI: implementation plan

Design reference: `docs/formal_traceability_and_lsp_plan.md` §7, §9.
Depends on T1 (shipped: `TraceIndex`, `RequirementView`, `RequirementSummary`, assurance).

## What exists after T1
`trace::TraceIndex` already computes per-clause assurance, weakest-link aggregation,
coverage with an open/complete denominator, and children of a requirement. The panel does
not use any of it yet — `RequirementsPanel.tsx` still renders the old flat list from
`list_requirements`.

## 1. Roll-up over the DAG (backend)

Today coverage is computed per requirement over its own clauses. The design also needs
coverage *of a parent over its reachable leaves*:

```rust
pub struct RollUp {
    pub coverage: f32,
    pub coverage_is_lower_bound: bool,   // any reachable node is `open`
    pub assurance: Level,                // weakest leaf clause
    pub leaf_clauses: usize,
    pub exempt_clauses: usize,
    pub stale_clauses: usize,
}
pub fn rollup(index: &TraceIndex, req_id: &str) -> RollUp;
```

Rules, which are the honest-metric rules from §9.2–9.3 and must be tested as such:
- Compute over the **set** of reachable leaf clauses, not by averaging children — the DAG
  means a leaf can be reached by two paths and must count once.
- `coverage_is_lower_bound` is true when **any** node on the reachable subgraph is `open`;
  the UI then renders "≥ x%" and never renders the parent as finished.
- Assurance is the **minimum** over leaf clauses, never a mean.
- Exempt clauses leave the denominator; partial clauses cap the numerator at 0.5.
- Cycles are already reported by the checker; roll-up must still terminate (visited set).

## 2. Zoom (backend supplies, frontend selects)

Five altitudes, one selection model. The backend does not need a "zoom" concept — it needs
to answer the same questions at different granularity:

| Zoom | Source |
|---|---|
| Z1 capability | `rollup()` over roots (requirements with no `refines`) |
| Z2 requirement | `rollup()` per requirement |
| Z3 clause | `RequirementView.clauses` |
| Z4 symbol | `Link.anchor.symbol_path` |
| Z5 region/line | `Link.anchor.start_line`/`end_line` |

New service call: `trace_tree(depth: Option<usize>) -> Vec<TreeNode>` returning the DAG
rendered as a tree with a visited-set (a node reachable twice appears under both parents,
marked `duplicate: true` so the UI can dim the second occurrence rather than pretend the
requirement is two things).

**Collapsing must never improve the picture**: a collapsed parent shows the aggregate of its
children's errors, stale counts and weakest assurance. Worth an explicit test —
`collapsed_parent_is_never_greener_than_expanded`.

## 3. Coverage map (the screenshot that sells it)

`trace_coverage_map() -> Vec<MapEntry>`:

```rust
pub struct MapEntry {
    pub file: PathBuf,
    pub symbol: Option<String>,
    pub lines: u32,                  // size of the rectangle
    pub requirements: Vec<String>,   // colour; empty = grey = untraced
    pub weakest: Option<Level>,
    pub stale: bool,
}
```

Built from the annotation index plus the existing `SymbolTable` for un-annotated symbols, so
grey areas are real (a file with no annotations still appears, sized correctly). This is the
number that tells the truth about adoption, so untraced code must never be omitted.

Frontend: a treemap in `react_frontend/components/CoverageMap.tsx`, plain SVG (no new
dependency), rectangles sized by `lines`, coloured by the first requirement, grey when the
list is empty. Selection is bidirectional — clicking a rectangle filters the outline,
selecting a requirement highlights its footprint.

## 4. Requirements panel rewrite

`RequirementsPanel.tsx` becomes a tree over `trace_tree`, each row showing:
`id · title · coverage bar (with ≥ when a lower bound) · assurance chain · stale count ·
open|complete marker`.

The assurance chain is rendered as its parts — `L4 model · L1 code` — never collapsed into a
single badge, because a proof about a model says nothing about code that nothing binds to it.

Clause rows expand underneath, each with its links and a "why is this not green?" action that
lists the missing roles (`no @models`, `no @drt harness`, `stale since <commit>`).

## 5. Gutter chips (editor)

For the open file, `trace_links_in_file(path) -> Vec<Link>`; the editor renders a chip at
`anchor.start_line` coloured by the clause's weakest level, grey/striped when stale. Hover
shows requirement id, clause text and the evidence summary.

Reuses the existing decoration mechanism in the CodeMirror wrapper; no new editor plumbing.

## 6. Progress over history

`trace_history(limit) -> Vec<HistoryPoint>` where each point is
`{ node_or_commit, at, coverage, assurance_counts }`.

v1 walks git commits (`git log --format=%H %cI -n <limit>`), checks out nothing — it reads
blobs via `git show <sha>:<path>` into a temp dir and rebuilds the index there. Slow but
correct, and cache-keyed by commit sha in `.tracelean/history.json` so each commit is
computed once. Undo-tree-node granularity is deferred; commits are the honest unit for a
shared project.

Frontend: a small line chart in the panel header (plain SVG).

## 7. Tests
- roll-up over a diamond DAG counts the shared leaf once
- one `open` node anywhere makes the whole ancestor chain a lower bound
- assurance is the min, not the mean
- exempt leaves the denominator; partial caps the numerator
- collapsed parent is never greener than expanded
- coverage map includes untraced files as grey
- history cache reuses computed commits

## 8. Order of work
1 `rollup()` + tests → 2 `trace_tree` → 3 panel rewrite → 4 coverage map backend →
5 treemap component → 6 gutter chips → 7 history.

## Risks
- The panel rewrite is the first user-visible change; keep the old list reachable behind a
  toggle until the tree is trusted.
- History over a large repo is slow; cache aggressively and cap the default window.

---

# Revision — as built

All seven steps of §8 are done. Two notes:

1. **Gutter chips clear on any document edit.** The trace index is rebuilt from files on
   disk, so the moment you type, its line numbers describe a document that no longer
   exists. Shifting the chips with the edit would be worse than dropping them: a chip one
   line off is a traceability claim about the wrong code. They come back on the next scan.
2. **The history cache carries a scheme tag.** `.tracelean/history.json` records
   `scheme: "history/v1"`; a change to how a point is computed invalidates the whole file
   rather than drawing old and new numbers on the same chart. Commits that could not be
   materialized are returned as problems alongside the points, so a gap in the line is
   explainable instead of interpolated over.
