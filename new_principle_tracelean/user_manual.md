# TraceLean user manual

A complete guide to using TraceLean: the terminal editor (TUI), the window
(GUI), and the command-line checker. It covers what each screen shows, every
key, what each mark and finding means, and how to take a requirement from a
sentence to proved and tested code. Worked examples use the bundled
[`demo/`](demo) project throughout, so you can follow along.

---

## Contents

1. [What TraceLean is](#1-what-tracelean-is)
2. [Words you will meet](#2-words-you-will-meet)
3. [Installing and building](#3-installing-and-building)
4. [Starting the editor](#4-starting-the-editor)
5. [The screen](#5-the-screen)
6. [Modes and the leader menu](#6-modes-and-the-leader-menu)
7. [Every key](#7-every-key)
8. [Editing files](#8-editing-files)
9. [The undo tree](#9-the-undo-tree)
10. [Panes and stations](#10-panes-and-stations)
11. [Seeing traceability in the editor](#11-seeing-traceability-in-the-editor)
12. [Writing requirements](#12-writing-requirements)
13. [Annotating code, tests and models](#13-annotating-code-tests-and-models)
14. [Evidence: earning A, J, T and P](#14-evidence-earning-a-j-t-and-p)
15. [Findings: what the checker reports](#15-findings-what-the-checker-reports)
16. [Reviewing a change](#16-reviewing-a-change)
17. [Working with an AI agent](#17-working-with-an-ai-agent)
18. [The command line](#18-the-command-line)
19. [Customising](#19-customising)
20. [Walkthroughs](#20-walkthroughs)
21. [Troubleshooting and FAQ](#21-troubleshooting-and-faq)

---

## 1. What TraceLean is

TraceLean is an editor that knows **why each piece of code exists**.

- **Requirements** live as Markdown files in `reqs/`. Each one has numbered
  promises called **clauses** ("The system shall …").
- **Code, tests and Lean models** say which clause they serve with a one-line
  comment, such as `/// @implements REQ-THERMO.to_celsius`.
- The editor joins the two. From any clause you reach the code, tests, model and
  proofs that claim it. From any line of code you reach the promise it answers
  to.
- A checker then tells you, for every clause, how far the claim is **backed by
  evidence**:
  - someone judged the formal model against the sentence;
  - generated tests found no disagreement between model and code;
  - Lean proved a property of the model.

  Each of these is earned by a run, never typed in by hand.

The goal is that a person reviewing a project, especially one written by an AI
agent, can see at a glance which promises are kept, which are only claimed, and
what changed.

```
requirement ──@models──► formal model ──@implements──► implementation
 (a clause)              (Lean function)                (code)
        └──────────@tests / @proves / @drt: the evidence ──────┘
```

Nothing is inferred: every arrow is a comment somebody wrote. A missing arrow
is shown, never hidden.

---

## 2. Words you will meet

| Word | Meaning |
|---|---|
| **Requirement** | A file in `reqs/` with an `id` such as `REQ-THERMO`, a title, and clauses. |
| **Clause** | One promise in a requirement, addressed as `REQ-THERMO.to_celsius`. |
| **Annotation** | A comment like `@implements REQ-X.clause` on the declaration right after it. It is the *only* way to link things. |
| **Model** | A Lean function (`@models`) saying exactly what a clause means. |
| **Specification** | An optional Lean predicate (`@specifies`) saying which answers are right. |
| **Binding / DRT** | Differential testing: the model and the Rust code are run on thousands of generated inputs and their answers compared. |
| **Marks `AJTP`** | The four kinds of evidence a clause can hold: **A**nnotated, **J**udged, **T**ested, **P**roved. |
| **Strength** | Whether a model's proved theorems pin it down completely: *pinned*, *attempted*, *open* or *nondeterministic*. |
| **Finding** | Something the checker reports, such as `Unbound` (model and code exist but nothing compares them). |
| **Buffer** | Anything shown: a file, a directory listing, a menu, a report. Everything on screen is a buffer. |
| **Station** | A fixed panel you can always open: files, trace, design, findings, history, sandbox, definitions. |
| **Leader** | `Space`. It opens the menu of everything you can do. |
| **Stale** | Evidence whose inputs (clause text, model, code, toolchain) changed since it was earned. It must be redone. |
| **Lock** | `.tracelean/trace.lock`, the committed index of links and evidence. |

---

## 3. Installing and building

You need:

| Tool | For | Check |
|---|---|---|
| Rust (stable) and Cargo | the editor and the checker | `cargo --version` |
| Lean 4 via `elan` (version in `formal/lean-toolchain`) | models, proofs, differential tests | `lean --version`, `lake --version` |
| Node.js | building the window's page (`web/`) | `node --version` |
| `git` | `--diff` and `--report --diff` | `git --version` |
| `bwrap` (bubblewrap) | the agent sandbox | `bwrap --version` |
| `llvm-tools` (`rustup component add llvm-tools`) | line coverage (`--coverage`) | — |

Build once, from the project root `new_principle_tracelean/`:

```bash
cargo build -p tracelean-core --bins     # tracelean-trace, the checker
cargo build -p tracelean-editor --bins   # tracelean-view, text views of the editor
node web/build.mjs                       # the window's page (only for the GUI)
cd formal && lake build && cd ..         # the Lean models (only for proofs and DRT)
```

The binaries land in `target/debug/`. Put that directory on your `PATH`, or run
them with `cargo run -p … --bin … --`.

---

## 4. Starting the editor

### The terminal editor (TUI)

```bash
cargo run -p tracelean-tui -- demo          # the bundled demo project
cargo run -p tracelean-tui                  # the current directory
cargo run -p tracelean-tui -- ~/code/mine   # any directory
```

### The window (GUI)

```bash
node web/build.mjs                          # once, and after changing web/src
cargo run -p tracelean-desktop -- demo
```

Both frontends are the **same editor**. They show the same buffers and use the
same keymap; the window adds the mouse. Anything you learn in one works in the
other.

Any directory can be opened. The editor reads what is there and writes only
what you save, plus its own state under `.tracelean/` (tabs and open folders,
so a project reopens as you left it).

### The welcome page

The document area opens on a welcome page. It explains the basics and offers
**Start here** entries, including the projects you opened recently. Move the
cursor onto one and press `.` (or click it in the window) to open it.

---

## 5. The screen

```
┌──┬─────────────── tabs: what is open in each pane ─────────────────┐
│📁│ explorer         │ document                   │ side panel       │
│🔗│ (the files)      │ (the file you edit)        │ (trace, design,  │
│🧪│ letters show     │ chips in the gutter:       │  findings,       │
│🕸│ what each file   │ I T M P D                  │  sandbox,        │
│🔍│ claims           │ coverage bar beside lines  │  history, proof) │
│⚙ │                  │                            │                  │
│🌳│                  │                            │                  │
├──┴──────────── which-key bar: what you can do here, keys first ─────┤
└──────────────────────────── status line ────────────────────────────┘
```

- **Explorer (left).** The project's files as a tree. A file that claims
  requirements carries coloured letters after its name, so you can see which
  files implement, test, model or prove something before opening any.
- **Document (middle).** The file you are reading or editing.
- **Side panel (right).** Whatever a station opens: the trace of the current
  file, the design graph, findings, the sandbox, the undo tree, or the proof
  goals of a Lean file.
- **Station icons (far left, window only).** One click opens each station; the
  terminal reaches the same stations with `Space w t …` (see
  [section 10](#10-panes-and-stations)).
- **Which-key bar.** In a menu it lists the menu's keys. Otherwise it lists
  what the cursor can do *where it is*. You never need to memorise keys: read
  the bar.
- **Status line.** What just happened, or why nothing did (for example
  `nothing happened: 'z' is not bound here`).

All of these are buffers produced by the core. A frontend only draws them, so
the terminal and the window always agree about what is there.

---

## 6. Modes and the leader menu

The editor is modal, like Vim, with an Emacs-style "which-key" menu:

| Mode | You are | Enter with | Leave with |
|---|---|---|---|
| **Normal** | moving and reading | (start here) | — |
| **Insert** | typing into a file | `i` | `Escape` |
| **Leader** | choosing an action from the menu | `Space` | `Escape`, or any unbound key |

From Normal mode:

- `Space` opens the **leader menu**; its submenus are listed below.
- `.` lists **what can be done here**: every action available at the cursor.
- `i` starts typing. On a directory listing it refuses out loud, because there
  is nothing to type into.
- Arrow keys always move the cursor, in any mode.

The leader menu:

| `Space` + | Menu | What lives there |
|---|---|---|
| `f` | **File** | open, save, new, rename, delete, find, definition, references, copy |
| `h` | **History** | undo, redo, the undo tree, branches |
| `t` | **Trace** | requirements, findings, evidence, coverage, stale, lock, judge |
| `g` | **Design** | fold and unfold the requirement graph |
| `c` | **Context** | gather what an agent needs to change a requirement |
| `d` | **Differential testing** | bindings, coverage floors, run, shrink, judge |
| `a` | **Agent** | sandbox, watch, review the diff, accept or reject |
| `w` | **Screen** | panes, splits, focus, stations |

`Escape` goes up one level towards Normal. A key that a menu does not bind
takes you back out instead of stranding you. In the terminal, `q` in Normal
mode quits and `Ctrl+C` quits from anywhere.

---

## 7. Every key

`F1` shows this list inside the editor, and `tracelean-view . keys` prints it.

### 7.1 Leader sequences

**File: `Space f …`**

| Key | Action |
|---|---|
| `o` | open the file or folder under the cursor |
| `n` | new file |
| `s` | save |
| `r` | rename |
| `d` | delete |
| `f` | find: go to the next match (`*` and `?` are wildcards) |
| `/` | find: list every line that matches |
| `g` | go to the definition of the name under the cursor |
| `u` | where this name is used (references) |
| `c` | copy the whole buffer |
| `l` | copy this line |
| `p` | copy the file's path |
| `e` | edit the setting under the cursor (in the Definitions station) |

**History: `Space h …`**

| Key | Action |
|---|---|
| `u` | undo |
| `r` | redo |
| `t` | show the undo tree |
| `b` | switch branch |
| `j` | go to the point under the cursor (in the tree) |
| `f` | show all changes, only this file's, or only the saved points |

**Trace: `Space t …`**

| Key | Action |
|---|---|
| `o` | open the requirement under the cursor |
| `n` | new requirement |
| `a` | approve this draft requirement |
| `c` | check the tree (the same report as `tracelean-trace .`) |
| `f` | list findings |
| `e` | evidence for the item under the cursor |
| `v` | which lines of this requirement's code the tests run |
| `r` | coverage roll-up |
| `s` | what went stale |
| `l` | write the lock file |
| `j` | the judge's prompt for this clause |

**Design: `Space g …`** (in the design graph)

| Key | Action |
|---|---|
| `o` | unfold or fold the requirement here |
| `a` | unfold everything under the requirement here |
| `e` | unfold the whole design |
| `f` | fold the design back to its roots |
| `u` | show only what is still to do, or everything |

**Context: `Space c …`**

| Key | Action |
|---|---|
| `o` | open the context an agent needs to change the requirement here |
| `t` | include this part or leave it out |
| `y` | copy the context to the clipboard |

**Differential testing: `Space d …`**

| Key | Action |
|---|---|
| `b` | list the bindings |
| `c` | coverage floors |
| `r` | run the bindings |
| `s` | shrink the last divergence |
| `j` | judge a divergence |

`r`, `s` and `c` show you the command to run rather than running it: the editor
never starts processes itself.

**Agent: `Space a …`**

| Key | Action |
|---|---|
| `n` | new sandbox |
| `c` | copy the sandbox command |
| `s` | watch the sandbox |
| `d` | review the diff |
| `f` | accept this file |
| `r` | reject this file |
| `a` | accept the whole change |
| `x` | reject the whole change |
| `e` | end the sandbox |

**Screen: `Space w …`**

| Key | Action |
|---|---|
| `h` `j` `k` `l` | move focus left, down, up, right |
| `v` | split across |
| `s` | split downwards |
| `+` / `-` | grow / shrink this pane |
| `o` | show the buffer here |
| `q` | close this buffer |
| `b` | list what is opened (the strip) |
| `t` | stations (next table) |

**Stations: `Space w t …`**

| Key | Opens |
|---|---|
| `p` | open a project |
| `l` | trace: what this file claims, and what else claims it |
| `r` or `d` | the requirements, as the refinement graph |
| `f` | findings |
| `h` | the undo tree |
| `s` | the sandbox |
| `c` | definitions: what can be configured |

### 7.2 Direct keys (chords)

These work in both frontends unless marked.

| Keys | Does |
|---|---|
| `Ctrl+S` | save |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo, a word at a time |
| `Ctrl+W` | close the tab |
| `Ctrl+Tab` | next tab of the pane |
| `Ctrl+P` | open a file by a few letters; `name:42` or `:42` jumps to a line |
| `Ctrl+Shift+P` | do anything, by a few words of its name (window only) |
| `Ctrl+Shift+O` | go to a name this file declares (window only) |
| `Ctrl+F`, `F3` | find in the buffer, then find again (`Shift+F3` goes backwards) |
| `Ctrl+Shift+F` | find in every file |
| `Ctrl+H` (`Ctrl+R` in the terminal) | replace every match in the file, as one undo step |
| `F12`, `Ctrl+click` | go to the definition |
| `Alt+Left` / `Alt+Right` | back / forward to where a jump left from |
| `Shift+arrows`, drag | select |
| `Ctrl+A` / `Ctrl+C` / `Ctrl+X` | select all / copy / cut (in the terminal, `Ctrl+C` quits; copy with `Space f c` or `Space f l`) |
| `Ctrl+L` | select the line; again selects the next one too |
| `Ctrl+D` | select the word; again selects its next occurrence |
| `Home` / `End` | to the first character, then the line's start / to the end |
| `PageUp` / `PageDown` | a screen up / down |
| `Ctrl+/` | comment the line or the selection |
| `Tab` / `Shift+Tab` | indent / dedent the line or the selection |
| `Alt+Up` / `Alt+Down` | move the line |
| `Shift+Alt+Down` | copy the line |
| `Ctrl+Shift+K` | delete the line |
| `Ctrl+Left` / `Ctrl+Right` | move by word |
| `Ctrl+Backspace` / `Ctrl+Delete` | delete a word backwards / forwards |
| `Left` / `Right` on the explorer | close / open a folder; `Left` at the top closes every folder |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | zoom in / out / back (window only) |
| `.`, right-click | what can be done here |
| `F1` | the list of keys |

### 7.3 The prompt row (terminal)

`Ctrl+F`, `Ctrl+Shift+F`, `Ctrl+P` and `Ctrl+R` open a one-line prompt at the
bottom:

- type the text, `Backspace` to correct it, `Enter` to accept, `Escape` to
  cancel;
- in a find prompt, `Tab` switches between *this file* and *every file*;
- in a replace prompt, the first `Enter` takes what to find and the second
  `Enter` takes what to put instead.

### 7.4 The mouse (window)

- **Click** a file to open it; click a folder to open or close it.
- **Click into** a file to put the cursor there and type.
- **Right-click anything** for every action available there, with the keys
  that reach it.
- **Click** a button row (an action carried by a row, such as `[ expand all ]`).
- **Point** at a chip or a coverage mark to read what it means.
- **Ctrl+click** a name to go to its definition.
- The wheel scrolls; drag selects.

### 7.5 Unbound keys are never swallowed

Every key has an outcome. At the root you are told
`nothing happened: 'z' is not bound here`. Inside a menu an unbound key takes
you back out. The bar can only list keys that do something, because it is
computed from the same keymap that dispatches them.

---

## 8. Editing files

| Task | How |
|---|---|
| Open a file | Arrow onto it in the explorer, `Space f o` (or click; or `Ctrl+P` and a few letters) |
| Type | `i`, type, `Escape` |
| Save | `Ctrl+S` or `Space f s` |
| New file | `Space f n`, then type its path |
| Rename / delete | `Space f r` / `Space f d` |
| Find | `Ctrl+F` (next match), `Space f /` (list every matching line), `Ctrl+Shift+F` (every file) |
| Replace all | `Ctrl+H` in the window, `Ctrl+R` in the terminal |
| Go to a definition | `F12`, `Ctrl+click` or `Space f g`; come back with `Alt+Left` |
| Find the uses of a name | `Space f u` |
| Copy | select with `Shift+arrows`, then `Ctrl+C` (window); or `Space f c` / `Space f l` / `Space f p` |

Definitions and uses are found by reading the files with each language's
grammar; no language server is needed. Syntax colouring comes from the core's
parser (tree-sitter), so what is coloured is what the grammar actually found.

Tabs and open folders are remembered per project and come back the next time
you open it.

---

## 9. The undo tree

History is a **tree**, not a stack. If you undo and then type something else,
the branch you left is kept, not lost.

- `Ctrl+Z` / `Ctrl+Y` (or `Space h u` / `Space h r`) step back and forward.
- `Space h t` (or the 🌳 station) shows the tree. Each branch is drawn in its
  own column; `#0` is the project as it was opened.
- In the tree, put the cursor on a node and press `Space h j` (or click it) to
  go there. Resting on a node shows the change it made.
- `Space h b` switches between branches.
- `Space h f` (or **All / File / Saved** at the top of the tree) shows every
  change, only the open file's changes, or only the points where you saved.

Every change is a command that carries what it destroyed, so any undo can be
replayed exactly.

---

## 10. Panes and stations

The screen starts as a **workbench** with three places: files on the left, the
document in the middle and the side panel on the right. Every buffer has a
*home*: files open in the middle and stations open on the right. When its
home is gone (you closed that pane), a buffer opens where the focus is.

- Split the focused pane with `Space w v` (across) or `Space w s` (down).
- Move the focus with `Space w h/j/k/l`, resize it with `Space w +` / `-`.
- `Space w q` closes the buffer; the pane collapses if it was the last one.
- `Space w b` lists everything opened. A buffer that is opened but not shown is
  kept, and showing it again does not recreate it.

### Stations

| Station | Icon | Keys | Shows |
|---|---|---|---|
| Project | 📁 | `Space w t p` | the project's files; open another project |
| Trace | 🔗 | `Space w t l` | for every claim in the open file: the clause's text and every other claim on that clause. On a requirement file: each clause and everything that claims it |
| Sandbox | 🧪 | `Space w t s` | an AI agent's sandbox and its changes ([section 17](#17-working-with-an-ai-agent)) |
| Design | 🕸 | `Space w t r` | the requirements as their refinement graph |
| Findings | 🔍 | `Space w t f` | what the checker found, each with what to create |
| Definitions | ⚙ | `Space w t c` | every setting, colour by colour, editable in place |
| Undo tree | 🌳 | `Space w t h` or `Space h t` | the history tree |
| Proof | ∴ | opens by itself with a `.lean` file | the goals Lean reports at the cursor, following the cursor and your edits |

---

## 11. Seeing traceability in the editor

### 11.1 In the file tree

Files carry coloured letters for what they claim (implements, tests, models,
proves), so the shape of the project's evidence shows before you open anything.

### 11.2 Chips in the gutter

Each annotated declaration has a chip in the gutter:

| Chip | The item |
|---|---|
| **I** | implements a clause |
| **T** | tests a clause |
| **M** | models a clause (Lean) |
| **P** | proves a property of a model (Lean) |
| **D** | is a differential comparison of model and code |

Point at a chip to read the clause; click it (or the name in the annotation) to
open the requirement. In a requirement file, each clause line carries chips for
what claims it, so a clause that nothing claims stands out while you write it.

Anywhere inside an annotated item, the bar at the bottom offers the clause it
claims: open it, or gather its context for an agent.

### 11.3 The trace station 🔗

For the open file, it answers *why is this here, and what else would notice if
it changed*. It lists each claim the file makes, the clause's text, and every
other claim on the same clause (its tests, its model, the code those tests
exercise), all as links. It follows you from file to file.

### 11.4 An opened requirement

Put the cursor on a requirement name (in code, prose or a panel) and press
`Space t o`, or click it. The requirement opens with every clause and its
evidence. On the demo:

```
REQ-THERMO
Converting temperatures
reqs/REQ-THERMO.md
status approved
refined by REQ-TABLE

for agent  REQ-THERMO  context to copy
evidence   implements 3/4 proves 4/4 models 2/4 tests 3/4 strong 0/4 drt 2/4
covered    REQ-THERMO  9/9 lines run (100%), by 2 tests

A---  agreement
  The two scales shall agree at minus forty.
  for agent   REQ-THERMO.agreement
  covered     REQ-THERMO.agreement  3/3 lines run (100%), by 1 test
  implements src/celsius.rs:32  agreement
  proves     specs/Thermo.lean:62  Thermo::agree_at_minus_forty
  tests      tests/round_trip.rs:12  the_scales_meet_at_minus_forty

A-T-  to_celsius
  Converting whole degrees Fahrenheit shall subtract thirty-two, multiply by five ninths and round up.
  …
  attempted
    Thermo::to_celsius_pinned: changed since it was checked: tracelean-trace . --proofs
```

- The four letters in front of each clause are its **marks** (next section).
- Every claim is a `path:line` link; follow it with `.` or a click.
- `covered` gives how many lines of the clause's implementing code the tests
  run, and by how many tests.
- The strength line (`pinned` / `attempted` / `open`) says whether the model's
  proofs pin it down, and if not, what is owed.
- A clause that nothing claims says so in red.

The same view is printed by `tracelean-view demo requirement REQ-THERMO`.

### 11.5 The marks `AJTP`

Four marks, in fixed positions, with a dash for each one missing:

| Mark | Held when |
|---|---|
| **A** annotated | something in the tree claims the clause |
| **J** judged | a person judged the Lean model to say what the clause says, and neither changed since |
| **T** tested | differential testing of model against code agreed over a stated number of cases, reaching every situation the binding declared, and nothing changed since |
| **P** proved | Lean accepted a theorem about the clause's model that is honest (see [14.3](#143-p-proved-honest-theorems)) |

- `AJTP`: fully backed.
- `A-TP`: tested and proved, but nobody has judged the model against the
  sentence.
- `A---`: only claimed.
- `AJ··`: a *structural* clause, one about the tree rather than about a value
  ("only declared modules touch the filesystem"). It is answered by a test
  that reads the tree; T and P do not apply and read `·`.

Marks are independent: none implies another. A requirement holds a mark only
when **every** clause holds it, never on average.

### 11.6 The design graph 🕸

`Space w t r` opens the requirements as their refinement graph, starting folded
at the roots:

```
[ expand all ]  [ fold all ]  [ open only ]
▾ A---  REQ-THERMO  I3/4 A4/4 J0/4 T2/4 P0/4  lines 100%  +children 81%  Converting temperatures
  · ----  REQ-TABLE  I1/2 A1/2 J0/2 T0/2 P0/2  lines 57%  Printing a table of temperatures
```

Each row reads:

- the requirement's marks;
- how many of its clauses are implemented (`I`) and hold each mark;
- `lines`: the share of its code's lines that tests run;
- `+children`: the same share over it and every requirement refining it.

`▸`/`▾` (or `Space g o`) unfolds or folds one level. `Space g a` unfolds
everything under the cursor, and **[ open only ]** (`Space g u`) hides every
requirement with nothing left to do.

### 11.7 Findings 🔍

`Space w t f` (or `Space t f`) lists every finding. Each message says what to
create, for example:

```
unbound: reqs/REQ-THERMO.md:1 REQ-THERMO.to_celsius: missing a DRT binding — bind the model and the implementation so generated inputs are compared
unmodeled: reqs/REQ-TABLE.md:1 REQ-TABLE.boiling: missing a model — write a Lean function and mark it `@models REQ-TABLE.boiling`
untested: reqs/REQ-THERMO.md:1 REQ-THERMO.to_fahrenheit: missing a test — write one and mark it `@tests REQ-THERMO.to_fahrenheit`
```

All finding kinds are explained in [section 15](#15-findings-what-the-checker-reports).

### 11.8 Coverage in the gutter

After `tracelean-trace . --coverage` (see [18](#18-the-command-line)), an open
file shows a green bar beside lines that tests ran and a red, tinted line where
none did. Point at a line to read, for example,
`run 7 times by converting_back_gives_what_went_in ×6, …`. Editing a file hides
its coverage until it is measured again, so the gutter never shows stale
numbers. `Space t v` lists, for the requirement under the cursor, which of its
lines the tests run.

### 11.9 Proof goals ∴

Opening a `.lean` file opens the proof panel. It shows the goals Lean reports
at the cursor and follows the cursor and your edits. Closing the last Lean file
closes it.

---

## 12. Writing requirements

### 12.1 Create one

`Space t n` (or right-click → *New requirement*) creates
`reqs/REQ-NAME.md`:

```markdown
---
id: REQ-NAME
title: What it is about
status: draft
clauses:
  key: The system shall …
---

# What it is about

Why the clauses are what they are.
```

When it is right, approve it with `Space t a`. Then annotate the code that
implements it with `@implements REQ-NAME.key`; the chips and the requirement
panel update at once.

### 12.2 The frontmatter

| Field | Meaning |
|---|---|
| `id` | The identity. Annotations use it; the file may move or be renamed freely. |
| `title` | A short name. |
| `status` | `draft` or `approved`. |
| `refines` | The requirements this one refines, e.g. `[REQ-THERMO]`. Cycles and unknown ids are findings. |
| `decomposition` | `complete` if you claim the clauses exhaust the requirement; `open` otherwise. An open requirement is never shown as finished. |
| `clauses` | `key: sentence`, one promise each. |

### 12.3 Good clauses

Write each clause in **EARS** form (ADR-0017): one sentence, one obligation,
"shall", testable.

| Pattern | Form | Example |
|---|---|---|
| Ubiquitous | The system shall … | The system shall serialise the same repository state to identical bytes. |
| Event | When …, the system shall … | When a theorem is refused, the system shall report it as a finding naming the theorem. |
| Unwanted | If …, then the system shall … | If no keymap names an action, then the system shall resolve it to a refusal. |
| State | While …, the system shall … | While a sandbox runs, the system shall show its changes as they happen. |
| Optional | Where …, the system shall … | Where a model cannot be pinned, the system shall let it be declared as such with a reason. |

The prose body says *why*. Explanations belong in the requirement file, not in
code comments.

### 12.4 Narrowings

A clause can carry narrowings, more specific cases that are judged and hashed
with the clause but are not clauses themselves:

```yaml
clauses:
  min_not_mean:
    text: The aggregate shall be the minimum of its parts' levels.
    empty: With no parts, the aggregate shall be L1.
```

### 12.5 Changing and deleting

- **Rewording** a clause (or its narrowings) makes *that clause's* evidence
  stale, not its siblings'. Never reword a clause to fit the code.
- **Before changing** anything, see what the change touches:
  `tracelean-trace . --context REQ-X --parts all`, or `Space c o` in the editor.
- **Deleting** a clause: search for its id first and retarget or delete every
  annotation, or they become `Dangling`.

### 12.6 Documents that describe requirements

A prose document can declare the requirements it describes, with the hash it
was last checked against:

```markdown
---
describes: [REQ-DRIVE]
described_hash:
  REQ-DRIVE: d79ca23ad57cf379
---
```

When the requirement changes, the document goes *into review*. That is shown,
and is not an error. Print the current hashes with `tracelean-trace . --hashes`.

---

## 13. Annotating code, tests and models

Annotations are recognised only inside comments and attach to the **nearest
following declaration**, so put them in that item's doc comment.

| Role | On | Says |
|---|---|---|
| `@implements` | code | this realises the clause |
| `@tests` | a test | this exercises the clause |
| `@models` | one Lean function | this is what the clause means |
| `@specifies` | a Lean `def … : Prop` beside the model | which answers are right |
| `@proves` | a Lean theorem about the model | a property of the model |
| `@pins` | a Lean theorem stating the model's obligation | the model is pinned |
| `@drt` | a differential test, and the Rust function it calls | model and code compared |

At most one `@models`, one `@specifies` and one `@pins` per clause.

### 13.1 Rust

```rust
/// Fahrenheit to Celsius, rounded up.
///
/// @implements REQ-THERMO.to_celsius
pub fn to_celsius(degrees: i64) -> i64 {
    -((32 - degrees) * 5).div_euclid(9)
}

/// @tests REQ-THERMO.round_trip
/// @tests REQ-THERMO.to_celsius
#[test]
fn converting_back_gives_what_went_in() {
    for degrees in [-40, -1, 0, 1, 37, 100] {
        assert_eq!(to_celsius(to_fahrenheit(degrees)), degrees);
    }
}
```

### 13.2 Lean

```lean
/-- `c` is `f` degrees Fahrenheit in Celsius, rounded up.

@specifies REQ-THERMO.to_celsius -/
def ToCelsius (f c : Int) : Prop :=
  9 * c - 9 < 5 * (f - 32) ∧ 5 * (f - 32) ≤ 9 * c

/-- @models REQ-THERMO.to_celsius -/
def toCelsius (f : Int) : Int := -((32 - f) * 5 / 9)

/-- @proves REQ-THERMO.to_celsius -/
theorem to_celsius_meets_its_specification (f : Int) : ToCelsius f (toCelsius f) := by
  unfold ToCelsius toCelsius
  omega
```

### 13.3 Qualifiers

| Qualifier | Needs | Means |
|---|---|---|
| `@partial` | `reason="…"` | covers only part of the clause |
| `@nondeterministic` | `reason="…"` | the model cannot be pinned (excluded from strength) |
| `@structural` | `reason="…"` | the clause is about the tree, not a value; needs only `@tests` |
| `@exempt` | `reason="…"`, `by="…"`, optional `until="YYYY-MM-DD"` | out of the count, with a named person's approval |

Example:

```rust
/// @tests ARCH-CORE-SHELL.shell_thin
/// @structural ARCH-CORE-SHELL.shell_thin reason="a property of how the modules are divided"
#[test]
fn only_declared_shells_touch_the_filesystem() { … }
```

Never use `@structural` or `@exempt` to silence a finding. A qualifier without
its reason is itself a finding (`UnsoundQualifier`, `UnsoundExemption`).

### 13.4 Axioms that stand for the outside world

Where something has no Lean definition (a filesystem copy, an external tool),
declare it as an axiom and say so:

```lean
/-- @assumes REQ-SBX.copy_is_faithful -/
axiom copy_is_faithful : ∀ w, copy w = w
```

A proof that rests on it is still accepted, and every `P` it earns lists the
assumption, so a reviewer sees it.

### 13.5 When a file does not parse

If the grammar cannot read part of a file, annotations after that point anchor
to the whole file and are capped (`Imprecise`).
`tracelean-trace . --unparsed path/to/File.lean` shows where it stopped.
Rewrite that declaration more plainly, without changing its meaning.

---

## 14. Evidence: earning A, J, T and P

Evidence is **earned by a run and recorded**, never written by hand. Each
record lists what it rests on: the clause text, the model, the code and the
toolchain. Changing any of them makes the record stale. `--lock` folds the
records into `.tracelean/trace.lock`, which you commit.

### 14.1 J: judged

A person reads the clause and its Lean model and decides whether the model says
what the sentence says.

1. In the editor, put the cursor on the clause and press `Space t j` to see the
   prompt. On the command line: `tracelean-trace . --judge REQ-THERMO.to_celsius`.
2. Record the verdict:

   ```bash
   tracelean-trace . --judge REQ-THERMO.to_celsius \
       --verdict agrees --by "Ana Silva" --note "rounds up, as the clause says"
   ```

   The verdicts are `agrees`, `drift` (say on which behaviour they differ) and
   `unmodelable`. `--delegated-by` records who asked an agent to judge on their
   behalf; the record always says who decided.

3. **Who may judge** is listed in `.tracelean/judges.json`:

   ```json
   {
     "people": [
       { "name": "ana", "delegatesTo": ["claude-review"] }
     ]
   }
   ```

   `--by` must name a listed person, or a delegate of one, given together with
   `--delegated-by <that person>`:

   ```bash
   tracelean-trace . --judge REQ-THERMO.to_celsius --verdict agrees \
       --by claude-review --delegated-by ana --note "…"
   ```

   Any other name is refused. Without a `judges.json`, the verdict is recorded
   against the name given, and the checker says so.

### 14.2 T: tested

Differential testing generates thousands of inputs, runs both the Lean model and
the Rust code, and compares the answers.

- `tracelean-trace . --drt` finds every clause that has a Lean `def` modelling
  it and a Rust `fn` implementing it (`@drt` or `@implements`). It pairs their
  arguments by type (`Int`/`i64`, `Nat`/`u32`, `Bool`, `String`, `List`/`Vec`,
  `Option`, and structures with matching fields), builds both runners and
  compares them.
- Agreement that reached every class of every argument (negative, zero and
  positive; empty and not; …) earns **T**.
- A disagreement is shrunk to a small input and printed with both answers.
- A pair already agreed against the same model, code and clause is `cached`;
  `--again` re-runs it.
- `.tracelean/drt.json` holds what the annotations cannot say: coverage
  floors ("at least 100 cases reached *a record missing a field*"), waivers
  with reasons, and one binding checking several clauses (`also_checks`).
- A binding only counts for a clause if its model computes that clause's own
  model; a test of some other function earns nothing.

### 14.3 P: proved (honest theorems)

`tracelean-trace . --proofs` asks Lean about every `@proves` theorem. A theorem
earns **P** only when it really says something about the clause's model. It is
**refused, with the reason**, when it:

| Refusal | Example |
|---|---|
| never applies the model | proves `n + n = 2 * n` but never mentions `double` |
| applies it only to fixed values | `double 2 = 4` is an example, not a proof |
| reaches the model through another definition | a theorem about `quad`, which calls `double` |
| holds of every function in place of the model | `n = m → double n = double m` |
| depends on `sorry` | an unfinished proof |
| uses `native_decide` (`Lean.ofReduceBool`) | trusts the compiler, not the kernel |
| depends on an undeclared axiom | an axiom without `@assumes` |
| has a type, not a function, as its model | `@models` on a `structure` |

Each refusal shows as a `ProofRefused` finding, and on the requirement view and
the report, with what to do about it.

### 14.4 Strength: does the specification pin the model?

A proof says the model *has* a property, not that the property leaves only one
answer: `discount s ≤ s` is also true of the function that always returns 0.
For each model, the checker writes the **obligation** from its accepted
theorems, with the model replaced by an arbitrary function `g`:

```
∀ g, S₁ g → S₂ g → … → Sₙ g → g = model
```

Prove it in a theorem annotated `@pins`, stated exactly as printed:

```lean
/-- @pins REQ-THERMO.to_fahrenheit -/
theorem to_fahrenheit_pinned :
    ∀ g : Int → Int, (∀ c, ToFahrenheit c (g c)) → g = toFahrenheit := by
  intro g h
  funext c
  have a := h c
  have b := to_fahrenheit_meets_its_specification c
  unfold ToFahrenheit at a b
  omega
```

| Strength | Means |
|---|---|
| **pinned** | Lean accepted a `@pins` theorem stating the obligation, and nothing changed since |
| **attempted** | a `@pins` theorem exists but was not accepted, or something changed since (the reason is shown) |
| **open** | no pinning theorem yet; the obligation it owes is shown |
| **nondeterministic** | declared with `@nondeterministic reason="…"`; excluded |

`tracelean-trace . --pins` lists every model's strength, and
`tracelean-trace . --strength toCelsius` (a model's name or a clause, matched
by substring) prints the obligation to prove for that model.

### 14.5 Coverage

```bash
tracelean-trace . --coverage
```

Builds the Rust tests with coverage instrumentation, runs each test alone and
records which tests ran which lines. It needs `llvm-tools`. The editor then
shows the coverage gutter ([11.8](#118-coverage-in-the-gutter)) and per-clause
line counts.

### 14.6 Staleness and the lock

```bash
tracelean-trace . --stale     # what a change re-opened, with the command to redo each; exit 1 if any
tracelean-trace . --lock      # fold the evidence into .tracelean/trace.lock
```

In the editor: `Space t s` shows what went stale and `Space t l` writes the lock.
Run `--stale` after every change; an agent can run it too, because it exits 1
while anything is left to redo.

---

## 15. Findings: what the checker reports

| Finding | Means | Blocks by default |
|---|---|---|
| `Unmodeled` | a clause has no Lean model yet (information, not a fault) | no |
| `Unimplemented` | modelled, but nothing implements it | no |
| `Unbound` | model and code both exist and **nothing compares them**; the most important finding | no |
| `Untested` | implemented, but no test | no |
| `Imprecise` | an annotation sits after a region the grammar could not parse | no |
| `UnsoundQualifier` | `@partial`, `@structural` or `@nondeterministic` without a reason | no |
| `ProofRefused` | Lean refused a `@proves` theorem; the reason says what to change | no |
| `Malformed` | a requirement document cannot be read | **yes** |
| `Dangling` | an annotation names a requirement or clause that does not exist | **yes** |
| `DanglingRefines` | `refines` names a requirement that does not exist | **yes** |
| `RefinesCycle` | requirements refine each other in a cycle | **yes** |
| `DuplicateId` | two documents share an id | **yes** |
| `Contested` | two exclusive claims on one clause | **yes** |
| `UnsoundExemption` | `@exempt` without reason, approver, or past its date | **yes** |
| `SeveralModels` / `SeveralSpecs` / `SeveralPins` | more than one model, specification or pin for one clause | **yes** |

`tracelean-trace .` ends with `blocking: true/false`, so CI can gate on it.
`tracelean-trace . --show Unbound` lists one kind in full.

---

## 16. Reviewing a change

These two commands are made for a human reviewing what someone, often an agent,
changed.

### 16.1 The evidence diff

```bash
tracelean-trace . --diff main          # or HEAD~1, a tag, any revision
tracelean-trace . --diff main --json   # the same, as data
```

It compares the lock, the requirement files and the proof verdicts at that
revision (read through `git show`, with no checkout) against now, **clause by
clause**:

```
== against 7e2e360: 76 clauses changed, 11 with a loss ==
REQ-ACT.dispatch_is_pure
    LOST P
    @proves removed: formal/TraceLean/Act.lean::TraceLean.Act::dispatch_reads_only_its_arguments
REQ-STRENGTH.kernel_decides
    reworded
      was: The system shall report a clause pinned only when Lean accepted …
      now: The system shall decide every acceptance by running Lean on the built models …
    LOST J
    …
```

- Clauses that were removed or **lost a mark come first**, so the risky part of
  a change is never buried.
- It shows reworded text before and after, marks lost and gained, annotations
  added or removed, code that changed under an annotation, and axioms a proof
  newly relies on (`NOW ASSUMES …`).
- A clause the change did not touch is not listed.

### 16.2 The report: a card per clause

```bash
tracelean-trace . --report target/report                # the current state
tracelean-trace . --report target/report --diff main    # and what changed since main
```

This writes `target/report/index.html`, one static page that opens from the
filesystem with no server or network, so you can attach it to a CI run or an
email. It has:

- **a summary**: clauses, how many hold each mark, how many changed or lost a
  mark, and the number of findings;
- **filters**: search, *not judged*, *not tested*, *not proved*, *with
  findings*, *changed*, *lost a mark*;
- **one card per clause**, grouped by requirement, holding:
  - the clause text and its four marks (hover over a missing one to see why);
  - **Not held**: for each missing mark, the first step that is missing ("no
    Lean model says what it means", "model and code exist, but nothing compares
    them", "Lean refused every theorem: …");
  - **Changed since …**: the clause's changes, when made with `--diff`;
  - **Findings** about the clause;
  - **Evidence** (folded): the model with its source, the code implementing it,
    what compares them, its tests, each theorem with Lean's verdict, the axioms
    it assumes and its statement, the strength with the obligation, and the
    judgement with who made it and their note;
- **a Removed section** for clauses that no longer exist.

A typical review: run `--report … --diff main`, tick **lost a mark**, then
**changed**, and read those cards.

---

## 17. Working with an AI agent

The editor never starts an agent or calls a model. You run the agent; the
editor gives it a safe place and the right context, and shows you what it did.

### 17.1 The sandbox

1. Press 🧪 (or `Space a n`) to make a sandbox: a copy of the project that the
   agent may change.
2. Copy the command shown (`Space a c`) and run it in your own terminal. It
   starts a shell under `bwrap`: the copy appears at the project's own path,
   your real tree and home stay read-only, and only the tool directories an
   agent needs (`.claude`, `.cargo`, `.elan`, …) are writable.
3. Start your agent (for example `claude`) in that shell. `tracelean-trace` is
   on its `PATH`, and `TRACELEAN_SKILLS` points at the methodology pages the
   agent should read. A `CLAUDE.md` in the project (see `demo/CLAUDE.md`) tells
   it to run `tracelean-trace . --context REQ-X.clause` before changing
   anything and `tracelean-trace .` when done.
4. Watch (`Space a s`): its changes appear in the sandbox panel as they happen.
5. Review (`Space a d`) the diff file by file. Accept or reject each file
   (`Space a f` / `Space a r`) or the whole change (`Space a a` / `Space a x`).
6. End the sandbox (`Space a e`).

Writes outside the copy, and protected paths, are reported as violations, not
silently allowed.

### 17.2 Handing over context yourself

For an agent outside the sandbox, put the cursor on a requirement name and
choose *Context for an agent* (`Space c o`). The page lists what the change
touches:

| Part | Holds |
|---|---|
| requirement | the requirement, with the clause being changed marked, and what each clause still lacks |
| refines | everything it refines, transitively: what it must keep meeting |
| refined by | everything refining it: what may have to change with it (off by default) |
| code | each implementing item, with its source |
| tests | each test claiming it, with its source |
| models | the Lean models and proofs claiming it |
| affected tests | tests of what refines it, and any test calling an implementing function |

Toggle a part with `Space c t`, then copy everything with `Space c y` and paste
it to your agent. The page shows exactly what will be copied, and nothing is
sent anywhere.

The same from the shell:

```bash
tracelean-trace . --context REQ-THERMO.to_celsius     # one clause
tracelean-trace . --context REQ-THERMO --parts all    # the whole requirement, every part
```

### 17.3 After the agent

```bash
tracelean-trace . --stale                          # what its change re-opened
tracelean-trace . --diff HEAD                      # what it did to the evidence
tracelean-trace . --report target/review --diff HEAD
```

---

## 18. The command line

### 18.1 `tracelean-trace`: the checker

Every command takes the project root first (`.`).

| Command | Does | Writes |
|---|---|---|
| `tracelean-trace .` | requirements, links, problems, documents and finding counts; last line `blocking: true/false` | — |
| `--show <Kind>` | one finding kind in full | — |
| `--context X [--parts …]` | everything needed to change a clause or requirement, as Markdown | — |
| `--stale` | what to redo, with the command for each; exit 1 if any | — |
| `--lock` | fold evidence into `trace.lock`, naming what went stale | lock |
| `--judge X` | print the judging prompt | — |
| `--judge X --verdict agrees\|drift\|unmodelable --by <who> [--delegated-by <who>] [--note …]` | record a judgement | evidence, lock |
| `--drt [--again]` | run every derivable differential test; record T where earned | evidence, lock |
| `--drt-due` | which test suites have bindings that need re-running | — |
| `--proofs` | ask Lean about every `@proves` and `@pins`; record P; list refusals | `proofs.json`, evidence |
| `--pins` / `--strength [model]` | each model's strength; the obligation for one model | — |
| `--coverage [--again]` | run each Rust test alone under coverage | coverage |
| `--diff <revision> [--json]` | what a change did to the evidence, clause by clause | — |
| `--report <dir> [--diff <revision>]` | the static HTML report, a card per clause | `<dir>/index.html` |
| `--hashes` | each requirement's current hash, for `described_hash` | — |
| `--unparsed <file>` | where the grammar stopped reading a file | — |
| `--today YYYY-MM-DD` | the date used to judge `@exempt until=` | — |

### 18.2 `tracelean-view`: the editor's views as text

What a station would show, printed. Useful in scripts, over SSH, or for an
agent:

```bash
tracelean-view . requirements                 # every requirement, its level, how many clauses are claimed
tracelean-view . design                       # the same, indented along refines
tracelean-view . requirement REQ-THERMO       # one requirement, as in section 11.4
tracelean-view . coverage REQ-THERMO.to_celsius
tracelean-view . trace src/celsius.rs         # what a file claims, and what else claims the same clauses
tracelean-view . context REQ-THERMO
tracelean-view . findings
tracelean-view . evidence
tracelean-view . rollup
tracelean-view . stale
tracelean-view . bindings
tracelean-view . keys
```

Add `--json` for the buffer itself: its text and spans, with roles and links.

### 18.3 In CI

```bash
tracelean-trace .        | tail -1   # fails the build on "blocking: true" if you grep for it
tracelean-trace . --stale            # exit 1 when evidence went stale
tracelean-trace . --report report --diff origin/main   # attach report/index.html as an artefact
```

---

## 19. Customising

### 19.1 Keys

Every binding is data in [`assets/keymap.json`](assets/keymap.json): modes,
each with a parent, and keys that either **enter** a mode or **dispatch** a
named action:

```json
"Trace": {
  "parent": "Leader",
  "bindings": [
    { "c": { "dispatch": { "action": "trace.check", "description": "check the tree" } } }
  ]
}
```

An edit that does not hold together (a key that enters an undeclared mode, an
action nobody registered) is refused when the keymap loads, not when you press
the key. The menus and the which-key bar are computed from this file, so they
always show the keys you defined.

### 19.2 Colours and fonts

Every colour and font is in `assets/theme.json`. A project overrides any of them
in `.tracelean/theme.json`. Easiest of all is the **Definitions** station
(`Space w t c`): every setting is listed with its value; put the cursor on one,
press `Space f e` (or click it) and type the new value. It is saved to the
project's override and shown at once. The window also picks up hand edits when
it regains focus.

### 19.3 Languages

`assets/languages/` holds each language's grammar and the words that declare
things. These decide what is coloured, where an annotation attaches, and what
"go to definition" finds.

For changing the editor itself (new actions, panels, frontends), see
[docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md).

---

## 20. Walkthroughs

### 20.1 A first look at the demo (5 minutes)

```bash
cargo run -p tracelean-tui -- demo
```

1. **Look.** The explorer lists `reqs/`, `specs/`, `src/` and `tests/`, with
   claim letters beside `celsius.rs`, `Thermo.lean` and `round_trip.rs`. The
   accented file name in `src/notes/` is there on purpose: it checks that the
   cursor counts characters, not bytes.
2. **Open code.** Arrow to `src/celsius.rs`, then `Space f o`. Each function
   has an **I** chip. Put the cursor inside `to_celsius`; the bar now offers its
   clause.
3. **Follow the claim.** With the cursor on `REQ-THERMO.to_celsius` in the
   comment, press `Space t o`. The requirement opens: each clause with its
   marks, links to its code, test and Lean model, its coverage and its
   strength.
4. **See the whole design.** `Space w t r` shows `REQ-THERMO` with `REQ-TABLE`
   refining it. `Space g a` unfolds everything.
5. **What is missing?** `Space w t f`: `REQ-TABLE.boiling` has nothing
   claiming it, `to_celsius` is `Unbound`, `to_fahrenheit` is `Untested`.
6. **Type and undo.** Open `src/blank.rs`, press `i`, type a line, `Escape`.
   Press `Ctrl+Z`, type something else, then `Space h t`: both branches are in
   the tree.
7. **Quit** with `q` from Normal mode.

### 20.2 Taking a new clause from sentence to `AJTP` (Kelvin, on the demo)

**1. Write the clause first.** Add to `demo/reqs/REQ-THERMO.md`:

```yaml
  to_kelvin: Converting whole degrees Celsius to Kelvin shall add two hundred and seventy-three.
```

Open the requirement (`Space t o` on its name). The new clause shows in red,
because nothing claims it, and `tracelean-trace .` reports it as `Unmodeled`.

**2. Read the context** before touching code:

```bash
cd demo
tracelean-trace . --context REQ-THERMO.to_kelvin
```

**3. Model it in Lean** (`specs/Thermo.lean`, inside `namespace Thermo`):

```lean
/-- @models REQ-THERMO.to_kelvin -/
def toKelvin (c : Int) : Int := c + 273
```

**4. Prove a property of the model, for every input:**

```lean
/-- @proves REQ-THERMO.to_kelvin -/
theorem kelvin_is_celsius_shifted (c : Int) : toKelvin c - c = 273 := by
  unfold toKelvin
  omega
```

This is honest: it applies `toKelvin` to a bound `c`, it is false for most other
functions, and it uses no axiom. A statement such as `toKelvin 0 = 273` would be
refused as an *example*, and `toKelvin c = toKelvin c` as holding of every
function.

**5. Implement it in Rust** (`src/celsius.rs`), with `@drt` so it is compared
with the model:

```rust
/// Celsius to Kelvin, in whole degrees.
///
/// @implements REQ-THERMO.to_kelvin
/// @drt REQ-THERMO.to_kelvin
pub fn to_kelvin(degrees: i64) -> i64 {
    degrees + 273
}
```

**6. Test it** (`tests/round_trip.rs`):

```rust
/// @tests REQ-THERMO.to_kelvin
#[test]
fn absolute_zero_is_zero_kelvin() {
    assert_eq!(to_kelvin(-273), 0);
}
```

**7. Earn the evidence:**

```bash
tracelean-trace . --proofs                        # P, once Lean accepts the theorem
tracelean-trace . --drt                           # T: toKelvin and to_kelvin on generated inputs
tracelean-trace . --judge REQ-THERMO.to_kelvin    # read the prompt: does the model say what the clause says?
tracelean-trace . --judge REQ-THERMO.to_kelvin --verdict agrees --by <you>   # J (a name in judges.json)
tracelean-trace . --lock
tracelean-trace . --stale                         # nothing left to redo
```

**8. Check.** `tracelean-view . requirement REQ-THERMO` shows `AJTP  to_kelvin`,
with links to the model, the theorem, the code and the test. The strength line
reads **open** and shows the obligation `∀ g : Int → Int, (∀ c, g c - c = 273) →
g = toKelvin`; proving it in a `@pins` theorem ([20.4](#204-pinning-a-model))
makes the model **pinned**.

### 20.3 Fixing an `Unbound` clause

`Unbound` means the model and the code exist but nothing compares them. For
`REQ-THERMO.to_celsius`:

1. Add `@drt REQ-THERMO.to_celsius` to the Rust function (it already has
   `@implements`).
2. Run `tracelean-trace . --drt`. The binding is derived from the Lean
   `toCelsius : Int → Int` and the Rust `to_celsius(i64) -> i64`; both are run
   on generated inputs.
3. If they disagree, you get the smallest input on which they differ, with both
   answers. Fix whichever is wrong; never fix it by rewording the clause.
4. `tracelean-trace . --lock`, and the clause shows **T**.

### 20.4 Pinning a model

```bash
tracelean-trace . --strength REQ-THERMO.to_celsius
```

It prints the obligation built from the accepted theorems, for example
`∀ g : Int → Int, (∀ f, ToCelsius f (g f)) → g = toCelsius`. Copy it into a
theorem annotated `@pins REQ-THERMO.to_celsius`, prove it, then run
`tracelean-trace . --proofs`. The model now reads **pinned**. If you later
change the model or one of its theorems, it returns to **attempted** until
checked again.

### 20.5 Reviewing an agent's change

```bash
# in the editor: Space a n, run the command, let the agent work, Space a d to read the diff
tracelean-trace . --stale
tracelean-trace . --diff HEAD
tracelean-trace . --report target/review --diff HEAD && xdg-open target/review/index.html
```

In the report, tick **lost a mark**. Any card there is a promise that was backed
before and is not now. Then tick **changed**, and read each card's changes and
its *Not held* reasons. Accept or reject per file in the sandbox panel.

### 20.6 Producing a report for a manager or auditor

```bash
tracelean-trace . --proofs && tracelean-trace . --lock
tracelean-trace . --report out/report
```

Send `out/report/index.html`. It is self-contained and every mark on it can be
checked against the evidence shown under it.

---

## 21. Troubleshooting and FAQ

**A key does nothing.** That is a bug: every key in every mode has an outcome.
Run `cargo test -p tracelean-tui --test driving`, which presses the keys this
manual names on a real terminal.

**The window says "No editor is attached".** The page has no bridge to the
core. Rebuild the page (`node web/build.mjs`) and check that
`withGlobalTauri` is still set in `crates/desktop/tauri.conf.json`.

**`Space d r` only shows a command.** By design: the editor starts no
processes. Run the command it shows in a terminal.

**A mark disappeared after I edited something.** The evidence it rested on
changed (the clause, its model, its code or the toolchain). Run
`tracelean-trace . --stale` for what to redo, and the command for each.

**My theorem was refused.** The finding says why (see the table in
[14.3](#143-p-proved-honest-theorems)). Most often the theorem does not mention
the model, mentions it only on fixed values, or would be true of any function.
Restate it about the model, for every input.

**`Imprecise` on a Lean file.** The vendored grammar could not parse a
declaration, often an inline `match` or `let` inside a theorem's statement.
`tracelean-trace . --unparsed <file>` shows where. Restate it with a helper or
with `Option.map`/`Except.bind`.

**`--diff` says there is no lock at the revision.** That revision has no
`.tracelean/trace.lock`. Compare against a later one, or commit a lock first.

**Coverage does not show.** Run `tracelean-trace . --coverage`; it needs
`rustup component add llvm-tools`. Coverage hides itself on a file you edited
since it was measured.

**The sandbox will not start.** Install `bubblewrap` (`bwrap`), and build the
checker once (`cargo build -p tracelean-core --bins`) so it is on the agent's
`PATH`.

**Where is everything stored?**

| Path | Holds |
|---|---|
| `reqs/` | requirements |
| `.tracelean/trace.lock` | the committed index: links and evidence |
| `.tracelean/evidence/` | one record per clause and bond |
| `.tracelean/proofs.json` | Lean's verdict on every theorem |
| `.tracelean/drt.json` | what bindings cannot derive: floors, waivers, extra clauses |
| `.tracelean/theme.json` | the project's colour overrides |
| `assets/keymap.json` | the keymap |

**Further reading:** [docs/USING.md](docs/USING.md) (the short version),
[docs/09-using-the-tui.md](docs/09-using-the-tui.md) (a session on the demo),
[skills/README.md](skills/README.md) (the method, for agents),
[reqs/README.md](reqs/README.md) (every requirement) and
[docs/decisions/](docs/decisions/) (why things are the way they are).
