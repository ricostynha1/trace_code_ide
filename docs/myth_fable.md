# Myth in Tracelean — Feedback, Reformulation, Implementation

*Response to `docs/myth_rewrite.md` / `docs/MYTH_TEXT_BASE_DESIGN.md`.*

---

## (1) Feedback — is there value here?

**Yes, and more than you may realize: tracelean has already built roughly half of Myth without calling it that.** `core/src/parser.rs` is exactly the "Text Formatter" from the C design — grammar in, `.scm` queries out, capture names resolved through a JSON theme map (`ui_settings/*.json`) into flat spans the frontend renders without touching the AST. The Myth proposal is essentially: *take that pipeline and let captures bind to behavior, not just color.* That is a small conceptual step with a large payoff, and it lands on infrastructure that already exists.

### Where the value is real

**1. Capture → action binding is the genuinely strong idea.**
Today the FileTree, MenuBar, and Editor context behaviors are three separate hand-written widgets (`FileTree.tsx`, `MenuBar.tsx`, custom handlers). Under Myth they become three *data files*: a structure, a query, a binding map. Adding "rename symbol" to the editor or "duplicate file" to the tree becomes a JSON edit plus one Rust function — no frontend work. This is the same leverage Emacs gets from "everything is a buffer with a keymap," and it's why Emacs users can extend the editor in minutes. That property compounds: every new surface you add gets styling, keyboard control, discoverability, and undo *for free*.

**2. It fits tracelean's command architecture unusually well.**
Your `Command` enum ("every mutation is a Command, no exceptions") is the missing piece the C design didn't have. If bound functions don't mutate state directly but instead *return `Command`s*, then every action triggered from any surface — file tree, menu, keybinding — is automatically undoable, serializable, and visible in the undo tree. `DeleteFile`, `RenameFile`, `CreateFile` already exist as commands. The file-tree-as-text idea is not a toy demo here; it plugs straight into machinery you've already tested.

**3. Semantic navigation and modal editing emerge instead of being built.**
`goParent`/`goChild`/`goSibling` as first-class movement, semantic selection, which-key discoverability — these all fall out of "the cursor is on an AST node" rather than requiring separate features. The which-key insight in the rewrite is correct and important: the keymap being structured data means the help system is a query, not a subsystem.

**4. One dispatch model across GUI and TUI.**
You maintain both a React frontend and a `tui/` crate. A keymap-as-data state machine interpreted in core means both frontends send raw key events and get back the same behavior. Right now `tui/src/input.rs` and the React key handling will inevitably drift apart; Myth prevents that class of bug structurally.

### Where I'd push back

**1. Don't literalize "everything is a text buffer" — the invariant you want is "everything is a *node tree with bindings*."**
The file tree rendered as literal styled text loses icons, drag-and-drop, indentation guides, hover affordances — things GUI users expect and that the React frontend can trivially provide. The valuable part of Myth is not that the file tree *is* text; it's that the file tree has a canonical structured representation whose nodes carry style + actions, uniformly queryable and keyboard-drivable. Let the React frontend render that node tree as rich components when it wants to, and as styled text in the TUI. Same model, two renderers. You noted "rendering beautifully is a separate concern" — I'd go further: rendering as literal text is *optional per frontend*, and that resolves the biggest weakness of the idea without giving up any of its power.

**2. You already reached the right conclusion on the keyboard grammar — a full tree-sitter grammar for the keymap is overkill.**
Your note at the bottom of the rewrite ("a json suffices well") is correct. Tree-sitter earns its cost when text is *user-edited free-form* and needs incremental reparse and error recovery. The keymap is config: JSON (or the simple `State: (key):target` format parsed with 50 lines of Rust) gives you the same state machine with less machinery. Keep the grammar version as the proof of concept it was. Same logic applies to the file tree — a line-based parser implementing the same capture interface beats writing and maintaining a `.grammar.js` + generated C parser for `file_line \n file_line`.

**3. The C-era data structures are solved problems in this codebase — drop them.**
`artiststyle_t` with `rel_start`, `STR_t` backing, `local_update_style` — tracelean already has `HighlightSpan`, char-indexed buffers, and tree-sitter's incremental parsing. The open questions #1 (rope/gap buffer) and #6 (incremental style runs) in the rewrite are inherited from the C context; don't carry them into the design. If large-file editing becomes a real bottleneck, adopt `ropey` behind the existing char-index API — but that's orthogonal to Myth.

**4. Answers to your remaining open questions:**
- **Function dispatch (Q4):** a static registry — `HashMap<&'static str, ActionFn>` built at startup, exactly your "dynamic_functions" instinct. No plugin system until you need one. Unknown name in a binding map = startup validation error, not a runtime surprise.
- **Conflict resolution (Q5):** follow tree-sitter highlight convention — *last matching pattern wins* for style; for actions, *union* them (a node can offer all actions from all matching captures) with query order as menu order. Style and actions want different merge rules; making that explicit dissolves the question.
- **Complex/nested queries (Q2):** tree-sitter's `QueryCursor` already handles nested patterns — your existing `run_highlight_query` gets this for free. The limitation in the C code doesn't exist in the Rust stack.

**Verdict: pursue it.** Scope it as a unification of what exists (highlighting pipeline + command system + two frontends) rather than a rewrite, and pilot it on one surface end-to-end before generalizing.

---

## (2) The idea, reformulated

### One sentence

> **Every UI surface is a structured document: content parsed into a node tree, where queries mark nodes, and marked nodes carry style and actions. Rendering, keyboard control, discoverability, and undo are generic services over that model.**

### The core abstraction: `Surface`

A **Surface** is defined by four declarative pieces:

| Piece | What it is | Editor example | File tree example |
|---|---|---|---|
| **Content** | The canonical text/structure | file buffer | rendered tree listing |
| **Parser** | Content → node tree | tree-sitter grammar | line-based parser |
| **Queries** | Node tree → named captures | `highlights.scm` | `@dir`, `@file` per line |
| **Binding map** | Capture → style + actions | `rust.json` theme | `{"@file": {"icon": "…", "actions": ["open_file","rename_file","delete_file"]}}` |

Two rules make it coherent:

1. **The parser is a trait, not always tree-sitter.** `TreeSitterParser` for real languages; `LineParser` (or any cheap custom parser) for regular surfaces like trees, menus, and which-key popups. Both produce the same node-tree/capture interface. Tree-sitter is an implementation choice per surface, not the definition of the system.
2. **The binding map may carry any attributes.** Style attributes (`color`, `bold`, `icon`) are interpreted by the renderer; the `actions` attribute is interpreted by the dispatcher. Frontends ignore attributes they don't understand — so the TUI skips `icon` and the GUI may render `@dir` as a collapsible row instead of a styled line.

### Actions

An **action** is a named Rust function in a static registry:

```rust
type ActionFn = fn(&mut AppState, &ActionCtx) -> anyhow::Result<ActionOutcome>;

struct ActionCtx {
    surface: SurfaceId,
    node: NodeRef,        // capture name, byte/char range, node text
    cursor: CursorPos,
    args: Option<Value>,  // e.g. new name for rename
}

enum ActionOutcome {
    Commands(Vec<Command>),   // state mutations — routed through undo tree
    Ui(UiEffect),             // open popup, focus surface, no state change
    None,
}
```

The critical rule: **actions that mutate state return `Command`s; they never mutate directly.** This routes every surface's behavior through the existing invertible-command/undo-tree machinery. "Delete file from the tree" and "delete file from a keybinding" and "delete file from the AI agent" are literally the same code path.

*(This refines the original note "functions receive the node content and the app state and perform the action" — receiving `&mut AppState` is fine, but the mutation contract is: express changes as Commands.)*

### Keyboard: a mode machine over the action registry

The keymap is data (JSON), interpreted in core:

```jsonc
{
  "Main":    { "C-SPC": "→Options", "up": "goUpLine", "S-up": "goParent", "NM": "insertChar" },
  "Options": { "ESC": "→Main", "c": "copy", "F": "→File", "NM": "→Main" }
}
```

- `→State` = transition (your uppercase-target rule, made explicit rather than case-encoded — survives states named `iOS` and functions named `URLOpen`).
- bare name = action registry lookup, dispatched with the current surface/node as context.
- `NM` = fallback, as in the original.
- **Which-key is a query, not a feature:** current state → list of (key, target) pairs → rendered by each frontend (popup component in GUI, bottom pane in TUI). The keymap is its own documentation.

### What each layer owns

- **Core (Rust):** surfaces, parsers, queries, binding maps, action registry, keymap interpreter, command execution. All behavior lives here.
- **Frontends (React / TUI):** send raw key events and clicks (mapped to a node via position); receive style runs / node trees / which-key data; render as beautifully as they like. Zero behavior.

The Myth thesis, preserved: one architecture — content in, structure applied, style + behavior out. The literal-text rendering becomes one renderer among renderers, which is the version of the idea that survives contact with GUI users.

---

## (3) Implementation overview in tracelean

The strategy is **generalize, don't rewrite**: each phase extends something that already works and ships a usable increment. Rough order of effort: phases 1–2 are small, 3–4 medium, 5 is the pilot payoff, 6+ optional.

### Phase 1 — Binding maps: let captures carry actions

- Extend the theme-map JSON schema (`ui_settings/<lang>.json`) so a capture entry may include `"actions": ["name", …]` alongside style attributes. Existing files remain valid.
- In `core/src/parser.rs`, alongside `get_highlights_query`, add `node_at(pos) -> NodeRef` and `actions_at(pos) -> Vec<ActionName>` (resolve captures covering `pos`, innermost-last, union their actions).
- Merge rules: style = last capture wins (current tree-sitter convention, already what the highlight pipeline does); actions = union in query order.
- Startup validation: every action name in every binding map must exist in the registry (Phase 2), else fail loudly.

### Phase 2 — Action registry

- New module `core/src/actions.rs` (the "dynamic_functions localization" from the original notes): static `HashMap<&'static str, ActionFn>` built in one `register_all()` function.
- First registrations wrap existing code paths: `open_file`, `rename_file`, `delete_file`, `create_file`, `undo`, `redo`, `save` — most are one-line adapters emitting the existing `Command::{RenameFile, DeleteFile, CreateFile, Replace}` variants from `core/src/commands.rs`, so undo-tree integration is free.
- IPC: add `list_actions_at(file, pos)` and `dispatch_action(name, ctx)` in `gui_backend/src/ipc/` next to the existing `get_highlights`.
- **Immediate visible win:** right-click / context-key in `Editor.tsx` shows the actions bound to the AST node under the cursor.

### Phase 3 — Keymap interpreter

- `ui_settings/keymap.json` in the state-machine format above; loader + validator in core (every non-transition target must be a registered action; every transition target must be a defined state).
- Interpreter in core: `(current_state, KeyEvent) -> KeyResult { Transition(state) | Dispatch(action) | Passthrough }`. Frontends forward raw key events over IPC and apply the result; `tui/src/input.rs` switches to the same interpreter, eliminating GUI/TUI keybinding drift.
- Semantic movement lands here: `goParent` / `goChild` / `goLeftSibling` / `goRightSibling` as registered actions using the tree-sitter tree already held by the parser — this is where the "modal editing over the AST" experience first becomes real.

### Phase 4 — Which-key

- Core: `bindings_for_state(state) -> Vec<(Key, Target, Kind)>` — a trivial map lookup since the keymap is data.
- GUI: small popup component listing them after a configurable delay in a non-Main state. TUI: bottom strip. (Rendering it as a Myth surface itself — "turtles all the way down" — is a nice later refactor, not the first version.)

### Phase 5 — Pilot text-based surface: the file tree

This is the proof of the full loop, and the codebase is unusually ready for it:

- Introduce the `Surface` trait in core: `content()`, `parse() -> NodeTree`, `captures()`, `binding_map()`. Two parser impls: `TreeSitterParser` (wraps existing `parser.rs`) and `LineParser` (regex/indent-based, ~100 lines).
- `FileTreeSurface`: renders the workspace as indented text; `LineParser` captures `@dir` / `@file` per line; binding map attaches `{open_file, rename_file, delete_file, new_file}` — all already `Command` variants.
- Frontend: `FileTree.tsx` consumes the surface (nodes + captures + bindings) instead of its own ad-hoc tree state. It may keep rendering rich rows — the point is that behavior and structure now come from the surface, and the tree is fully keyboard-drivable through the Phase 3 keymap with which-key discoverability.
- Success criterion: rename a file from the tree with keyboard only, then undo it from the undo-tree panel. When that works, the architecture is validated end-to-end.

### Phase 6+ — Generalize (as needed, not upfront)

- **MenuBar** as a `LineParser` surface (menus are the easiest case).
- **Dynamic style resolution** (`"color": "@color.value"`): resolve capture-reference values in the core span builder before sending to the frontend — closing the TODO from the original doc; the frontend keeps receiving literal values.
- **Nested/scoped queries** (method-in-class bindings): already supported by `QueryCursor`; expose scoped captures in binding maps when a feature needs them.
- **`ropey` buffer backing** if large-file editing hurts — independent of Myth, behind the existing char-index API.

### What deliberately stays out of v1

- Tree-sitter grammars for trivial surfaces (`LineParser` covers them).
- The custom keymap *grammar* (JSON keymap, per your own updated note).
- Literal text rendering of GUI surfaces (the TUI gets it naturally; the GUI renders surfaces as components).
- User-defined actions / plugin scripting — the registry's design leaves room for it, but static Rust registration is enough until there's a concrete need.
