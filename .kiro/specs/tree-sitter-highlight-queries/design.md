# Design — Tree-sitter Highlight Queries

## Approach

Replace ad-hoc `node-kind → color` JSON with standard tree-sitter `highlights.scm` query files, executed via the WASM tree-sitter query API already loaded in the webview.

## Pipeline

```
source text
  → tree-sitter parse (WASM, existing)
  → run highlights.scm query against tree
  → query yields (capture_name, node_range)[]
  → capture_name resolved to CSS class / color via theme map
  → CodeMirror decoration built from ranges
```

## Query files

- `grammars/<lang>/highlights.scm` per language (Python, Rust, C++, Lean 4, Markdown).
- Standard captures: `@keyword`, `@string`, `@number`, `@comment`, `@function`, `@function.call`, `@type`, `@variable`, `@property`, `@constant`, `@punctuation.delimiter`, `@markup.heading`, etc.
- Nested capture resolution: tree-sitter queries naturally handle nested spans (a `@markup.heading` capture on the heading node plus `@markup.heading.marker` on the `#` marker) — this is the fix for the markdown-heading propagation bug, since the query targets sub-nodes directly instead of relying on a single top-level color assignment.

## Theme map

```ts
const captureColors: Record<string, string> = {
  "keyword": "var(--color-keyword)",
  "string": "var(--color-string)",
  "comment": "var(--color-comment)",
  "function": "var(--color-function)",
  "markup.heading": "var(--color-heading)",
  // ...
};
```

Longest-prefix match on dotted capture names (e.g. `markup.heading.marker` falls back to `markup.heading` then `markup` if no exact entry).

## Migration

- Keep old JSON-map highlighter behind a feature flag until `.scm` queries are verified equivalent for all 4 existing languages.
- Remove old highlighter once parity confirmed.

## Risks

- Query performance on large files: mitigate with incremental re-query on edited range only (tree-sitter supports range-limited query execution).
- Capture name drift between community grammars: pin grammar + query file versions together.
