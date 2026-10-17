# How the file tree is a buffer

The file tree on the left is not a widget. It is the same kind of value as an
open file — a **buffer**, text with spans over it — and the window draws it with
the code that draws a file. This page follows it from the files on disk to the
pixels, because it is the smallest complete example of how everything in
TraceLean is built: a document, plus a little added behaviour.

Read [05-view](05-view.md) for the value itself (`Buffer`, `Span`, roles,
actions); this page is about one use of it.

## 1. From disk to rows

```
disk ──snapshot──► Workspace { files: path → text }
                        │
                        │ the paths only
                        ▼
              explorer::rows(files, open_folders) ──► [Row { depth, name, path, folder, open }]
```

- `observe::workspace::snapshot` reads the project into a `Workspace`, a map of
  relative path to text. The editor never asks the disk what is in a folder
  again; the tree is a function of that map.
- `surface::explorer::rows` turns the paths into rows. It is pure: folders
  before files at every level, each group in name order, and what is inside a
  closed folder is not produced. Which folders are open is **session state**
  (`Editor::open_folders`), not part of the workspace — closing a folder is not
  a change to the project and is not in the history.

## 2. From rows to a buffer

`explorer::tree_buffer` writes the rows as text, one row a line:

```
TRACE_CODE_IDE            ← the project's name, a heading
▾ src
  ▸ notes
    main.rs  I T          ← claim letters, in the claims' colours
▸ tests
README.md  ✎              ← a badge: unsaved edits
```

and, alongside the text, **spans**: ranges of characters with a `role` and a
list of `actions`.

| Span over… | role | actions |
|---|---|---|
| the project name | `heading` | none |
| a row's name | `path` (`heading` for the file the document shows) | `file.open` |
| a claim letter | `claim` of that kind | none |
| a badge (`●`, `+`, `✗`, `✎`) | `added`, `removed`, `entry`, … | none |

That is all a tree *is*. A row's name is a span that says "opening this opens
the row"; nothing in the text says it is a folder except the `▸`/`▾` glyph the
producer wrote.

The buffer's identity is `dir:<project>` and its kind is `directory`. The
kind is what lets the window style it like a sidebar and lets the screen decide
which pane it belongs in (`screen::destination`: a listing goes to the
explorer, a file to the document, a station to the side panel).

## 3. Why a row is not a path

The text shows a *name*; a name is not enough to open a file (`notes/a.md` and
`src/a.md` are both `a.md`). So the producer also answers which path a row
stands for: `explorer::path_at(rows, text, offset)` finds the row from the line
the position is on. The shell resolves a click or a key **through that**; nothing
works out a path from the text. The same rule keeps a tab (a number and a
title) from being mistaken for the buffer it shows.

## 4. The one added behaviour: opening a folder

Every row carries `file.open`. On a file that shows the file. On a folder the
shell (`Editor::perform`) recognises the path as a folder and calls
`toggle_folder`, which flips it in `open_folders` and produces the buffer again.
Producing it again replaces the old buffer in place — same identity, new text —
so the pane does not move.

The rest of what the tree does is the same machinery every buffer uses:

- A key press is looked up in the keymap (`assets/keymap.json`), which names an
  **action**; `act::dispatch(action, focus, workspace)` turns it, together with
  what is under the cursor, into an **intent**; the editor performs the intent.
  A click on a row's name sends the same action with the same focus.
- Which actions a position offers (`.` lists them, the context menu shows them)
  is computed from the spans there plus the buffer's own (`offer::offers`), so
  the tree adds nothing the dispatcher does not know.
- Renaming or deleting a file from the tree is a **command** on the workspace
  (`RenameFile`, `DeleteFile`) with an inverse, so it is in the undo tree like
  typing is. Marks on rows (`✎` unsaved, `●` an agent's change waiting) come
  from comparing the workspace with the disk and the agent's copy, and are
  recomputed whenever the views are refreshed (`Editor::refresh_views`).

## 5. From the buffer to the screen

The editor hands the window the buffer as JSON (the Tauri command `shown`
returns each pane's `Buffer` plus where it sits). The page (`web/src/`) does
three things, and only these:

1. `render.ts` splits the text into pieces at span boundaries.
2. `app.ts` draws each piece: a span with actions becomes a `<button>`, the
   role becomes a CSS class (`role-path`, `role-heading`), and the theme
   (`assets/theme.json`, overridden by `.tracelean/theme.json`) gives the
   classes their colours.
3. A click on a button calls back with `(pane, action, offset)`.

The terminal frontend does the same with cells and key bindings. Because both
draw the same value, `drt::frontend::conformance` can check, for either, that
the text it drew is the buffer's and that it offered exactly the actions the
spans declare.

## Where tree-sitter comes in

Not in the tree: a tree's text is generated by the producer above, so there is
nothing to parse. Tree-sitter works on the *file* buffers
(`surface::highlight`): the grammar for the file's language turns its text into
token spans (`keyword`, `function`, `string`, …) that sit in exactly the same
`spans` list, drawn the same way, coloured by the same theme. The two kinds of
buffer differ in where their spans come from — a producer for the tree,
a grammar for a file — and in nothing the frontend sees.

## What this gives you as a recipe

A new kind of panel is, in order:

1. a **pure function** from the data it shows to `(text, spans)` — write it
   first, in the core, with a requirement clause;
2. an **identity and a kind** so the screen knows where it goes;
3. **actions** on its spans, named in the keymap and known to `dispatch`;
4. a line in the shell that produces it when asked (`Editor::produce`).

Everything else — drawing, keys, the context menu, the conformance check, the
terminal version — comes for free, because it is already written against
buffers.
