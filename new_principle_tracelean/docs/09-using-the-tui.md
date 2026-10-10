---
describes: [REQ-DRIVE, REQ-MYTH]
described_hash:
  REQ-DRIVE: ea6e5510b364457b
  REQ-MYTH: 0120c37ad5d78c7b
---

# Using the terminal frontend

Everything here is checked. The keys, the menus and the tree are the ones
`crates/tui/tests/driving.rs` presses, opens and lists — it runs the real binary
on a real pseudo-terminal over [`demo/`](../demo), so a page of instructions
that stopped being true is a test that stopped passing
([REQ-DRIVE](../reqs/surface/REQ-DRIVE.md)).

## Open it

```bash
node web/build.mjs                  # once, if you also want the window
cargo run -p tracelean-tui demo     # the demo tree
cargo run -p tracelean-tui          # this project, checking itself
cargo run -p tracelean-tui ~/code/anything
```

With no path it opens the current directory. `demo/` is the tree to start on:
small enough to see all at once, and the one the driving suite drives.

## The three rows

```
┌────────────────────────────────────────────────┐
│ DEMO                                           │  the buffer — a window of it
│ ▾ src                                          │
│   ▾ notes                                      │
│ …                                              │
│                                                │
│ ready: . lists what can be done here, space …  │  the status line
│ a  agent  d  differential testing  f  file  …  │  the menu, or what is here
└────────────────────────────────────────────────┘
```

All three are buffers the core produced. The frontend draws them with one
function and adds nothing of its own — that is
[REQ-VIEW](../reqs/surface/REQ-VIEW.md), and it is why the window shows you the
same file listing: same buffer, different medium. The window draws a row that
carries an action as a button; the terminal binds it to a key. Neither invents
one.

The last row is the menu while a mode offers one, and otherwise what the cursor
can do where it is.

## Keys

| Key | Does |
|---|---|
| `Space` | opens the leader menu |
| `i` | types into the file under the cursor |
| arrows | move the cursor — no mode, no leader |
| `Escape` | leaves one mode, towards the root |
| `q` | closes the frontend, from the root mode |
| `Ctrl-C` | closes it from anywhere |

The arrows are not bound by the keymap. They pass through to the surface, which
is what `REQ-MYTH` reserves the fourth outcome for: a key the modes do not claim
is not a key that vanishes.

Under the leader:

| Key | Menu | Then |
|---|---|---|
| `f` | files | `o` open · `n` new · `s` save · `r` rename · `d` delete |
| `h` | history | `u` undo · `r` redo · `t` show the tree · `b` switch branch |
| `t` | trace | `c` check · `f` findings · `e` evidence · `r` rollup · `s` stale · `l` lock |
| `d` | differential testing | `b` bindings · `c` coverage · `r` run · `s` shrink · `j` judge |
| `a` | agent | `s` watch · `d` review the diff · `a` accept · `x` reject |

Every one of those is read out of [`assets/keymap.json`](../assets/keymap.json),
which is data you can edit. An edit that does not hold together — a key that
enters a mode nobody declared, an action nobody registered — is refused when the
keymap loads, not when you press the key
([REQ-MYTH](../reqs/surface/REQ-MYTH.md), `keymap_is_data`).

A key nothing binds is never swallowed. At the root you are told:
`nothing happened: 'z' is not bound here`. Inside a menu it takes you back out,
so a mistyped leader never strands you.

## A first session, on the demo tree

```bash
cargo run -p tracelean-tui demo
```

1. **Look.** The explorer is a tree (`Space f o` on a folder opens or closes
   it): `src/celsius.rs`, `src/notes/café.md`, an empty `src/blank.rs`. The
   accented filename is there on purpose: a frontend that counted bytes instead
   of characters would put the cursor in the wrong column.
2. **Open a file.** Move the cursor to `celsius.rs` with the arrows, then
   `Space f o` — leader, files, open. There is no Enter-to-open: every action
   has a name and a key sequence that reaches it, and `file.open` is no
   exception. The spans come from the Rust parser, so what is marked is what
   the grammar found — the core produced them, the frontend coloured them.
3. **Type.** `i` puts you in insert mode; Escape leaves. On a directory listing
   `i` refuses out loud — *not a file: there is nothing here to type into* —
   because there is nothing there to edit.
4. **Undo.** `Space h` opens history. It is a tree, not a stack: undoing and
   then typing something else does not lose the branch you left
   ([REQ-UNDO](../reqs/history/REQ-UNDO.md)).

## A second session, on this project

The demo tree has no requirements, no models and no annotations, so the trace
menus have nothing to report there. For those, open the tree that does:

```bash
cargo run -p tracelean-tui        # from the project root
```

Then `Space t` and:

| Key | Shows |
|---|---|
| `c` | check the tree — findings, documents, what is blocking |
| `f` | list findings |
| `e` | evidence for the anchor under the cursor |
| `r` | the coverage rollup |
| `s` | what went stale |
| `l` | write the lockfile |

`Space d` is the same idea for differential testing: `b` lists the bindings, `c`
the coverage floors.

Three of those — `drt run`, `drt shrink`, `drt coverage` — answer with the
command to run rather than running it. The editor starts no processes and never
calls a model ([ARCH-NO-DRIVING](../reqs/arch/ARCH-NO-DRIVING.md)); every other
report is a read of the tree, so it answers offline.

## The window

```bash
node web/build.mjs
cargo run -p tracelean-desktop demo
```

Same editor, same buffers, same keymap — `crates/desktop` owns a window and five
commands and makes no decisions. A row that carries an action is a button you
can click; the key that dispatches it is the same key.

If the window ever says *No editor is attached*, the page has no bridge to the
core. `withGlobalTauri` in `crates/desktop/tauri.conf.json` is what injects it,
and `crates/core/tests/one_representation.rs` checks that it is still set.

## When something does not work

- **A key does nothing.** That is a bug, not a setting — `REQ-MYTH.totality`
  says every key in every mode has an outcome. Run
  `cargo test -p tracelean-tui --test driving`: it presses the keys this page
  names and reads the screen back.
- **The menu shows a key that does nothing.** The bar is computed from the
  keymap that dispatches the key, so it cannot describe a binding that does not
  exist (`whichkey_is_a_query`). If it does, `shipped_keymap.rs` should be
  failing.
- **A finding you do not understand.** The table in the
  [README](../README.md#what-the-output-means) says what each one means. Only
  malformed documents, dangling annotations, refinement cycles, duplicate
  identifiers, contested clauses and unsound exemptions block.

## What checks this page

```bash
cargo test -p tracelean-tui --test driving       # the keys, the menus, the demo tree
cargo test -p tracelean-core --test shipped_keymap
cargo test -p tracelean-core --test differential_drive -- --ignored
```

The first opens a pseudo-terminal, spawns the real binary on `demo/`, and writes
the bytes a keyboard writes — no key is named on the frontend's behalf, which is
the whole point. The last compares `surface::drive` against
`formal/TraceLean/Drive.lean` over generated sessions, so the walk this page
describes is a walk two implementations agree about.
