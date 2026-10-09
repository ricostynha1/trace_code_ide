# The screen: what is opened, and what is shown

**Status: proposal.** Nothing here is built. It exists to be checked against
what the surface is meant to look like before any of it is written.

## What is missing

Today `shown` returns one `Buffer` and the window draws it. Five things the
surface is meant to have and the representation cannot express:

1. **Several buffers on screen at once**, in regions that divide the screen and
   can be resized — by keyboard and by dragging with a mouse.
2. **More buffers opened than shown.** The opened set appears as a strip along
   the top; pressing one shows it.
3. **Four stations always present**: Project, Sandbox, Requirements, Design.
4. **Pressing a station opens a buffer** holding that station's content.
5. **The desktop frontend draws a station as an icon**, not as the literal text
   the buffer carries.

(1)–(4) are missing from the core: there is no layout value and no producer for
a strip, a station, or a station's content. (5) is currently *forbidden* by
`REQ-VIEW.text_is_the_content`, and is the one rule below that has to change.

## The value

```
Screen
  opened   : List Buffer     every buffer this session holds
  layout   : Layout          which of them are on screen, and where
  focus    : BufferId        which pane a key goes to
  strip    : Buffer          the opened set, as a Menu buffer
  stations : Buffer          the four, as a Menu buffer

Layout = Pane BufferId
       | Split { axis : Axis, parts : List Part }
Part   = { weight : Nat, layout : Layout }
Axis   = Across | Down
```

**Weights are natural numbers, not fractions.** A pane gets `weight / sum` of
its region. Resizing has to be one total function that Lean, Rust and TypeScript
agree about case for case, and floating point is three answers at the last
digit. A keypress and a mouse drag both end in the same `resize(layout, divider,
amount)`; the drag converts pixels to a weight delta in the shell, where it can
be checked against the region it was handed.

**The strip and the stations are buffers**, not special structures.
`REQ-VIEW.everything_is_a_buffer` is the reason: a tab strip with its own type
would be a second thing to render, the terminal would not get one, and it would
rot exactly the way the old TUI did. They are `Menu` buffers whose rows carry
actions on their spans, so a click and a key reach them by the one path
`REQ-ACT.one_path` already requires.

## The new actions

| Action | Does |
|---|---|
| `show <id>` | put an opened buffer into the focused pane |
| `open <station>` | produce a station's buffer, add it to `opened`, show it |
| `close` | drop the focused buffer from `opened` and collapse its pane |
| `split across` / `split down` | divide the focused pane |
| `focus next` / `focus left\|right\|up\|down` | move the focus |
| `grow` / `shrink` | resize the divider beside the focused pane |

They resolve to intents through `REQ-ACT` like everything else. Nothing here
acts on a name.

## Icons: the one rule that changes

`REQ-VIEW.text_is_the_content` and `screen_is_readable` say the lines a frontend
drew are the buffer's text, and `conformance` in `web/src/view.ts` checks it. An
icon is not that text, so as the rules stand the window may not draw one.

**The amendment.** A frontend may present a piece as a symbol **when the
symbol's accessible name is the piece's own text**, and the capture harness
reads accessible names rather than painted glyphs. The window draws an icon
where the buffer says `project`; the harness reads `project`; the comparison
still means what it said. A person using a screen reader hears the buffer's
text, which is the same guarantee from the other side. *Done in phase D; see
below, including where this draft was wrong about the mapping.*

**The mapping is role → symbol, and the role comes from the core.** So the
stations get roles (`Station`, and a `Tab` role for the strip), the stylesheet
maps role to icon the way it already maps role to colour, and a role the desktop
does not recognise is drawn as its text — `REQ-VIEW.rendering_is_total` already
requires that. *Superseded: a role cannot tell four stations apart. The mapping
is action → symbol.*

*Rejected:* a frontend-owned table from entry text to icon. That is the frontend
deciding what exists by pattern-matching on strings it recognises, which is the
thing banned one layer up. If the icon is chosen by the frontend from text it
parsed, nothing can check that it parsed correctly.

## Requirements to write

**New `REQ-SCREEN`** (`reqs/surface/`), refining `REQ-VIEW` and
`ARCH-CORE-SHELL`. Draft clauses:

```
screen_is_a_value        What is on screen shall be one value: the buffers opened,
                         the layout placing some of them, and which has focus.
opened_outlives_shown    A buffer shall remain opened when it is not shown, and
                         showing it again shall not re-produce it.
layout_covers_the_region Placing a layout in a region shall give every pane a
                         rectangle, and the rectangles shall tile the region
                         without gap or overlap.
every_pane_is_opened     Every buffer a layout places shall be one of the opened
                         buffers.
resize_preserves_the_sum Resizing a divider shall move weight between its two
                         sides and change no other pane's weight.
resize_has_a_floor       A pane shall not be resized below one character, and a
                         resize that would shall answer with that floor.
focus_is_placed          The focused buffer shall be one the layout places.
strip_is_the_opened_set  The strip shall be a buffer whose rows are the opened
                         buffers, in the order they were opened, each carrying
                         the action that shows it.
stations_are_constant    The four stations shall be produced for every state, so
                         that a session can always reach them.
station_opens_a_buffer   Pressing a station shall produce a buffer and show it,
                         and shall not change the layout otherwise.
```

**Amendments to existing requirements:**

- `REQ-VIEW` — add `presentation_may_be_symbolic`; reword `screen_is_readable`
  to name accessible text rather than painted text.
- `REQ-SHOW` — producers for the strip, the stations, and each station's
  content; `screen_from_state` (the core produces the `Screen`, not just a
  buffer).
- `REQ-MYTH` — default bindings for the actions above, or `actions_reachable`
  fails the moment they exist.
- `Role::Level` becomes `Level(Grade)`, so a frontend can distinguish L1 from L4
  without parsing the text it was given.

**New `REQ-COST`** (`reqs/agent/`), for the sandbox station's pricing — clauses
drafted below.

Every clause added is an `Unmodeled` finding the day it lands. That is the work
being signed up for, not a regression.

## Models, bindings, code

- `formal/TraceLean/Screen.lean` — `place`, `resize`, `show`, `openStation`,
  `focusStep`, `strip`, `stations`.
- `crates/core/src/surface/screen.rs` — the Rust side.
- `web/src/screen.ts` — a second implementation of the two questions the page
  asks: which rectangle holds which buffer, and which divider did the pointer
  grab. Compared against the **Lean model**, never against the Rust.
- `.tracelean/drt.json` — a binding per function. Floors to declare: a single
  pane; a split of two; a nested split; a weight at its floor; a drag past the
  end of the region; an opened set of none; a station opened twice.
- `crates/desktop/src/main.rs` — `shown` returns a `Screen`. Both frontends
  redrawn against it.

## What each frontend does with it

| | Terminal | Desktop |
|---|---|---|
| Split | box-drawn regions | flex panes |
| Divider | `grow`/`shrink` keys | the same actions, also by dragging |
| Strip | one line of text | tabs |
| Stations | one line of text | an icon bar |
| Station row | its text | its icon, named by its text |

## What each station holds

### Sandbox — v0's flow, unchanged

v0 already had the arrangement this port wants, in
`core/src/sandbox/{session,watch,transcript,cost}.rs` and `SandboxPanel.tsx`:
create a session (a copy of the project under containment), hand the person the
shell command to enter it, let them run Claude Code there themselves, watch what
the workspace changes, tail the tool's own JSONL transcript, and price the usage
it reports. TraceLean launches nothing. That flow is kept as it was.

The station's buffer is a `Record` whose rows are three interleaved things:

| Row | From |
|---|---|
| a change the workspace made | the watcher — `REQ-OBS.visible_while_running`, `REQ-MIRROR` |
| a message the tool exchanged | the transcript tail — `REQ-TRANSCRIPT` |
| usage and its estimated cost | priced from the transcript's usage records |

So the AI chat panel does come back — as *this*, the tool's own transcript read
from disk, not a chat the editor is party to. There is no input box, and that is
not an omission: `REQ-OBS.no_instruction_channel` is stated as an absence
because an absence is what can be checked.

**Pricing needs a requirement it does not have.** Nothing in `reqs/agent/`
mentions tokens or cost today. Estimating spend is a pure function — usage
records plus a price table to an amount — so it is modelled and differentially
tested like everything else. Draft `REQ-COST` clauses:

```
cost_from_usage        Spend shall be computed from the usage the transcript
                       reports and a price table, and from nothing else.
price_is_per_model     A price shall be looked up by the model the transcript
                       names, and an unknown model shall be reported as unpriced
                       rather than priced at zero.
cache_priced_apart     Cached input, written cache and ordinary input shall be
                       priced separately.
estimate_is_labelled   A figure derived from a transcript shall be rendered as an
                       estimate, never as a billed amount.
```

`estimate_is_labelled` matters: this is a reading of somebody else's log against
a table we maintain. Showing it as if it were a bill is the kind of rounding-up
`ARCH-HONEST` exists to stop.

### Design — the refinement graph, in characters

Agreed as an indented ASCII graph over `reqs/arch/` and the requirements
refining them. **Colour carries the evidence level**, in the terminal as much as
in the window — which needs one change to the representation: `Role::Level`
today is a single role for any level, so a frontend cannot tell L1 from L4
without reading the text. It becomes `Level(Grade)`, carrying which one. Then
the terminal colours a level the same way the window does, from the same value,
and `REQ-COV`'s figures are legible at a glance in both.

### Project

Opens the project tree, as a directory buffer navigable with the same keys as
anything else. Settled below: a buffer, never a native folder picker.

### Requirements

The requirement index: one row a requirement, carrying the level its evidence
reached and the requirement's own identifier. Both spans take their actions
from their roles, so a level here offers what a level offers anywhere.

## What is not restored

| | Why |
|---|---|
| Terminal panel | dropped |
| Project map / treemap | an ASCII rendering is plausible and wanted, but **not in this piece of work** |
| In-house AI chat | cut — `ARCH-NO-DRIVING`. What returns is the transcript view above |
| Lean infoview | `REQ-LSP` exists; not in this piece of work |
| Monaco editor | the editor is `crates/editor`; a pane draws its buffer |

## Phases

| | |
|---|---|
| A | **Done.** `REQ-SCREEN` written; `Screen.lean`; `screen.rs`; ten bindings with floors. Frontends unchanged. See below. |
| B | **Done.** Both frontends draw the layout; resize by key and by drag. See below. |
| C | **Done.** Strip and stations drawn by both frontends; pressing one shows or opens. See below. |
| D | **Done.** The conformance amendment, and the window's bar is emblems. See below. |
| E | **Done.** Requirements and Design producers; `Level(Grade)` and its colours. See below. |
| F | **Done.** The sandbox station: transcript tail, usage, `REQ-COST`. See below. |

Each phase leaves the tree usable and the checker green before the next starts.

## What phase A produced

`reqs/surface/REQ-SCREEN.md` (14 clauses) · `formal/TraceLean/Screen.lean` ·
`crates/core/src/surface/screen.rs` (13 unit tests) ·
`crates/core/tests/differential_screen.rs` · ten bindings in
`.tracelean/drt.json`.

All 14 clauses hold `L3` on the model↔implementation bond — 20,000 generated
cases, no divergence — and two also hold `L4`: `stations_constant` and
`show_keeps_opened`/`open_twice_holds_one`. The project's own checker reports
one finding, the `Imprecise` in `Command.lean` that was there before. The
requirement↔model bond is a person's; the advice is in
`.tracelean/judging/REQ-SCREEN.md` and reports two clauses as drift.

### What the laws found that the comparison could not

Worth recording, because it is the argument for this methodology stated as an
event rather than a claim. All ten differential runs passed on the first
attempt. Then the same generated cases were put through the *laws* the clauses
state — not "do the two implementations agree", but "is what they agree on
true" — and three of them failed:

1. **`place` left a hole.** A split holding a part that places nothing — a split
   of no parts — was handed a share of the region and produced no rectangle, so
   the region was not covered. Both implementations had the same hole, so they
   agreed perfectly and the run was green. Fixed by weighting a part by what it
   will place.
2. **`split` hit every pane of a name, not one.** Two panes may carry the same
   identity (`panes_are_distinct` is a clause, not an invariant of the type), and
   splitting both added two panes for one request and gave both new halves the
   same minted name. Fixed by splitting the first match only.
3. **`focus_follows_geometry` has no answer under duplicate names**, which is
   not a bug but a precondition — recorded as one in the suite rather than
   asserted away.

A differential run establishes that two implementations agree. It cannot
establish that what they agree on is the clause, and two implementations written
from the same misunderstanding agree beautifully. That is what the property
assertions in the generation tests are for, and it is why they are worth the
lines.

## What phase B produced

The screen is drawn. `Space w` opens the screen menu: `v` and `s` split, `q`
closes, `hjkl` move the focus, `+` and `-` resize, `o` shows the buffer under
the cursor, `t` opens the station under it.

- **`Intent::Arrange`** (`surface/act.rs`, `Act.lean`) carries a named change
  rather than performing one, and `Screen.arrange` is the single function that
  performs it — `one_arrangement_path`, the sixteenth clause, added because this
  is exactly where a key and a pointer would otherwise grow separate arithmetic.
  `screen.station` resolves to a `Display` of the kind that station stands for,
  so producing a station's buffer needs no new machinery — it is the producer
  path that already exists.
- **`Editor` holds a `Screen`** and `screen.opened` is the only store of
  buffers: `buffer()` reads the focused pane's out of it rather than keeping a
  copy beside it. Each pane keeps its own cursor and first line, parked when the
  focus leaves and taken back out when it returns.
- **`Editor::laid_out(region)`** answers with every pane, where it goes, what it
  shows and where it is scrolled to — with the buffer *whole*. A frontend
  reserves its own chrome (a terminal spends a column on a divider; a window
  spends none) and windows the buffer to what is left by calling the core's
  `window`. Only the frontend knows that number; the windowing is still the
  core's function.
- **The terminal** composes the panes into one grid of `(character, role)`
  cells, so colour survives composition, and paints or reads that one grid.
- **The window** positions each pane in `ch` and line heights — the units the
  editor laid it out in — and `grab` sends a pointer's focus or drag through the
  same `arrange` a key reaches.

`REQ-SCREEN` now stands at fifteen clauses with `L3` and two of those also with
`L4`; `station_produces_a_buffer` is the one left, and it is the shell's work.

### The join nothing else crosses

Every part of the arrangement is modelled and differentially tested, and all of
it would still hold of a frontend that laid two panes out and drew one. So the
test for this phase drives the real binary on a real pseudo-terminal, presses
`Space w v`, and reads the divider off the screen — and a second one presses
`Space w q` and checks it is gone again. Both were watched failing with the
divider drawing removed before being kept.

## What phase C produced

Both bars are on the screen from the first frame: the stations, then what is
opened. `Space w t` opens the stations (`p`, `s`, `r`, `d`); `Space w b` shows
the opened set as a buffer you can put the cursor in, which is how `Space w o`
reaches a target from a bare keyboard.

**A station is an action of its own** — `screen.station.project` and its three
neighbours — rather than one `screen.station` taking the station's name as a
target. A target comes from what the cursor is on, so a single action would have
been reachable only when a station row was already under the cursor, which is
only true once you have got to the stations, which is what the action was for.
The rows carry their own action, so a key and a click reach the same one.

**A click on a bar is resolved against the bar.** `focus_in(buffer, offset)`
moved out of the editor for this: the bars are not the focused pane, and
resolving a click there against the pane would have done something — just not
the thing clicked.

### What the station found

The requirements station opened, and showed the whole tree. `produce` matched
`BufferKind::Directory { .. }` and threw the path away, so every directory ever
asked for answered with the same listing. Nothing could have noticed while only
one directory was ever asked for, and `listing_from_entries` held of the
function that was called — it was the caller that never varied. Fixed:
`listing_of(path)` lists what is under the path it was given.

### Icons, and the rule that did not have to change

The design above proposed amending `screen_is_readable` so a frontend could draw
a symbol in place of a piece's text. It turned out not to be needed here: the
emblem sits **beside** the station's name, not instead of it, and is
`aria-hidden`, so the page still contains the buffer's text verbatim and no
clause moves.

Replacing the text with the emblem — the fully iconic bar — is phase D, and it
is phase D precisely because that is the version that needs the rule changed
first.

## What phase D produced

The rule changed, and the bar is iconic. `REQ-VIEW` gained
`presentation_may_be_symbolic` and `screen_is_readable` now says *accessible*
text. The window paints 📁 🧪 📋 🕸 and no words; the terminal paints the words,
because it has no glyphs and `rendering_is_total` says a frontend without them
draws the text rather than nothing.

**The line moved from painted to named.** A region may be painted as anything
so long as it carries the buffer's own text as its accessible name, and a
screen is read by those names. `Presented { painted, name }` ·
`accessible(row)` · `presentedConformance`. A screen reader announces the same
string the capture harness reads, so the check and the person are one guarantee
and not two.

`a_symbol_is_read_as_its_name` is the clause as a theorem: repaint every region
with the same glyph and no verdict changes. That is the permission stated so it
cannot hold by accident — what a frontend paints is outside the check, and the
names are the whole of it.

**The emblem is keyed by action, not by role.** The draft above said role →
symbol. A role cannot distinguish four stations — they are all one role — so
that mapping would have given all four the same glyph. What does distinguish
them and is already declared, already dispatched and already governed by
`frontend_adds_nothing` is the **action**: `screen.station.project` → 📁. A
window mapping an action to a glyph is doing what the terminal does when it
maps one to a key. The rejected alternative stands and is now actually avoided:
the previous bar keyed its emblem off `piece.text.split(" ")[0]`, which was the
frontend pattern-matching text it recognised, wearing a comment that said
otherwise.

**One decision, two consumers.** `shown(buffer)` says what is painted and what
it is named; the page builds its DOM from it and `screenText` is it joined. A
frontend that painted one thing and reported another would have to disagree
with a value it computed once.

**The control.** `web/src/frontend.ts --mislabel` names every painted region
after its own glyph, which is exactly the mistake the amendment leaves room
for, and the capture harness fails it. Without that the permission would be
unfalsifiable: a harness that had only ever read correct names would pass a
frontend that returned the buffer's text no matter what it drew.

**What the floors caught.** The first generator drew `painted` and `name`
independently from wide alphabets, so two of them were almost never equal and
the ordinary case — a region with no emblem, painted as the word it is —
appeared 33 times in 3,000. The generator was fixed rather than the floor:
small alphabets, `max_len: Some(0)`, names a subset of the paintings. 352 now.

Still a person's: whether a glyph *means* its name. `🗑` where the buffer says
`project` is conformant — its name is the text — and nothing machine-checkable
says otherwise. That is the L2 rung, and the advice in
`.tracelean/judging/REQ-VIEW.md` says so rather than letting it pass unnoticed.

## What phase E produced

All four stations open something of their own. `station_produces_a_buffer` is
no longer `Unmodeled`: `stationKind` is the one function that says what a
station stands for, `Act` dispatches through it, and
`every_station_produces` is the theorem that every row on the bar leads
somewhere. A station drawn on the screen that does nothing when pressed is the
worse of the two failures available, and it is now the one that cannot happen.

**`Role::Level` carries its grade.** It was one role for any level, so a
frontend had to read `L3` out of the text to colour it — which is a frontend
parsing the buffer, the one thing `REQ-VIEW` forbids. Now the span says which,
and both frontends paint the four apart from the same value: red for an
annotation nobody checked, yellow for a person's word, green for a differential
run, bright cyan for a theorem. The terminal and the window agree because
neither decided.

**Two producers, two clauses.** `index_from_requirements` is the requirement
set, one row each, carrying the level its evidence reached.
`graph_from_refinement` is the same set drawn indented from the roots. Both are
`REQ-SHOW` producers for the reason all the others are: a window that knew what
a requirement index looks like and a terminal that did not would be two
editors.

**What the payload broke, and how it was caught.** A role is now an object in
the page's type, and `===` on an object compares identity — so the obvious
comparison would cut every character of a graded span into its own region, draw
exactly the same text, and pass every check that existed. The text cannot show
it. So `web/src/frontend.ts --regions` answers with how many regions each line
was cut into, and one test asks for two adjacent spans of the same grade and
requires one region. It was watched failing with the comparison broken.

## What phase F produced

The sandbox station shows what the tool changed, what it said, and what its
usage is estimated to have cost — in that order, which is the order of trust:
the workspace diff is what happened, the transcript is the tool's account of
it, the estimate is a reading of that account against a table we keep by hand.

TraceLean still launches nothing. `observe::watch` is a shell of two reads and
no decisions; everything after it is a function of the bytes it handed over.

**`REQ-COST`, four clauses.** Spend comes from usage and a price table and
nothing else; a price is looked up by model and an unknown model is reported
*unpriced* rather than priced at zero; cached input, cache writes and ordinary
input are priced separately; and the figure is rendered as an estimate, never
as an amount billed. All four hold `L3`, and `estimate_is_labelled` also holds
`L4`.

Arithmetic over naturals — prices per million tokens, amounts in millionths of
a unit — because a floating-point total would make two implementations disagree
in the last digit for reasons that have nothing to do with pricing.

**The transcript reader learned to count.** It reported events, unrecognised
lines and a held remainder; a tool's accounting records carry no text, so every
one of them was landing in `unrecognised`. That was true while nothing here
knew what usage was and misleading afterwards. `Read` now carries `usage`, and
a record reporting usage is recognised whether or not a person could read it.

**Still nowhere to send anything.** The reader's whole output is events,
unrecognised lines, a remainder and usage. There is no field a command could
come out of, which is `REQ-OBS.no_instruction_channel` stated as an absence —
and an absence is what can be checked.

## What the staleness mark caught

Not part of any phase, and the most useful thing found in it. The shared Lean
and Rust differential runners are built once and reused across fifty-eight test
processes, guarded by a mark recording what they were built from. The mark held
the *bindings* — which ops, which functions, which arguments — and not the
sources.

So editing a model and running the suites compared a new implementation against
a runner built from the old one. It did not error; it disagreed, which is
worse. And on the implementation side it was worse again: two stale runners
that both fail to read a value whose shape has changed *agree*, because two
runners failing identically is agreement — so the suite went green while
neither side had been asked the question.

Found the day `Role` gained a payload, when every TypeScript suite reported the
model failing to parse a role the model itself now emits. The mark now carries
a digest of every source the runner would be built from. A project whose whole
subject is stale links had one in its own harness.

A second, smaller thing fell out of it: the reduction that shrinks a diverging
case numbered its cases from `u64::MAX / 2`, which a JavaScript runner cannot
echo — its numbers are doubles and integers above 2^53 do not survive the round
trip. Every divergence against the TypeScript side came back as a protocol
error instead of a counterexample: the reduction failing exactly when it was
needed. It counts from 2^50 now.

## Settled: the Project station opens a buffer, not a dialog

In v0, `MenuBar.tsx` called Tauri's dialog plugin and the operating system's
folder picker appeared. That is not what this does.

Pressing Project opens a `Directory` buffer at the parent of the current root,
navigable with the same keys as anything else, and a row carries `choose`. The
way Emacs opens a file: inside the editor, with the editor's own movement, not
by handing the job to a window the editor cannot see into.

It is also the only version that works twice. A native dialog is an effect the
core cannot perform (`ARCH-CORE-SHELL`) and a terminal has no dialog at all, so
that station would work in one frontend and need a second answer for the other
— the asymmetry that left the old TUI unmaintained. A directory buffer is a
value, so it is modelled, differentially tested, and identical in both.

**No native picker is planned, here or later.**
