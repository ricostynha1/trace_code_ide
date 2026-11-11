# TraceLean on the demo project

This tutorial is done **on the demo**, a small project that is open next to this
tab (if not: `Space p`, **File ▸ Open project**, pick `demo/`). It has real
problems on purpose. You will read each one, fix it, and watch the evidence
come in, until the project is closed.

Rows that start with ▸ hold more. **Click one** (or put the cursor on it and
press `Space m`) to unfold it; again to fold it. Come back here any time:
`Space ?`. Every edit you make can be taken back (`Ctrl+Z`, or the undo tree,
`Space h t`), so try things.

+++ 1. Find your way around

```
┌──────────────────────────── menu bar ─────────────────────────────────┐
│ File Design Trace History Agent Window  [Overview]  A− A+  Search  ?  │
├────┬─────────── tabs: what is open, each with a × ────────────────────┤
│ ▣  │ explorer        │ document                  │ side panel         │
│ ⛓  │ (the files)     │ (what you read or edit)   │ (trace, design,    │
│ ▤  │  src            │  I  /// @implements REQ-X │  findings, history)│
│ ◔  │   celsius.rs I  │     pub fn to_celsius(…)  │                    │
│ ⚠  │  tests          │                           │                    │
│    │   round.rs  T   │  ▲ chips in the gutter    │                    │
├────┴────────── what can be done here, keys first ─────────────────────┤
│ Space f s  Save        Space w v  Split right     Space t c  Check    │
├───────────────────────────── status line ─────────────────────────────┤
│ saved: src/celsius.rs                          Δ 2   ⚠ 3    NORMAL    │
└───────────────────────────────────────────────────────────────────────┘
```
- **Icon rail** (far left): one icon per view of the project, as in VS Code.
  Point at one for its name, click to open it. From the top: project, trace,
  sandbox, design graph, history, findings, definitions, report, overview,
  modules.
- **Explorer**: the files. The letters after a name say what the file claims.
- **Document**: what you read or edit. **Side panel**: what a rail icon opens.
- **Bottom bar**: always lists what works *where the cursor is*.
- **Status line**: what just happened, or why nothing did; on the right `Δ`
  (clauses your unsaved edits touch), `⚠` (findings) and the mode.

+++ The mouse, in full
- **Click** a file to open it; a folder to open or close it.
- **Click into** a file to put the cursor there and type.
- **Right-click** anything: a menu at the pointer with every action available
  there, and the keys that reach each one.
- **Point** at a chip, a requirement name or a node of the Overview: a card
  shows its words, its marks and who claims it.
- **Ctrl+click** a name to go to its definition.
- **×** on a tab closes it; so does a middle click. Drag a tab to split.
- The wheel scrolls; drag selects; drag a divider to resize.
+++

+++ The keyboard, for those who want it
The keys are Vim-inspired, but you can ignore that: click and type, `Ctrl+S`
saves, `Ctrl+Z` undoes. Three modes; the status line names the one you are in.

```
            i  or  click into a file
  NORMAL ─────────────────────────────▶ INSERT
  (move, read, command)  ◀───────────── (type)
     │   ▲                  Escape
     ▼   │ Escape, or any key the menu does not bind
   MENU  (Space: every command, one letter each)
```
- `Space` opens the menu of every command. Read it, press a letter.
- `.` lists what can be done where the cursor is. `Escape` goes back one step.
- `Ctrl+P` searches every command, `Ctrl+O` opens a file by name, `Ctrl+F`
  finds, `F12` goes to a definition, `F5` refreshes the files, `F1` lists keys.
+++
+++

+++ 2. Why does this code exist?

That is the question the editor answers. **Requirements** live in `reqs/`; each
has **clauses**, single checkable sentences. Code, tests and Lean models name
the clause they serve, in a comment.

> Try it: `Ctrl+O`, type `celsius`, `Enter`. Each function has a doc comment
> with `@implements REQ-THERMO.…`, and a coloured **chip** in the gutter.
> Point at a chip for its card; click it to open the requirement.

Then open `reqs/REQ-THERMO.md` (click a chip, or `Space t o` on a name). The
file has four clauses: `to_fahrenheit`, `to_celsius`, `round_trip`,
`agreement`. Under them, every claim on each clause, with a link.

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
Every arrow is a comment somebody wrote. Nothing is inferred from a file name.
An arrow that is missing is shown as missing, never guessed.
+++

+++ The letters beside code
```
 I   /// @implements REQ-THERMO.to_celsius     this code does what the clause says
 T   /// @tests      REQ-THERMO.to_celsius     this test checks it
 M   /-- @models     REQ-THERMO.to_celsius -/  this Lean function is its model
 P   /-- @proves     REQ-THERMO.to_celsius -/  this theorem is about the model
 D   /// @drt        REQ-THERMO.to_celsius     compared with the model on generated inputs
```
The same letters follow a file's name in the explorer. See them on
`src/celsius.rs`, `tests/round_trip.rs` and `specs/Thermo.lean`.
+++

+++ The four marks: A J T P
Every clause carries four marks. They are **independent**: a strong one never
hides a missing one, and nothing is averaged.

```
 A  annotated   something claims the clause          you wrote a comment
 J  judged      a person read the model and agreed   Space t j shows the prompt
 T  tested      code and model agreed on N cases     tracelean-trace . --drt
 P  proved      Lean accepted a theorem about it     tracelean-trace . --proofs

 A---   only claimed          AJT-   judged and tested, no proof
 ----   nothing yet           AJTP   everything
```
A mark is **earned by a run**, never typed in. Change the code, the model or
the clause and the mark goes stale until the run is redone.
+++
+++

+++ 3. See the demo as a whole

> Try it: click **Overview** in the menu bar.

Two requirements: `REQ-THERMO` (approved, four clauses, mostly claimed) and
`REQ-TABLE` (a **draft** that refines it). Each box has four dots for A J T P
and a bar for how much is implemented. Point at one for its card, click to
open it.

+++ What the Overview draws
- **Tallies** across the top: requirements, clauses, how many are complete,
  line coverage, findings.
- **Graph**: a requirement sits below the one it refines. `REQ-TABLE` hangs
  under `REQ-THERMO`.
- **Treemap**: the same requirements as rectangles; area is the number of
  clauses, red holds little, green holds everything. Big red is where to look.
- **Matrix**: a row per requirement, a cell per mark, most work left first.
- **Test runs on record**: what was compared with its model, and how many
  cases.
- **Filters**: work left, not implemented, touched by your unsaved edits, one
  root only.
- As text, for the keyboard: `Space w t r`.
+++
+++

+++ 4. What is wrong with the demo

> Try it: `Space t c` checks the tree, `Space t f` lists the **findings**.

The demo reports seven. Each row below is one, with its fix; sections 5 to 7
do them in order.

```
 Unbound   REQ-THERMO.to_celsius      a model and code exist; nothing compares them
 Unbound   REQ-THERMO.to_fahrenheit   (same)                         → fix 1
 Untested  REQ-THERMO.to_fahrenheit   no test names this clause      → fix 2
 (gap)     REQ-TABLE.boiling          nothing claims it at all       → fix 3
 Unmodeled four clauses               no Lean model                  → left (see the end of 6)
```

+++ The findings worth knowing
```
 Unbound        a model and code both exist, and nothing compares them   ← fix first
 Dangling       an annotation names a clause that does not exist
 Contested      two things claim to be the one model of a clause
 Unmodeled      a clause has no Lean model                               (information)
 Unimplemented  a clause no code implements                              (information)
 Untested       a clause no test names                                   (information)
```
`Unbound` matters most: the clause looks finished and is not. Never silence a
finding by exempting it or by rewording the clause; claim it, or fix it.
+++
+++

+++ 5. Fix what is claimed but not checked

### Fix 1: bind the two conversions to their models (Unbound)

`to_celsius` and `to_fahrenheit` each have a Lean model in `specs/Thermo.lean`
and a Rust function, but nothing runs them against each other. Mark the Rust
function for comparison. In `src/celsius.rs`, add one line to each doc comment:

```rust
/// @implements REQ-THERMO.to_fahrenheit
/// @drt REQ-THERMO.to_fahrenheit
pub fn to_fahrenheit(degrees: i64) -> i64 {
```
```rust
/// @implements REQ-THERMO.to_celsius
/// @drt REQ-THERMO.to_celsius
pub fn to_celsius(degrees: i64) -> i64 {
```
Save (`Ctrl+S`). The `Δ` on the status line counted the clauses your unsaved
edit touched, and the marks they could lose. Then, in a terminal at the demo
folder, run `tracelean-trace . --drt`: generated inputs go to the model and to
the code, and the answers are compared (`Space d r` does not run it for this project). Both clauses now say **T**; the two `Unbound` findings
are gone.

+++ If they disagree
```
 seed 7 ──▶ generator ──▶ case 1, case 2, … case 2000
                              │            │
                         Lean model     your code
                              └── equal? ──┘
              all equal  → the clause earns T
              one differs → the case is shrunk to the smallest that fails
```
A failing case is shown side by side: what the model said, what the code
said. Decide which is wrong, and fix that one. Sometimes it is the model.
Never fix it by rewording the clause. Try it: change `+ 32` to `+ 31` in
`to_fahrenheit`, run `tracelean-trace . --drt --again`, read the case, then undo the edit.
+++

### Fix 2: give `to_fahrenheit` a test (Untested)

`tests/round_trip.rs` names `round_trip`, `to_celsius`, `agreement` and
`every_sample`, but not `to_fahrenheit`. Add to the end of the file:

```rust
/// @tests REQ-THERMO.to_fahrenheit
#[test]
fn water_freezes_and_boils() {
    assert_eq!(to_fahrenheit(0), 32);
    assert_eq!(to_fahrenheit(100), 212);
}
```
Save, `Space t c`. The `Untested` finding is gone, and the explorer shows `T`
after the file. Do not write a test that supplies the value the code should
have produced and checks it back: a test checks the code against something
else.
+++

+++ 6. Fix what nothing claims

### Fix 3: the draft requirement `REQ-TABLE.boiling`

Open `reqs/REQ-TABLE.md` (`Space t o` on a requirement name). It is a draft:
its clause `boiling` says *the samples shall include the temperature water
boils at*, and nothing claims it. The rule: **no code without a clause, and no
clause left without a claim.** Here the code exists (`main` samples 100), so
claim it. In `src/main.rs`, add one line above `fn main`:

```rust
/// @implements REQ-TABLE.every_sample
/// @implements REQ-TABLE.boiling
fn main() {
```
and prove it with a test in `tests/round_trip.rs`:

```rust
/// @tests REQ-TABLE.boiling
#[test]
fn boiling_water_is_steam() {
    assert_eq!(describe(100), "steam");
}
```
(`describe` must be imported: add it to the `use thermo::{…}` line at the top
of the file.) Save, `Space t c`: `boiling` is claimed, implemented and tested.
Then put the cursor on the requirement and `Space t a`: **approve** turns the
draft into an approved requirement. Only a person approves.

+++ Writing a requirement of your own
A requirement is an ordinary Markdown document, with nothing hidden in a header:

```
# REQ-TABLE: Printing a table of temperatures

- status: draft
- refines: REQ-THERMO

Why this exists, in prose.

## Clauses

- **every_sample**: Running the program shall print each sample temperature in both scales.
- **boiling**: The samples shall include the temperature water boils at.
```
- The first line names it: `# ID: title`. The ID is its identity, not its file name.
- One clause, one checkable sentence: "The system shall …".
- `refines:` names the requirement this one makes more precise; the design
  graph draws it.
- `Space t n` creates one with this shape filled in. **Write the clause first,
  then the code.** `Space c o` shows what an agent would need to change it.
+++


### How much of the code do the tests run?

Measure once, in a terminal: `tracelean-trace . --coverage`. Then put the
cursor on `REQ-TABLE` in `reqs/REQ-TABLE.md` and press `Space t v`
(**Trace ▸ coverage**): for each function that implements it, how many lines
ran, by which tests, and the lines no test runs. In the demo, before your
fixes, `describe` ran 4 of 7 lines (only the `"ice"` branch) and `main`
ran 0 of 5, because no test calls it. Add a test for the other two branches:

```rust
/// @tests REQ-TABLE.every_sample
#[test]
fn describe_covers_every_branch() {
    assert_eq!(describe(37), "water");
    assert_eq!(describe(100), "steam");
}
```
Measure again: `describe` is at 7 of 7.

### Left on purpose: Unmodeled

Four clauses still say `Unmodeled` (`boiling`, `every_sample`, `agreement`,
`round_trip`): no Lean function is marked `@models` for them. It is
information, not a fault: a model is written when a clause is worth proving.
`agreement` and `round_trip` already have theorems; a theorem is only
accepted when it applies the model to a variable, so write the model first
and restate the theorem about it.
+++

+++ 7. Earn the rest, and close the project

You have claims and tests. Three marks need a run or a person:

+++ T: tested against the model
`tracelean-trace . --drt`, as in fix 1. Both conversions now show **T**. Look at
**Test runs on record** in the Overview: the seed, the number of cases.
+++

+++ P: proved
`specs/Thermo.lean` has `@proves` theorems. Lean checks them from a terminal
at the demo folder: `tracelean-trace . --proofs`. An accepted theorem that
really states the model earns **P**. A theorem about an example (`toF 0 = 32`)
is refused: it must hold for every input.
+++

+++ J: a person agrees
`Space t j` on a clause shows its prompt: the clause beside its model, and the
question *does the model say what the clause says?* Read it. Then record the
answer, with your name, in a terminal:

```
tracelean-trace . --judge REQ-THERMO.to_celsius --verdict agrees --by <you>
```
Other answers are `drift` (the model says something else) and `unmodelable`.
+++

+++ Close the project
```
tracelean-trace . --lock       pin what has been earned        Space t l
tracelean-trace . --stale      what must be redone: should be nothing
```
`Space t p` writes the evidence report: a card per clause, for whoever
receives the project. The project is **closed** when the Overview shows every
mark earned, `Space t f` lists nothing, and `Space t s` lists nothing to redo.
+++
+++

+++ 8. Change something without losing the evidence

> Try it: in `src/celsius.rs`, change `to_celsius` and **save**.

- **Before saving**: `Δ n` on the status line is the clauses your edit puts in
  question, with the marks they could lose. Click it.
- **After saving**: `Space t s` (**what went stale**) lists the tests,
  comparisons and judgements the change re-opened. Redo them; nothing is
  silently kept.

### Undo: nothing is ever lost

Make three edits to `src/main.rs`: type a comment line, save, type another,
save, then `Ctrl+Z` twice and type something different (and save). Open the undo tree:
`Space h t` (or the history icon on the rail).

```
 ○─╮ #0  the tree as it was opened
 ● │ #3  typed "// other⏎" in src/main.rs  ✓ saved      ← where you are
   ○ #1  typed "// one⏎" in src/main.rs  ✓ saved        ← the way you came,
   ○ #2  typed "// two⏎" in src/main.rs  ✓ saved          still there
```
- Every state the project has been in is a node; undoing and then typing
  **branches**, it never deletes the old way. The branch you are on is
  listed first.
- `Space h j` goes to the node under the cursor; `Space h b` switches branch;
  `Space h f` shows all changes, only this file's, or only the saved ones.
- Point at a node to preview what that change did.
- The tree covers every file, so one undo can bring back a deleted file too.

### An agent works; you watch (simulated)

You do not need an agent to see what one does. A sandbox is a copy of the
project; whatever is changed inside it shows up here live, and nothing reaches
your project until you accept it. We will play the agent by hand.

1. `Space a n` (**Agent ▸ new sandbox**). It shows a command like
   `sh …/enter.sh`; copy it (`Space a c`) and run it in a terminal. You are
   now in a shell inside the sandbox copy of the demo.
2. In that shell, make the three kinds of change an agent makes:

```sh
echo '// the agent was here' >> src/main.rs                     # edit a file
printf 'pub fn to_kelvin(c: i64) -> i64 { c + 273 }\n' > src/kelvin.rs   # create one
rm src/blank.rs                                                  # remove one
```
3. Look at the editor, without doing anything:
   - the explorer shows a mark by each file: `●` `src/main.rs` (changed),
     `+` `src/kelvin.rs` (made), `✗` `src/blank.rs` (deleted), and `●` on the
     `src` folder. Point at a mark for what it means.
   - the undo tree has new nodes, one per file: `agent: 1 changes to
     src/blank.rs`, `agent: 2 changes to src/kelvin.rs`, and so on, under a
     line saying how many files are not accepted yet.
   - `kelvin.rs` claims no clause: the first thing to notice about an
     agent's new code. Give it one, or reject it.
4. **Review**: open the sandbox panel (`Space w t s`, or the sandbox icon on
   the rail) and put the cursor on a changed file. `Space a d` shows the diff; `Space a f` accepts one file,
   `Space a r` rejects one; `Space a a` accepts all, `Space a x` rejects all.
   Accepted files are marked `✓ accepted`.
5. **Take it back**: `Space h u` (undo; `Ctrl+Z` does the same with a file
   focused), `Space h r` redoes, or jump in the undo tree to the node before the
   agent's change. The sandbox copy follows, so the agent's next step starts
   from where you are.
6. `Space a e` ends the sandbox.

+++ With a real agent
Start your agent in that same shell (for example `claude`). `Space c o` on a
requirement gathers everything the agent needs to change it: the clause, its
model, its code, its tests; `Space c y` copies it. Ask it to give `to_kelvin`
a clause, a model, a test: then read the diff, and look at which marks it won
or lost before accepting. The editor never drives the agent; it only watches.

```
 your tree ──copy──▶ sandbox ◀── the agent edits here
     ▲                  │
     └── accept ◀── you review: the diff, and the marks it won or lost
```
+++
+++

+++ 9. Make it yours

`Space w t c` (or the definitions icon on the rail) opens the **settings**:
every colour, the theme (dark or light, also `Space w c`), the zoom, and where
things are drawn. Each is a line of JSON in `.tracelean/theme.json`.

+++ The settings that change the layout
```
 layout.menuBar    shown | hidden        the bar at the top
 layout.stations   hidden | left | top   the icon rail (the Window menu does the same)
 layout.zoom       60 … 200              percent; Ctrl + and Ctrl - step it
 base              dark | light          which shipped theme your colours sit on
```
+++
+++

## Where next

- `F1` lists every key. `Ctrl+P` searches every command by a few words.
- `user_manual.md` is the full manual; its section 20 repeats this walk-through
  with a new clause (Kelvin) taken from a sentence to **AJTP**.
- Close this tab with the **×** on it, or `Ctrl+W`.
