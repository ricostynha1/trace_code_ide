---
describes: [REQ-DRT-TS]
described_hash:
  REQ-DRT-TS: dcb4028876793f50
---

# Two frontends, both checked

```
crates/tui       terminal, crossterm
crates/desktop   window, Tauri  ──► web/  the page it loads
crates/editor    the state both of them drive
```

`crates/editor` exists so there is one answer to *what the editor does*. A
terminal and a window that each performed intents their own way would be the
same failure one representation prevents, one layer up. A frontend renders and
sends keys; the editor decides.

## The web frontend

`web/src/view.ts` is a **second implementation** of the two questions a frontend
asks of a buffer — what are its lines, and what can be done here. That is the
thing most likely to drift, so it is not trusted:

| Checked | How |
|---|---|
| `plainText`, `actionsAt`, `conformance` | differentially, against the **Lean model** |
| what it says it drew | `drt::frontend::check` |
| what it put on the page | `drt::frontend::capture` |
| where the buttons are | a person (L2) |

It is compared with the model and never with the Rust. Two implementations agree
perfectly when both are wrong.

`web/src/render.ts` is the only place the page decides what it says; `app.ts`
builds DOM from it and `frontend.ts` answers the harness from it, so reporting
and drawing cannot come apart. `--embroider` and `--invent` make it draw wrongly
on purpose, and the suite proves both are caught.

A role becomes a class name and the stylesheet says how it looks — the same
division the terminal makes with colours. `level` carries its grade, so both
frontends paint L1 apart from L4 from the same value rather than by reading the
characters. `--mislabel` and `--regions` are the other two wrong-on-purpose
handles: the first names a painted emblem after itself, the second reports how
many regions each line was cut into, which is the one property of this frontend
that the text it draws cannot show.

## What each frontend asks for

| Asked | Answered with |
|---|---|
| `shown(height)` | the window of the buffer that fits |
| `cursor(height)` | the line and column, or nothing if it is off the window |
| `menu` | the mode's menu, while there is one |
| `press(key)` | nothing; the editor interpreted it |
| `act(action, offset)` | nothing; the editor dispatched it |

Four buffers reach the screen — the window, the status line, the menu, and
nothing else — and every one of them came from a producer. The menu's rows carry
actions, which is how a window offers what a terminal binds to a key: the same
`dispatch`, from a click instead of a keystroke.

## The TypeScript runner (`REQ-DRT-TS`)

`drt::ts_runner` generates a Node process speaking the same line protocol as the
Lean and Rust runners; `Binding.also_implemented_by` lets one model be the
oracle for several implementations.

Node strips the types and runs the project's own file, so there is no build
output that could differ from what the browser loads. The cost: nothing
typechecks the call, so a binding that does not fit throws — which is why every
call is wrapped and a throw becomes *that case's* error rather than the end of
the run.

Argument order still comes from the implementation's signature (`tsParameters`,
modelled and differentially tested), so a binding cannot silently reorder them.

**The TypeScript carries no annotations.** There is no TypeScript grammar here,
so no claim inside one of those files can be *placed* — and ADR-0008 rejects
recovering that by reading text. The claims live where they can be checked: the
binding file names `web/src/view.ts::plainText` and its neighbours, and the
suites drive them. Each file says in prose what it realises and why the claim is
not in it. The stage-2 fixpoint caught this: stage 0 was reading those comments
as links and this kernel was not.

## Driving them (`REQ-DRIVE`)

The table above checks each frontend by handing it a buffer. That leaves one
translation per frontend untested — the code turning what a keyboard sent into
what the keymap calls it — and the editor was unusable for a reason that lived
exactly there: the leader was bound as `Space`, both frontends handed over
`" "`, and every suite still passed.

So `crates/tui/tests/driving.rs` supplies no names. It opens a pseudo-terminal,
runs the real binary on [`demo/`](../demo), writes the bytes a keyboard writes
and reads the screen back. What it compares against is `surface::drive` —
modelled in `formal/TraceLean/Drive.lean` and differentially tested — so the
prediction is not the suite's own opinion.

`keymap::typed` and `keymap::NAMED` are why there is one naming rather than one
per frontend, and `web/src/app.ts` carries the same set in its own language
because a browser spells its keys differently.

The window has the same gap in a different place: its page reaches the core
through a bridge the runtime injects only when the configuration asks for it. A
window whose page cannot ask for a buffer draws none, which is not a rendering
fault — it is this frontend never being connected. `one_representation.rs`
checks the configuration and the page still name the same bridge.

## Build

```
node web/build.mjs          # web/src/*.ts -> web/dist/*.js, no dependencies
cargo run -p tracelean-tui
cargo run -p tracelean-desktop
```

`build.mjs` uses Node's own type stripper and rewrites `./x.ts` imports to
`./x.js`. No bundler, no package manager, no lockfile, and nothing between the
source that was checked and the file that ships.
