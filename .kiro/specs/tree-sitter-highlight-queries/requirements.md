# Requirements — Tree-sitter Highlight Queries

Source: docs/IMPLEMENTATION.md MVP1 task 1.8.

## Problem

Current highlighting: ad-hoc node-kind → color JSON map. Fails on nested structures (markdown headings, etc) where color must propagate through child nodes.

## Requirements

| ID | Requirement |
|----|-------------|
| HQ-01 | Editor highlighting driven by tree-sitter `.scm` highlight query files, one per language. |
| HQ-02 | Highlight queries assign standard capture names (`@keyword`, `@string`, `@function`, `@comment`, etc) per tree-sitter convention. |
| HQ-03 | Capture-name → color mapping is theme-configurable, decoupled from grammar-specific node kinds. |
| HQ-04 | Markdown headings and other nested-node cases render correct color through the full span, not just the outer node. |
| HQ-05 | Adding a new language requires only a new `.scm` file, no highlighter code change. |
| HQ-06 | No regression in highlighting for existing languages (Python, Rust, C++, Lean 4) after migration. |

## Out of scope

- New language grammars.
- Semantic (LSP-based) highlighting.
