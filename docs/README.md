# TraceLean IDE

AI-powered code IDE with formal verification and full traceability: natural-language requirements → Lean 4 specs → implementation code.

## Core Principles

- **Tree-sitter first**: All parsing, highlighting, structural queries via tree-sitter (WASM in editor, native in backend).
- **Formal traceability**: Every requirement maps to a Lean spec, every spec maps to code, every code maps to tests.
- **AI-assisted workflow**: OpenRouter for requirement elicitation, formalisation, code generation, and repair.
- **Enforcement**: Background agent validates code against specs on every edit.
- **Undo-tree**: Emacs-style branching undo, command-based, multi-file aware, with named commit points.
- **Remote execution**: Edit locally, offload heavy compute (Lean, tests) to remote Docker hosts.
- **Collaborative editing**: Free from command architecture — stream commands between users, states converge.
- **Docker-native**: Entire system runs in containers. `docker compose up` = fully functional.

## Docs

- [Requirements](./REQUIREMENTS.md) – Functional and non-functional requirements.
- [Design](./DESIGN.md) – Architecture and technology choices.
- [Implementation](./IMPLEMENTATION.md) – Incremental MVPs and tasks.
- [Phases](./PHASES.md) – High-level implementation roadmap.
- [Open Questions](./OPEN_QUESTIONS.md) – Risks and unknowns.
