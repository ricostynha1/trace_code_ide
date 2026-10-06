# Developer guide

How the editor is built, and how to change it: keys, actions, colours, what
a panel shows. For using it, see [USING.md](USING.md).

## The one idea: everything on screen is a buffer

The core (`crates/core`) produces **buffers**; frontends only draw them.

```
Buffer { id, kind, text, spans }
  kind  : file | directory | review | menu | record   (where it belongs)
  text  : exactly the characters shown
  spans : [ { start, stop, role, actions } ]          (character offsets)
```

- A **role** says what a region *is* — `path`, `heading`, `requirement`,
  `level{grade}`, `token{kind}` (syntax), `claim{role}` (implements/tests/…),
  `added`, `removed`. Never how it looks.
- **actions** name what can be done there (`file.open`, `trace.requirement`),
  in the keymap's vocabulary. A frontend turns them into buttons (window) or
  keys (terminal); it never invents one.

A file, the explorer, the requirements panel, the which-key bar, the status
line and the tab strip are all buffers. Two frontends draw them:

| Frontend | Code | Draws a role as |
|---|---|---|
| window (Tauri) | `crates/desktop` + `web/src/*.ts` + `web/style.css` | CSS class `role-*`, `token-*`, `claim-*` |
| terminal | `crates/tui` (`draw.rs`) | a truecolour escape |

Because the text is the core's, a frontend cannot show something the core did
not say — `crates/core/tests/one_representation.rs` and the frontend
conformance tests check it.

```
crates/core     what to show and what an action means — pure functions
  surface/      producers: explorer.rs, requirement_view.rs, sandbox_view.rs,
                highlight.rs (syntax), chips.rs (gutter), offer.rs (menus),
                act.rs (action → intent), keymap.rs, screen.rs (panes, layout)
  trace/        requirements, annotations, the checker
crates/editor   the session: buffers opened, cursor, folders open; performs intents
crates/desktop  the window's commands (one line each, onto the editor)
crates/tui      the terminal
formal/         Lean models of the core; checked against it by differential tests
```

## How the file system is displayed

1. The editor scans the project into a **workspace** (`Editor::workspace`,
   cached): every file's path.
2. `surface::explorer::rows(files, open_folders)` turns paths into rows —
   folders first, then files, by name; a closed folder hides its contents.
   Which folders are open is session state (`Editor.open_folders`), restored
   from `.tracelean/editor.json`.
3. `explorer::tree_buffer` writes the rows as a `directory` buffer: the
   project name as a heading, `▸`/`▾` before folders, two spaces per depth,
   the shown file as a `heading`, a badge after a row an agent changed (`●`,
   `+` created, `✗` deleted) or with unsaved edits (`✎`). Every row carries
   `file.open`; on a folder that toggles it.
4. `screen::destination` sends a `directory` buffer to the explorer pane, a
   `file` to the document, a `menu`/`record` to the side panel.
5. The window draws each line as a `.line` div, each span as a `<span>` or a
   `<button>` (if it has actions). A click sends `act` with the pane and the
   character offset; the editor resolves the row to its path
   (`explorer::path_at`) — nothing in the page reads paths out of text.

To change what the tree shows (icons, ordering, badges), change
`explorer.rs` and its tests; both frontends follow.

## Change a key

Keys are data: **`assets/keymap.json`**. Modes nest like Emacs prefixes:

```json
["File", { "parent": "Leader", "bindings": [
  ["s", { "dispatch": { "action": "file.save", "description": "save" } }],
  ["x", { "enter":    { "mode": "Sub",        "description": "more…" } }]
]}]
```

`dispatch` runs an action; `enter` opens a mode, whose bindings appear in the
which-key bar. Rebuild after editing (the keymap is compiled in), then
`cargo test -p tracelean-core --test shipped_keymap` — it refuses a binding
to an action that does not exist, a mode nothing enters, or two meanings
for one key.

Editor chords (`Ctrl+S`, arrows…) are `Editor::chord` in
`crates/editor/src/lib.rs`, listed in its `CHORDS` table for F1; the page
maps browser keys to them in `web/src/app.ts` (`chords`).

## Add an action

Say `file.duplicate`:

1. **Name it** in `ACTIONS` (`crates/core/src/surface/keymap.rs`).
2. **Say what it means** in `dispatch` (`crates/core/src/surface/act.rs`):
   an `Intent` — `Display` a buffer, `Edit` a file, `Observe` something,
   `Arrange` panes, `Refuse` with a reason. The core decides; it does no IO.
3. **Mirror it in Lean**: the same case in `dispatch`
   (`formal/TraceLean/Act.lean`); `cd formal && lake build`.
4. **Do it** in the editor's `perform` (`crates/editor/src/lib.rs`) if the
   intent is new — that is where files are written.
5. **Offer it**: a row in `surface/offer.rs` puts it in the right-click
   panel and the which-key bar; a key in `assets/keymap.json`.
6. **Test**: `cargo test -p tracelean-core --test differential_act -- --include-ignored`
   generates thousands of foci and checks Rust and Lean agree.

## Add something to a panel

A panel is a producer: data in, `Buffer` out. Look at
`surface/requirement_view.rs` — it builds lines with `Lines::line(&[(text,
role, actions)])`. Add a line, give its pieces roles and actions, and add a
test asserting the text and the action at an offset (see the tests at the
bottom of that file). If the buffer has a Lean model, change both and run
its `differential_*` suite.

A new role is a bigger change — it is the vocabulary both frontends draw:
`Role` in `surface/view.rs` and `formal/TraceLean/View.lean`, `actionsFor`
in `produce.rs`/`Produce.lean`, the terminal's `colour` in `crates/tui/src/draw.rs`,
`Role`/`sameRole` in `web/src/view.ts`, `classOf` in `web/src/render.ts`, a
CSS rule, and the role schema in `crates/core/tests/support/mod.rs`.

## Change colours and fonts

**Every colour and font is in `assets/theme.json`** — no colour is written
anywhere else. Sections:

| Section | Colours |
|---|---|
| `ui` | window chrome: backgrounds, tabs, status bar, menus, selection, caret |
| `fonts` | `code`, `codeSize`, `chrome`, `chromeSize` |
| `roles` | what each role is drawn in (`path`, `heading`, `levelL1`…) |
| `chips` | gutter chips and claim pills: `implements`, `tests`, `models`, `proves`, `drt` |
| `syntax` | token kinds: `keyword`, `string`, `comment`, …, Markdown `heading`/`bold`/`italic`/`link` |

The window turns each into a CSS variable named `--section-key`
(`ui.accent` → `--ui-accent`); `web/style.css` only refers to variables. The
terminal reads the same file. A project can override any key in its own
`.tracelean/theme.json`; the window re-reads it when it regains focus.

To add a colour: add the key to `theme.json`, use `var(--section-key)` in
`style.css` (or `theme::colour(theme, "section", "key")` in the TUI).

## Look at what you changed

```bash
node web/build.mjs                       # TypeScript → web/dist
node web/test/look.mjs shot.png demo \
  'choose={"pane":"explorer","offset":0,"action":"file.open","target":"src/celsius.rs"}'
```

`look.mjs` runs a real editor (`crates/editor/examples/look.rs`, over
stdin/stdout), serves the page to headless Chrome, runs each step as a
window command, and saves a screenshot — without opening a window.

## Tests to keep green

```bash
cargo test --workspace -- --include-ignored   # unit, architecture, every Rust↔Lean suite
cd formal && lake build                       # the Lean models and proofs
node web/build.mjs && node web/test/pointer.mjs   # the page in a real browser
cargo run -p tracelean-core --bin tracelean-trace -- .   # must say blocking: false
```

The architecture tests hold the lines this design depends on: the core does
no IO outside declared shells, nothing calls a model or the network, and the
editor starts no processes (`ARCH-NO-DRIVING`).
