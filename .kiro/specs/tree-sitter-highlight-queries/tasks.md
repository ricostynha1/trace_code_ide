# Tasks — Tree-sitter Highlight Queries

- [x] 1. Add `.scm` highlight query files for Python, Rust, C++, Lean 4, Markdown (reuse community queries where license-compatible, adapt otherwise).
- [x] 2. Wire tree-sitter WASM query execution in webview: run query per visible range, return (capture, range)[].
- [x] 3. Implement capture-name → color theme map with longest-prefix fallback.
- [x] 4. Replace CodeMirror decoration builder to consume query captures instead of node-kind JSON map.
- [x] 5. Fix markdown heading case specifically: verify `@markup.heading` + marker sub-captures render full-span color.
- [x] 6. Feature-flag old highlighter; run both side-by-side in dev to compare output on existing test files.
- [x] 7. Tests: snapshot test per language — known source file → expected capture list.
- [x] 8. Remove old node-kind JSON highlighter once parity confirmed for all languages.
- [x] 9. Update docs/IMPLEMENTATION.md: mark 1.8 done.
