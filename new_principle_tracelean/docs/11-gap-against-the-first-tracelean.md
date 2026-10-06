# What the first TraceLean could do that this one cannot

Written 2026-10-05, from a full inventory of the first tree
(`../tracelean/react_frontend`, `../tracelean/gui_backend`, `../tracelean/core`)
against this one. The question asked was blunt: *the new IDE is not usable; what
was lost?* The answer is below, with the reason each thing is missing and what
bringing it back would mean under this project's rules.

## The verdict

The first TraceLean was a working IDE with a traceability layer on top. This one
is a traceability core with an editor-shaped surface that a person can barely
operate. Its *model* is sounder — everything on screen is a buffer the core
produced, every arrow is modelled in Lean and differentially tested — but the
*interaction layer* that lets a person act on it was never built. Almost every
action exists in the core (35 of them) and is reachable only by a three- or
four-key leader sequence nobody can discover.

Three things make it unusable today, and they come before any missing feature.

## The three blockers

### 1. Clicking does nothing — including opening a file

Listing rows carry `file.open`, and the window draws them as buttons. A click
still never reaches them:

- `web/src/app.ts` binds `mousedown` on every pane to `grab(pane, 0)`, which
  calls the editor and then **redraws every pane** (`show` empties `#buffer`).
- The browser fires `click` only when `mousedown` and `mouseup` land on the
  *same element*. The button under the pointer was replaced in between, so the
  click is dropped.
- Even when it isn't, `act` resolves against the **focused** pane
  (`crates/desktop/src/main.rs::act` uses `editor.focus()`), not the pane that
  was clicked.

Fix: send the pane with the action (`act { pane, action, offset }`, the core
focuses then dispatches — one call, one redraw), and don't redraw on a
`mousedown` that only focuses. Small, and nothing else is usable until it lands.

### 2. No way to see what you can do

The first IDE had a menu bar (Open Folder, Requirements, Undo Tree, Project, AI
Chat, Lean, Sandbox), toolbar buttons in every panel, tooltips, and a
**right-click menu in the editor** listing every action at the pointer
(`myth_actions_at`, grouped, with LSP code actions). This one has a status line
that says *space opens the leader menu*.

The core already answers the question a context menu asks:
`view::actions_at(buffer, offset)` gives every action of every span covering a
position. What is missing:

| Needed | Status |
|---|---|
| A right-click on any element opens a menu of its actions | **missing** — no `contextmenu` handler |
| Actions that apply to the buffer as a whole (save, close, split, undo…) offered alongside the span's own | **missing** — spans carry only `file.open`; everything else lives only in the keymap |
| Each entry shows its key binding, so the menu teaches the keys | **missing** |
| Hover tooltips naming what a thing is / does | partial — a button's `title` is its raw action names |

The honest way to build it here: a core function `offered_at(buffer, offset,
keymap) -> Buffer` (a menu buffer, so it is checkable like every other screen),
returning the span's actions plus the buffer-kind's actions, each row carrying
its key. Both frontends draw it; the window at the pointer, the terminal under
the cursor. That keeps `REQ-VIEW.frontend_adds_nothing` true — the page invents
no action — and gives a terminal user the same list.

### 3. Editing is not really possible with a mouse

There is no click-to-place-cursor (a click on plain text only focuses the pane),
no selection, no copy/paste, no scroll wheel, no `Ctrl+S`, no `Ctrl+Z`. Typing
works only after pressing `i`; saving is `Space f s`. The first IDE was
CodeMirror 6: every standard editing gesture, search (`Ctrl+F`), per-file undo
(`Ctrl+Z`), save (`Ctrl+S`) with a `.lean` type-check after it.

## Feature by feature

Legend — **Fits**: can it come back without breaking this project's
architecture (`ARCH-NO-DRIVING`, `REQ-VIEW.one_representation`)?

### Layout and look

| First TraceLean | Now | Fits | Notes |
|---|---|---|---|
| Explorer left · editor centre · panels right | **done** (`REQ-SCREEN.workbench_has_three_places`) | yes | Panels in the first were independent toggles that could all be open side by side; here the side pane shows one buffer at a time. |
| Drag splitters | present (divider drag), untested by a person | yes | |
| Tab per open file, active tab marked | strip of opened buffers, no "active" mark | yes | needs the strip to say which is shown (a role). |
| One Dark syntax highlighting (tree-sitter + `highlights.scm`, colours from `ui_settings/theme.json`) | **missing** — files render as flat text | yes, if the **core** classifies tokens | The page may not parse text. Port the first tree's tree-sitter highlighter into the core as span roles (`Role::Token { kind }`); the theme's `syntax` section (already in `assets/theme.json`, copied verbatim) colours them. |
| Line numbers, active-line highlight | missing | yes | line numbers are a gutter the frontend can draw from line index; active line from the cursor the core reports. |
| Colours easy to change | **done** — `assets/theme.json` + per-project `.tracelean/theme.json`, read by both frontends, reloaded on window focus | yes | The first had hard-coded CSS for chrome; only syntax colours were JSON. |
| Warning toast, status bar with save/check state | status line only | yes | |

### Files

| First TraceLean | Now | Fits |
|---|---|---|
| Folder tree with ▶/▼ expand, 📄 icons, refresh ⟳ | flat sorted path list | yes — a tree listing is a producer change |
| File > Open Folder (native dialog) | `project` station lists `.` only; no dialog | yes — the shell picks a path, the core lists it |
| Open file on click / Enter | broken by blocker 1 | yes |
| New / rename / delete (prompt dialogs) | actions exist (`file.new/rename/delete`), no way to type a name with a mouse | yes |
| Save (`Ctrl+S`), dirty-free backend-authoritative edits | `Space f s` | yes |
| Per-file undo `Ctrl+Z` / redo | global `Space h u` / `h r` only | yes — the undo tree here is already a tree |
| Search in file `Ctrl+F` | missing | yes |
| Hover symbol tooltip, go-to-definition, references (LSP) | `surface/lsp.rs` exists, nothing in the window calls it | yes |
| File-tree +N/−N badges for previewed changes | missing | yes |

### Requirements, Lean and traceability

| First TraceLean | Now | Fits |
|---|---|---|
| Trace panel: tree of requirements with coverage bars, L1–L4, stale ⟳, filters (errors/warnings/unbound/stale), zoom Capabilities/Requirements/Evidence | `requirements` station: a menu of requirements with their levels | yes |
| Clause detail: assurance chain req↔model·model↔code·proof, gap text, link buttons opening file:line | `trace.evidence` record; no links you can click | yes |
| Coverage treemap, coverage trend over commits | missing | yes |
| Project graph (Requirements/Models/Code/Evidence columns, click to highlight reach) | `design` station: a menu | yes |
| Gutter chips M/I/T/D/P beside annotated lines | missing | yes — anchors are already scanned per file |
| "Spec →" / "← Req" jump between requirement and Lean model | missing | yes |
| Create requirement form; Approve (creates spec) / Link status | missing | yes |
| Lean infoview: goal at cursor, messages, pinned goal | missing (an LSP module exists) | yes — reading the server is observation |
| Save `.lean` → type-check, diagnostics list | missing | yes, as long as the check is something the user starts |
| "differential test" button | `drt.run` answers with **the command to run**, not a run | by design — `ARCH-NO-DRIVING`; offer a Copy button for it |
| Judge: "copy judge prompt" + paste reply | `drt.judge` | yes — this is exactly `ARCH-NO-DRIVING.export_not_call` |
| Judge via API (calls a model, costs money) | gone | **no** — `no_model_call` |

### Sandbox and external agents — the workflow you described

The first flow: Sandbox → **+ New sandbox session** → **▶ Terminal** (or **Copy**
the command `tracelean-sandbox shell '<root>' '<id>'`) → run `claude` inside →
AI Chat → **External Agent** shows the transcript **live**, tool calls as chips,
usage and estimated cost → **SESSION CHANGES** list with ↺ revert.

| Step | First TraceLean | Now | Fits |
|---|---|---|---|
| Create an isolated workspace | reflink copy into `.tracelean/sessions/<id>/work`, one per project | building blocks only (`observe/workspace.rs`: `write_into`, `probe_containment`, `witness`); nothing creates a session | yes — copying the tree is not driving the agent |
| Show a command to copy / open a terminal | `sandbox_shell_command`, **Copy** button, `sandbox_open_terminal` | **missing** | copy: yes (`user_runs_it`). Opening a terminal emulator for the user is borderline — `user_runs_it` says *TraceLean uninvolved in its invocation*; a Copy button is unambiguous. |
| bwrap shell over the copy (HOME blanked, `.git` live, `~/.claude` writable) | `tracelean-sandbox` binary | **missing** | yes — it is the user's shell |
| Live chat view of the agent | tails `~/.claude/projects/<slug>/*.jsonl` every 500 ms, streams `sandbox-transcript` events, renders text / thinking / tool calls / results / usage | reads `.tracelean/agent/transcript.jsonl` **once, when the station opens** — a path Claude Code never writes. It will always be empty. No refresh. | yes — `REQ-TRANSCRIPT.read_only` is exactly this. Point it at the real location and refresh it. |
| Changes come back | mirrored live into the real tree as one undo node per burst; no accept gate | `observe.start` / `accept` / `reject` / `diff`, keyboard-only, on demand | yes — the new design (review before taking in) is stricter and better; it needs buttons |
| Cost estimate, labelled as an estimate | yes | yes (`REQ-COST.estimate_is_labelled`) | yes |
| End session / destroy all | yes | n/a | yes |

The window can redraw without a key press only if something asks it to. A live
transcript needs the window to poll (or the shell to push an event) — that is a
frontend refresh, not the editor driving anything.

### Built-in AI

| First TraceLean | Now | Fits |
|---|---|---|
| AI Chat with OpenRouter / Bedrock / Mock, streaming, tool calls, spend cap, context bar, compaction, logs, model stats, chat switcher | gone | **no** — `ARCH-NO-DRIVING.no_model_call` forbids model calls and credentials outright |
| AI edits staged as hunks: ◀ ▶ ✓ accept ✗ reject, Apply, ✓✓ all, Discard | gone for the built-in model; the *same review* is what an external agent's changes need | yes — as review of observed changes |
| Test runner bar: "▶ Run tests", failures clickable to file:line, "🤖 Fix with AI" | gone | running tests is the editor starting a process — the architecture says name the command instead. "Fix with AI" → export the prompt (`export_not_call`). |
| ACP agent connection | gone | **no** — `no_launch` |

If the built-in chat is wanted back, that is a decision against
`ARCH-NO-DRIVING`, not a missing feature, and it should be made in that
document rather than worked around.

### Undo tree

| First TraceLean | Now | Fits |
|---|---|---|
| SVG tree, click a node to jump, hover to preview its diff in the editor, right-click to pin | `history.tree` record, `history.undo/redo/branch` keys | yes |
| Commit points, Save Checkpoint, command log | partial (`history` report) | yes |

## The first tree's own dead ends — not worth copying

- Editor actions `annotate`, `lake_build`, `goto_node` fired events nothing listened to.
- Project graph "show in traceability" did not open the trace panel; its file links dropped the line number.
- Built-in chat path `ai_chat_session` did not pass the shell sandbox/network policy that the default path did.
- ACP ran against a headless state cut off from the window.
- Chat sessions and sandbox sessions shared `.tracelean/sessions/`.
- The which-key bar was disabled; no right-click in the file tree.

## What to build, in order

1. **Clicks work.** `act` carries the pane; no redraw on a focus-only `mousedown`; click on text places the cursor (the core already has `offset`). — *small*
2. **Right-click menu everywhere**, from a core-produced `offered_at` menu buffer: span actions + buffer-kind actions + their keys. Same list in the terminal. — *medium, needs a REQ-ACT/REQ-VIEW clause*
3. **Ordinary editing keys in the window:** `Ctrl+S`, `Ctrl+Z`/`Ctrl+Y`, arrows/Home/End/PageUp/Down without entering a mode, mouse wheel scroll, selection + copy/paste. — *medium*
4. **Sandbox workflow:** create session (copy), show **Copy command**, a `tracelean-sandbox shell` binary, transcript read from `~/.claude/projects/<slug>/`, window refresh while a session is live, changes listed with accept/reject/diff buttons. — *large, mostly shell; the models (`REQ-SBX`, `REQ-MIRROR`, `REQ-TRANSCRIPT`) already exist*
5. **Syntax highlighting** from a core tree-sitter pass, coloured by `theme.json.syntax`; line numbers; active line. — *medium*
6. **Traceability UI:** clickable links (file:line), gutter chips, Spec ↔ Req jump, clause detail with Copy judge prompt / paste reply. — *medium*
7. **Explorer as a tree** with expand/collapse; Open Folder dialog. — *small–medium*
8. Undo-tree view with jump/preview; Lean infoview. — *larger*

Items 1–3 are what "usable" means. Item 4 is the workflow named as lost.

## Progress (2026-10-05/06)

The status columns above are from before this work. Each item below is core +
Lean/DRT where the core changed + editor + window, with tests
(`crates/editor/tests/{pointer,sandbox}.rs`, `web/test/pointer.mjs`,
`crates/tui/tests/driving.rs`).

- **Pointer and keys.** Click (or Enter outside insert mode) opens / places
  the cursor; right-click is the core's `surface::offer` list with keys (on a
  tab: shown, then its pane's list), `.` the same in the terminal.
  `Ctrl+S/Z/Y/W`; `Ctrl+F` + `F3` (from the selected text); `Ctrl+H` replaces
  all as one undo (`Editor::replace_all`; `Ctrl+R` in a terminal);
  `Ctrl+Shift+P` does any bound action by a few words (`Editor::palette`);
  `Ctrl+P` opens a file (`surface::quick`; `name:42`, `:42` go to a line;
  nothing typed lists the files just left, then the tabs);
  `Ctrl+Tab`, Home/End/Page; Left/Right walk the explorer as a tree
  (`Editor::tree_step`; Left on a top-level row closes every folder); wheel;
  middle click or `×` closes a tab. The terminal asks for find/open text on
  its last row. `F1` lists every chord and bound action with its keys (`keys`
  record, Lean `Screen.documentRecords`). Hover text is the keymap's words.
- **Editing.** The selection is the editor's (`Editor::anchor`): a drag or
  double click becomes it on release, Shift with any movement extends it,
  `Ctrl+A` takes the file, `Ctrl+L` or a click on its number the line
  (again: one more), `Ctrl+D` the
  word (again: its next occurrence), a find its match. It survives
  scrolling, reaches past the window, is drawn over the text (`ui.selection`;
  reverse video in the terminal, `draw::paint_marked`, where Shift+arrows
  select too), and is typed over, deleted, copied (`Ctrl+C/X`), pasted over
  or indented as one change. Home goes to the first character, then column 0.
  Enter keeps the indentation (a
  level deeper after `{ ( [ : := by where do`); Tab/Backspace/Shift+Tab move
  by a level (2 in Lean, else 4). `( [ {` bring their closer (before space
  or a closer), a typed closer steps over it, Enter inside a pair opens an
  indented line, Backspace removes an empty pair; `"` likewise in code (not
  after a word, not in `.md`/`.txt`). `Ctrl+Left/Right/Backspace/Delete` by
  word; `Ctrl+/` comments (`//`, `--`, `#`); `Alt+Up/Down` moves a line,
  `Shift+Alt+Down` copies it, `Ctrl+Shift+K` removes it; over a selection
  Tab, Shift+Tab and `Ctrl+/` change every line as one step
  (`Editor::over_lines`). The bracket beside the cursor and its match are
  marked (`Editor::bracket_pair`), and so is every place in the window the
  name under the cursor is written (`Editor::occurrences_shown`,
  `ui.wordHighlight`); going to a line scrolls only when it is out of sight; the status line ends with the mode
  (`NORMAL`, `INSERT`, …) and `Ln, Col`.
- **Look.** Explorer | document | side; every colour in `assets/theme.json`;
  tree-sitter syntax colours (`Role::Token`); line numbers and active line;
  tabs by title (two `main.rs` read `main.rs · tui/src` and `main.rs ·
  desktop/src`: Lean `Screen.stripTitle`, DRT-checked), the focused one
  marked (Lean `Screen.strip`), `●` when
  unsaved, `×` (or a middle click) closes; a panel's actions look like
  buttons (`ui.button`); `Ctrl+=`/`-`/`0` zoom (Tauri `zoomHotkeysEnabled`,
  the webview's own zoom, so the character grid stays exact); closing the window with unsaved work is refused once, naming the
  files (`Editor::may_close`); a welcome page of clickable starts. Markdown is marked too
  (`highlight::markdown`: headings, code, frontmatter keys, requirement names
  that open), and a line longer than its pane scrolls across (only then a
  scrollbar: the divider sat 3px past the pane and gave every pane one).
- **Files.** The explorer is a tree (`surface::explorer`,
  `REQ-SHOW.listing_is_a_tree`) titled with the project, open folders kept per
  session, the shown file highlighted, `●`/`✗` on files (and their folders)
  an agent changed, `✎` on those with unsaved edits; a row or file copies its path (`file.copy_path`, Lean
  `Watch.copyPath`). New files open; "Open another folder…" re-roots the editor,
  and the welcome page lists the folders opened before, each opening on a
  click (`surface::welcome`, `REQ-SHOW.recent_reopens`,
  `~/.config/tracelean/recent.json`, kept by the frontends only).
  Reopening a project brings back its tabs, open folders and front file
  (`editor::recall`, `.tracelean/editor.json`, names only); a tab keeps its
  cursor and scroll while another is shown. A file changed on disk by
  something else (git, another editor) is reloaded as one undoable step, or,
  when edited here too, kept with a warning (`Editor::follow_disk`, on the
  same 1.5 s tick in the window and the terminal).
- **Sandbox.** `[ New sandbox ]` copies the project to
  `.tracelean/sessions/sNNNN/work`; `[ Copy command ]` copies
  `sh …/enter.sh`, a launcher beside the copy holding the `bwrap` line (copy
  at the project's path, `.git` read-only, `$HOME` hidden but agent state).
  A 1.5 s poll lists changes with their size (`+3 −1`, counted as the diff
  shows them; click for the diff), accepts/rejects all or one
  file (`[✓]`/`[✗]`, `observe.accept_file/reject_file`), shows Claude Code's
  conversation newest first, and says on the status line when changes wait.
  The cost estimate sits under an "Agent cost" heading, in the table's unit.
- **Traceability.** A requirement opens in the document
  (`surface::requirement_view`): per clause its level, chain, text and each
  claim as a `path:line` link; `judge REQ.clause` opens the exact
  `tracelean-trace --judge` prompt (`trace.judge`), copied with "Copy all".
  `@implements REQ-X.c` in comments links to the requirement and, rested on,
  shows the clause's sentence (`Editor::requirement_text`); findings link to
  their place and requirement (`surface::findings_view`); the index shows
  `[implemented/total]`; the status line's right end counts the checker's
  findings (`⚠ 11`, `ui.warning`; click lists them), recounted between keys
  after a save (`Editor::problems`); "New requirement…" writes a draft from a template,
  and a draft's view has `[ Approve ]` (`trace.approve`, key `a`; Lean
  `approveUnder`), an undoable edit of its `status:` that Ctrl+S writes.
  The window's gutter marks each claimed declaration `M I T D P`
  (`surface::chips`, `REQ-SHOW.claims_beside_code`, from the text being
  edited, theme `chips`); a chip opens its requirement (none in the terminal).
- **Go to definition** without a language server: `F12`, Ctrl+click, `Space f g`
  or the right-click menu (`file.definition`) finds the lines that declare the
  name under the cursor (`surface::definition`: `fn`/`struct`/`def`/`theorem`/
  `function`/… then the name) and opens the one, or lists the several.
  `Ctrl+Shift+O` lists what the file declares by name and goes to one
  (`Editor::symbols`, `definition::declared_on`).
  Alt+Left/Right (and the mouse's back/forward buttons) return along the
  jumps the document made (`Editor::go_back`; another file, a list, a line).
  "Where this name is used" (`file.references`, `Space f u`) lists its
  whole-word uses; `Ctrl+Shift+F` lists every line holding some text — both as
  `path:line` links. These lists, an opened requirement and a judge's prompt
  open in the document pane (Lean `Screen.documentRecords`).
- **Speed.** The trace index is read once and kept until the editor writes the
  disk (save, accept) or the tree is checked: opening a requirement on this
  repository went from ~3 s to instant. Building the index counted lines from
  the top of the file for every comment and annotation; counting once took
  it from 2.9 s to 0.5 s (debug), so the window opens in about half a second.
  Every key replayed the whole history from the base (twice), so typing
  slowed as a session went on (62 → 122 ms a key after 200 keys); the editor
  now holds the current state (`Tree::push_from`, unit-tested equal to
  `push`). Chips are counted when a file is first drawn and between keys
  (51 ms → 15 µs a redraw); a grammar's query is compiled once. Left: a key in
  a 3,700-line file still re-highlights it whole (~60 ms debug, ~30 ms release).
- **History.** `Ctrl+Z/Y` take a typed word or a run of Backspaces at once
  (the tree still keeps each key) and leave the cursor where the change was.
  Readable rows (`history::command::describe`), indented per
  branch, the current one marked; clicking `#n` jumps there (`history.jump`,
  `Move::To`, `Tree::jump_to`).
- **Found and fixed on the way.** Undoing the first change said "nowhere to
  go"; undo/rename pulled the focus into the explorer; scrolled panes took
  window offsets as buffer offsets; the Lean `Act.deleteFocus` read a
  workspace listing a path twice by its first entry (now via `canon`). Driving
  the built page against a real editor found two more: a sandbox copied from
  disk listed unsaved edits as the agent's (it copies what the editor holds
  now), and tree-row menu labels named `main.rs` where they acted on
  `src/main.rs`. Later: `let Kind::File {..}` counted as declaring `Kind`;
  switching tabs left the explorer marking the file shown before.
- **After the first look (10-06).** Syntax colour never reached the window:
  its content policy refused the stylesheet the page wrote, so the token rules
  are static CSS over `--syntax-*` (the page test now serves the same policy).
  Markdown gets the first tree's `markup.*` colours (heading, bold, italic,
  link: new `TokenKind`s, Rust and Lean). A fifth station, 🌳 history, opens
  the undo tree (`Space w t h`). Right-click lays its actions along the bottom
  as which-key does, keys first. The demo is traced: `REQ-THERMO` claimed by
  code, tests and `specs/Thermo.lean`, and a draft `REQ-TABLE` with a gap.
- **Night of 10-06/07.** Panels in the chrome font, the shown file's row
  lit, levels as pills. A `claim` role (Rust, Lean, TS, TUI) colours each
  claim as its gutter chip; an opened requirement shows an evidence line
  (`implements 3/4 · tests 3/4 …`), `refined by`, and each claim's symbol.
  The which-key bar is fixed at the bottom from the start, showing what can
  be done at the cursor (`offers_here`). `docs/USING.md` and
  `docs/DEVELOPER_GUIDE.md`. `web/test/look.mjs` photographs the page with a
  real editor behind it (`examples/look.rs`, over stdio). The driving suite
  opens a copy of the demo, so a person's sandbox there no longer fails it.
- **Not done.** Opening a terminal for the user (ruled out by
  `ARCH-NO-DRIVING.user_runs_it`); Lean infoview and type-check on save (both
  start a process); LSP hover; coverage treemap and trend.
