# Using TraceLean

TraceLean is an editor that knows **why code exists**. Requirements live in
`reqs/*.md`; code, tests and Lean models say which requirement they serve with
one-line annotations; the editor joins the two, so from any requirement you
reach the code that implements it, and from any line of code you reach the
requirement it answers to.

## Start

```bash
cargo run -p tracelean-desktop -- demo   # the window, on the bundled demo
cargo run -p tracelean-tui -- demo       # the same editor in a terminal
```

`demo/` is a small traced project — open it first. Any directory works: the
editor reads what is there and writes only what you save.

## The window

```
┌──┬──────────────── tabs: what is open ──────────────────────┐
│📁│ explorer      │ document                  │ side panel    │
│🔗│ (files)       │ (the file you edit)       │ (trace,       │
│🧪│               │ I T M P chips in gutter   │  requirements,│
│📋│               │                           │  sandbox,     │
│🕸│               │                           │  history…)    │
│🌳│               │                           │               │
├──┴──────────── which-key: the actions here, with keys ───────┤
└───────────────────────── status line ────────────────────────┘
```

The icons on the left are **stations**, always there:

| Icon | Opens |
|---|---|
| 📁 | the project's files |
| 🔗 | the trace of the file you are reading: each claim it makes, the clause's text, and every other claim on that clause (its tests, its model, the code it tests) — it follows you from file to file. On a requirement's own document it lists each clause and everything that claims it, flagging clauses nothing claims |
| 🧪 | the sandbox: run an AI agent on a copy of the tree, then review its changes |
| 📋 | the requirements and how well each is evidenced |
| 🕸 | the refinement graph: which requirement refines which |
| 🌳 | the undo tree: every state the project has been in, each branch in its own column. Click a node to go there, rest on it to see the change it made; **All / File / Saved** at the top show everything, only the open file's changes, or only where you saved (`Space h f`) |

## Finding your way between requirements and code

- **In code**, an annotation names a requirement clause:
  ```rust
  /// @implements REQ-THERMO.to_celsius
  pub fn to_celsius(f: f64) -> f64 { … }
  ```
  The roles are `@implements`, `@tests`, `@models` (a Lean model), `@proves`
  (a Lean theorem) and `@drt` (a differential test of the code against its
  model).
- **In the file tree**, a file that claims requirements carries coloured
  letters after its name — which files implement, test, model or prove
  something is visible before you open any.
- **Chips** in the gutter mark each claimed item: **I** implements, **T**
  tests, **M** models, **P** proves, **D** drt. Point at a chip to read the
  clause; click it, or the name in the annotation, to open the requirement.
- **🔗 Trace** beside the code answers "why is this here, and what else
  would notice if it changed": for every claim in the open file, the clause
  and everything else that claims it, as links.
- **In a requirement's own file** (`reqs/*.md`), each clause line carries
  the same chips for what claims it, so a clause nothing meets stands out
  while you write it.
- **Anywhere inside an annotated item** — a function's body, say — the bar at
  the bottom offers the clause it claims: open it, or gather its context for
  an agent.
- **`F12` (or Ctrl+click) on a requirement name** in code jumps to that
  clause's line in its document.
- **Click any requirement name** (in code, prose or a panel) to open it. An
  opened requirement lists every clause with its evidence level (L1–L4) and
  every claim as a `path:line` link — click to jump there. A clause nothing
  claims says so in red.
- **📋 Requirements** lists them all with their levels and a bar of how many
  clauses something implements (`███░░ 3/5`); **🕸** shows how they refine
  each other.

Evidence levels, weakest to strongest: **L1** annotated, **L2** judged by a
person, **L3** differentially tested, **L4** proved. A clause's level is the
weakest of three bonds — requirement↔model, model↔code, model↔proof — so an
unchecked bond shows as L1 however strong the others are; the chain beside
the level (`L2/L1/L3`) says which bond holds it back.

## Keys

Everything has a mouse route (click, right-click) and a key route. You never
need to memorise: **the bar at the bottom always shows what can be done where
the cursor is, keys first** — click an entry or type its keys. Right-click
anything to see the same for the place you pointed at. **F1** lists every key.

The usual editor keys work: `Ctrl+S` save, `Ctrl+Z`/`Ctrl+Y` undo/redo,
`Ctrl+F` find (`F3` next), `Ctrl+H` replace, `Ctrl+P` open a file by name,
`Ctrl+Shift+P` any action by name, `Ctrl+Shift+F` search all files, `F12` or
Ctrl+click go to definition, `Alt+Left` back, `Ctrl+W` close tab, `Ctrl+/`
comment.

The editor is modal, like Vim: `i` to type, `Escape` to stop. Outside typing,
**Space** opens the leader menu at the bottom (Emacs which-key style):

| Keys | Does |
|---|---|
| `Space f` | files: `o` open, `s` save, `n` new, `r` rename, `d` delete, `g` definition, `u` references |
| `Space t` | trace: `o` open requirement, `n` new requirement, `a` approve draft, `c` check tree, `f` findings, `r` coverage, `s` stale |
| `Space h` | history: `u` undo, `r` redo, `t` tree, `b` branch |
| `Space c` | context for an agent: `o` open it for the requirement here, `t` include/leave out a part, `y` copy |
| `Space a` | agent: `n` new sandbox, `c` copy its command, `d` review diff, `a`/`x` accept/reject |
| `Space d` | differential testing: `r` run, `b` bindings, `c` coverage |
| `Space w` | panes: `h j k l` move focus, `v`/`s` split, `q` close, `t` stations (`p` files, `l` trace, `r` requirements, `d` graph, `s` sandbox, `h` history) |

## Working with an AI agent

**Give it the context first.** Put the cursor on any requirement name — in a
code annotation, a requirement, a panel — and choose *Context for an agent*
from the bottom bar (or `Space c o`; every opened requirement also has a
`for agent` link per clause). The context page lists what the change touches:

| Part | What it holds |
|---|---|
| requirement | the requirement, with the clause being changed marked, and what each clause still lacks (code, a test, a Lean model) |
| refines | everything it refines, transitively: what it must keep meeting |
| refined by | everything refining it: what may have to change with it (off by default) |
| code | each implementing item, with its source |
| tests | each test claiming it, with its source |
| models | Lean models and proofs claiming it |
| affected tests | tests of what refines it, and any test that calls an implementing function |

Click a part to include it or leave it out (`Space c t`), then
**Copy for an agent** (`Space c y`) and paste it to your agent. The page
shows exactly what will be copied. Nothing is sent anywhere by the editor.
An agent can gather the same text itself:
`tracelean-trace . --context REQ-X.clause [--parts code,tests,…|all]` — the
agent skills (`skills/`) tell it to.

Then let the agent work in a sandbox:

1. 🧪 (or `Space a n`) makes a sandbox: a copy of the tree the agent may change.
2. Copy the command shown and run it in your own terminal, then start your
   agent (e.g. `claude`) there. The editor never starts processes itself.
3. Its changes appear in the sandbox panel as they happen. Review each diff and
   accept or reject per file.

## Writing a requirement

`Space t n` (or right-click → New requirement) creates `reqs/REQ-NAME.md`:

```markdown
---
id: REQ-NAME
title: What it is about
status: draft
clauses:
  key: The system shall …
---
```

Approve it when it is right (`Space t a`); annotate code with
`@implements REQ-NAME.key`; the chips and the requirement panel update.

## Look and feel

Every colour and font is in `assets/theme.json`. A project may override any of
them in `.tracelean/theme.json`; the window picks the change up when it regains
focus. Changing keys, adding actions and the design behind it all:
[DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md).
