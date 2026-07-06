# Requirements — Traceability Dashboard Gaps

Source: docs/IMPLEMENTATION.md MVP5, tasks 5.5 and 5.8 (unchecked).

## Requirements

| ID | Requirement |
|----|-------------|
| TD-01 | Dashboard offers a call-graph / dependency-graph view showing function-level call relationships within the project. |
| TD-02 | Call-graph view is toggleable alongside existing requirement/spec/code/test trace view (same window, view switcher). |
| TD-03 | Call-graph edges derived from tree-sitter symbol table (function calls resolved to defining symbol where possible). |
| TD-04 | Unresolved/external calls (stdlib, unknown symbols) shown as leaf nodes, visually distinct from local calls. |
| TD-05 | AI chat panel shows live agent activity: current action (e.g. "reading file X", "querying trace graph"), referencing graph nodes it touches. |
| TD-06 | Related graph nodes touched by agent are highlighted in the traceability graph while agent is active. |
| TD-07 | Agent activity feed updates in near real time (event-driven, not polled). |

## Out of scope

- Editing the call graph.
- Cross-project call graphs.
