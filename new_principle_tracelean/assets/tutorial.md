# TraceLean in five minutes

Do each step in order. The **status line** at the very bottom says what
happened after every key. Come back here any time: `Space ?`.

Rows that start with ▸ hold more. **Click one** (or put the cursor on it and
press `Space m`) to unfold it; again to fold it.

## 1. You can use it like any editor

The keys are **Vim-inspired**, but you can ignore that completely: click where
you want to type and type, `Ctrl+S` saves, `Ctrl+Z` undoes, the mouse wheel
scrolls, the menu bar at the top holds every command, and a right-click
anywhere lists what can be done there.

+++ What the screen is made of
```
┌──────────────────────────── menu bar ─────────────────────────────────┐
│ Agent  File  Trace  Stations …   [Overview]  A− A+  Search  Theme  ?  │
├──────────── tabs: what is open, each with a × ───────────────────────┤
│ explorer        │ document                      │ side panel          │
│ (the files)     │ (what you read or edit)       │ (trace, design,     │
│  src            │  I  /// @implements REQ-X.c   │  findings, history) │
│   celsius.rs I  │     pub fn to_celsius(…)      │                     │
│  tests          │                               │                     │
│   round.rs  T   │  ▲ chips in the gutter        │                     │
├─────────────── what can be done here, keys first ─────────────────────┤
│ Space f s  Save        Space w v  Split right     Space t c  Check    │
├───────────────────────────── status line ─────────────────────────────┤
│ saved: src/celsius.rs                          Δ 2   ⚠ 3    NORMAL    │
└───────────────────────────────────────────────────────────────────────┘
```
- **Explorer**: the project's files. Letters after a name say what the file
  claims (see section 3).
- **Document**: the file, requirement or page you are reading.
- **Side panel**: whatever a station opens.
- **Bottom bar**: always lists what works *where the cursor is*.
- **Status line**: what just happened, or why nothing did. On its right:
  `Δ` clauses your unsaved edits touch, `⚠` findings, and the mode.
+++

+++ The mouse, in full
- **Click** a file to open it; a folder to open or close it.
- **Click into** a file to put the cursor there and type.
- **Right-click** anything: a menu at the pointer with every action
  available there, and the keys that reach each one.
- **Point** at a chip, a requirement name or a node of the Overview: a card
  shows its words, its marks and who claims it.
- **Ctrl+click** a name to go to its definition.
- **×** on a tab closes it; so does a middle click.
- The wheel scrolls; drag selects.
+++

+++ The keyboard, for those who want it
There are three modes, and the status line always names the one you are in.

```
            i  or  click into a file
  NORMAL ─────────────────────────────▶ INSERT
  (move, read, command)  ◀───────────── (type)
     │   ▲                  Escape
     │   │ Escape, or any key the menu does not bind
     ▼   │
   MENU  (Space: every command, one letter each)
```
- `Space` opens the menu of every command. Read it, press a letter.
- `.` lists what can be done where the cursor is.
- `Escape` always goes back one step.
- Nothing is hidden: a key that does nothing says so on the status line.

Useful chords in any mode: `Ctrl+S` save, `Ctrl+Z` / `Ctrl+Y` undo and redo,
`Ctrl+P` open a file by name, `Ctrl+Shift+P` search every command,
`Ctrl+F` find, `Ctrl+W` close the tab, `F12` go to the definition,
`Ctrl +` / `Ctrl -` bigger and smaller text, `F1` every key.
+++

> Try it: `Ctrl+P`, type `readme`, `Enter`. Click into the file, type a
> word, undo it with `Ctrl+Z`. Come back with the **tutorial** tab.

## 2. Why does this code exist?

That is the question the editor answers. **Requirements** live in `reqs/`.
Each has **clauses**: single sentences that can be checked. Code, tests and
Lean models name the clause they serve, in a comment.

```rust
/// @implements REQ-THERMO.to_celsius
pub fn to_celsius(f: f64) -> f64 { (f - 32.0) * 5.0 / 9.0 }
```

> Try it (on the demo): `Ctrl+P`, type `celsius`, `Enter`. A coloured
> **chip** sits in the gutter beside each annotated line. Point at one for
> its card; click it to open the requirement.

+++ The chain, drawn
```
 requirement ──@models──▶ Lean model ──@implements──▶ implementation
  (a clause)              (a function)                 (your code)
      │                        │  ▲                        │
      │                   @proves  └──── @drt: both run ────┘
      │                        │         on generated inputs
   a person                 a theorem     and must agree
   judges it              Lean checked
```
Every arrow is a comment somebody wrote. Nothing is inferred from a file
name. An arrow that is missing is shown as missing, never guessed.
+++

+++ The chips beside code
```
 I   /// @implements REQ-THERMO.to_celsius     this code does what the clause says
 T   /// @tests      REQ-THERMO.to_celsius     this test checks it
 M   /-- @models     REQ-THERMO.to_celsius -/  this Lean function is its model
 P   /-- @proves     REQ-THERMO.to_celsius -/  this theorem is about the model
 D   /// @drt        REQ-THERMO.to_celsius     compared with the model on generated inputs
```
The same letters follow a file's name in the explorer, so you can see what a
file is for before opening it.
+++

+++ The four marks: A J T P
Every clause carries four marks. They are **independent**: a strong one
never hides a missing one, and nothing is averaged.

```
 A  annotated   something claims the clause          (you wrote a comment)
 J  judged      a person read the model and agreed   (Space t j)
 T  tested      code and model agreed on N cases     (Space d r)
 P  proved      Lean accepted a theorem about it     (Space t c)

 A---   only claimed          AJT-   judged and tested, no proof
 ----   nothing yet           AJTP   everything
```
A mark is **earned by a run**, never typed in. Change the code, the model or
the clause and the mark goes stale until the run is redone.
+++

+++ Writing a requirement
A requirement is a Markdown file with a header:

```
---
id: REQ-THERMO
title: Converting temperatures
refines: []
status: approved
clauses:
  to_celsius: When given a Fahrenheit value, the system shall return …
  to_fahrenheit: When given a Celsius value, the system shall return …
---

# Converting temperatures
Why this exists, in prose.
```
- One clause, one checkable sentence. "The system shall …".
- `refines:` names the requirements this one makes more precise. That is
  what the design graph draws.
- `File › new` (`Space f n`) creates one; `Space t n` creates a requirement
  with the header filled in.
+++

## 3. See the whole design

> Try it: click **Overview** in the menu bar.

+++ What the Overview draws
```
 ┌ 52 requirements ┐┌ 396 clauses ┐┌ 12/52 complete ┐┌ 81% lines ┐┌ 3 findings ┐

 I  Implemented  ████████████████████░░░░░   313/396
 A  Annotated    █████████████████████████   395/396
 J  Judged       ████████░░░░░░░░░░░░░░░░░   120/396
 T  Tested       ██████████████░░░░░░░░░░░   230/396
 P  Proved       ████░░░░░░░░░░░░░░░░░░░░░    61/396

        ┌ ARCH-HONEST ┐          roots at the top
        │ ● ○ ○ ○ 2/6 │          dots: A J T P
        └──────┬──────┘          bar: how much is implemented
       ┌───────┴───────┐
 ┌ REQ-EVID ──┐  ┌ REQ-STALE ─┐  a requirement refines the one above it
 │ ● ● ● ○ 9/9│  │ ● ◐ ○ ○ 9/10│
 └────────────┘  └─────────────┘
```
- **Filters** above the graph: work left, not implemented, touched by your
  unsaved edits, one root only, or a name.
- **Treemap**: the same requirements as rectangles. Area is the number of
  clauses; red holds little, green holds everything. Big red is where to look.
- **Matrix**: a row per requirement, a cell per mark, most work left first.
- **Test runs on record**: what was compared with its model, with how many
  cases, and buttons to run them again.
- Point at anything for its card. Click to open it.
+++

The same information as text, for the keyboard: `Space w t r`.

## 4. What is wrong, and what to do next

> Try it: `Space t f` lists the **findings**: what the checker reports.
> `Space t c` re-checks the tree.

The number beside ⚠ in the bottom-right corner is the count; click it.

+++ The findings worth knowing
```
 Unbound        a model and code both exist, and nothing compares them   ← fix first
 Dangling       an annotation names a clause that does not exist
 Contested      two things claim to be the one model of a clause
 Unmodeled      a clause has no Lean model                               (information)
 Unimplemented  a clause no code implements                              (information)
 Untested       a clause no test names                                   (information)
```
`Unbound` matters most: it looks finished and is not.
+++

+++ Before you save: what your edit touches
Edit a file that claims clauses and `Δ n` appears on the status line: the
clauses your unsaved change puts in question. Click it.

```
 Clause                    Holds now   Claimed by the unsaved file
 REQ-THERMO.to_celsius     AJT-        src/celsius.rs
 REQ-THERMO.round_trip     A---        src/celsius.rs
```
`AJT-` has something to lose: after saving, its tests and judgement must be
redone. `A---` has nothing earned yet.
+++

## 5. Earning the marks

+++ T: test code against its model
`Space d r` generates inputs from a seed, runs the Lean model and your code
on each, and compares.

```
 seed 7 ──▶ generator ──▶ case 1, case 2, … case 2000
                              │            │
                         Lean model     your code
                              └── equal? ──┘
              all equal  → the clause earns T
              one differs → the case is shrunk to the smallest that fails
```
A failing case is shown side by side: what the model said, what the code
said. Fix the one that is wrong; sometimes it is the model.
+++

+++ J and P: a person, and a proof
- **J** (`Space t j` on a clause): shows the clause beside its model and asks
  you whether the model says what the clause says. Your answer is recorded
  with your name.
- **P**: write a theorem about the model in Lean and mark it
  `@proves REQ-X.clause`. `Space t c` asks Lean; an accepted theorem that
  really states the model earns P.
+++

## 6. Working with an agent

+++ Hand a clause to an agent, review what comes back
The **Context for an agent** menu (`Space c`), on a requirement, gathers
everything an agent needs to change it: the clause, its model, its code, its
tests. `Space c y` copies it. `Space a n` starts a sandbox: a
copy of the project the agent works in. You see what it touched and what that
did to the evidence, then accept or reject, file by file.

```
 your tree ──copy──▶ sandbox ◀── the agent edits here
     ▲                  │
     └── accept ◀── you review: the diff, and the marks it won or lost
```
The editor never drives the agent; it only watches.
+++

## 7. Make it yours

`Space w t c` opens the **settings**: every colour, the theme (dark or
light, also `Space w c`), the zoom, and where things are drawn. Each is a
line of JSON in `.tracelean/theme.json`.

+++ The settings that change the layout
```
 layout.menuBar    shown | hidden        the bar at the top
 layout.stations   hidden | left | top   the icon rail (the Stations menu does the same)
 layout.zoom       60 … 200              percent; Ctrl + and Ctrl - step it
 base              dark | light          which shipped theme your colours sit on
```
+++

## Where next

- `F1` lists every key.
- `user_manual.md` is the full manual.
- Close this tab with the **×** on it, or `Ctrl+W`.
