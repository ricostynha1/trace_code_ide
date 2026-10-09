# Judging advice — REQ-SCREEN

Produced by an assistant asked to read each clause against its model. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**: nothing
here is evidence until a person enters the verdict with `--judge … --verdict …
--by <name>`, and the record is then theirs.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-SCREEN.<clause>`.

| Clause | Advice |
|---|---|
| `screen_is_a_value` | agrees, with a note |
| `opened_outlives_shown` | agrees |
| `layout_tiles_the_region` | agrees, with a note |
| `every_pane_is_opened` | agrees |
| `panes_are_distinct` | agrees |
| `split_keeps_the_buffer` | agrees |
| `close_collapses_the_pane` | agrees — the drift below was accepted and the clause reworded |
| `resize_moves_one_divider` | agrees |
| `resize_has_a_floor` | agrees, with a note |
| `focus_is_placed` | agrees |
| `focus_follows_geometry` | agrees, with a note |
| `strip_is_the_opened_set` | agrees |
| `stations_are_constant` | agrees |
| `station_opens_a_buffer` | agrees — the clause was split in two |
| `station_produces_a_buffer` | not modelled; the shell's half, not yet built |

## drift — both accepted

Recorded as they were reported, with what was decided. Both decisions were the
project owner's; neither was taken by the side being judged.

**`close_collapses_the_pane`** — "Closing shall drop the buffer from the opened
set and give its region to the rest of the layout, **leaving no pane without a
buffer**."

The emphasised half cannot be false. `Layout.pane` carries a buffer as a field,
so a pane without one is not representable and no implementation could produce
it. A clause that no run could fail is not a law; it is a fact about the type
wearing a requirement's clothes.

What `closeBuffer` actually guarantees, and what the suite checks, is the
neighbouring and falsifiable thing: **no region is left without a pane**. That
is a real obligation — `without` can empty a split, and the function has to
answer with something a frontend can draw rather than an empty layout. It is
also the state the function refuses twice over: closing the last opened buffer
answers unchanged, and closing one that filled every pane keeps the next opened
buffer on screen.

Suggested rewording, for the person who owns this bond to accept or reject:

> Closing shall drop the buffer from the opened set and give its region to the
> rest of the layout, leaving no region without a pane.

**Accepted.** The clause now reads "leaving no region without a pane", and the
records that named the old wording went stale on the next lock, which is the
mechanism working rather than a loss.

**`station_opens_a_buffer`** — "Pressing a station shall **produce** its buffer
and show it in the focused pane, and shall change the layout in no other way."

The model covers the second half and not the first. `openBuffer` is handed a
buffer that already exists and decides where it goes; producing it — reading the
project tree, tailing a transcript, rendering the refinement graph — is the
shell's work under `ARCH-CORE-SHELL`, and none of it exists yet (phases E and F
of `investigations/screen-proposal.md`).

**Accepted, by splitting.** The project's own rule decides it: one claim per
clause, and a sentence whose "and" joins two independent obligations is two
clauses. So `station_produces_a_buffer` is the shell's half and reads as
outstanding work until the producers exist, and `station_opens_a_buffer` is the
half that is a value and carries the evidence. The alternative — `@partial` on
one clause — would have kept a single grade over two obligations of very
different maturity, which is the averaging the method refuses elsewhere.

## agrees, with a note

**`screen_is_a_value`** — "What is on screen shall be one value." Discharged by
the `Screen` structure rather than by a computation: the clause is a claim about
the *shape* of the state, and the type is what makes it true. `coherent` is what
makes it checkable, but `coherent` is really the conjunction of the three
clauses below it. Consider whether this is a `@structural` clause in the sense
`ADR-0012` means — it is about the tree of types rather than about a value — in
which case it caps at L2 and the L3 it currently carries is generous.

**`layout_tiles_the_region`** — "…shall cover the region without gap or
overlap." `place` does this, and it did not before: a split holding a part that
places nothing was handed a share of the region and produced no rectangle,
leaving a hole. Both implementations had the same hole, so the differential run
agreed perfectly and reported nothing; what found it was asserting the tiling
itself over the generated regions, in
`crates/core/tests/differential_screen.rs::tiles`.

The note is that the law is checked in Rust rather than proved in Lean. A
theorem — the shares of a split sum to the extent, so the placements tile by
induction — would earn the proof bond and is the obvious next piece of work. The
judgement asked of you here is only whether `place` *means* the sentence.

**`resize_has_a_floor`** — the floor is on the **weight**, not on the characters
a pane ends up with. A region too narrow to give every pane a character still
produces panes of no width, and the body says so on purpose: that is a small
screen, not a resize. Confirm that is what the sentence was meant to say, since
"below a weight of one" reads as an implementation's unit rather than a person's.

**`focus_follows_geometry`** — "adjacent in that direction" is rendered as
*nearest wholly beyond, overlapping across the direction travelled*, with ties
broken in reading order. The sentence does not say what to do about ties and the
function must, so a decision was taken where the requirement was silent. Confirm
it, or say what the tie should do instead.

## agrees

**`opened_outlives_shown`** — proved twice: `show_keeps_opened` says showing
changes no buffer the session holds, and `open_twice_holds_one` says opening
something already held keeps what was held rather than replacing it with what
was offered. That second theorem is the clause's real content — the difference
between a buffer and a panel rebuilt whenever it becomes visible.

**`every_pane_is_opened`**, **`panes_are_distinct`**, **`focus_is_placed`** —
the three conjuncts of `coherent`, each computed directly and none of them a
restatement: a screen can fail any one of them and the generator reaches all
three (800, 250 and 800 cases respectively out of 2,000).

**`split_keeps_the_buffer`** — `splitFocus` puts the pane's buffer in both
halves, and the suite asserts the count of panes showing it rose. It splits the
*first* pane of that name rather than every one, which matters because
`panes_are_distinct` is a clause and not an invariant; two panes of one name
would otherwise become four.

**`resize_moves_one_divider`** — weight leaves one side and arrives at the
other: the suite asserts the total is unchanged and that at most two parts
differ, so a third pane cannot have drifted.

**`strip_is_the_opened_set`** — a rendering of `opened`, not a list kept beside
it. The suite asserts one row per opened buffer, in order, each naming its
buffer and carrying `screen.show`.

**`stations_are_constant`** — proved: `stations_constant` is `rfl`, because the
argument is taken and ignored. The clause's point is that the station that opens
a project must be reachable when no project is open, and a constant function is
the strongest possible form of that.

## Independent review — 2026-09-20

Second opinion, produced by a reviewer who did not write these models. Advice,
not a record (`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `screen_is_a_value` | agrees |
| `opened_outlives_shown` | agrees |
| `layout_tiles_the_region` | agrees |
| `every_pane_is_opened` | agrees |
| `panes_are_distinct` | drift |
| `split_keeps_the_buffer` | agrees |
| `close_collapses_the_pane` | drift |
| `resize_moves_one_divider` | agrees |
| `resize_has_a_floor` | drift |
| `focus_is_placed` | agrees |
| `one_arrangement_path` | unmodelable |
| `focus_follows_geometry` | agrees |
| `strip_is_the_opened_set` | agrees |
| `stations_are_constant` | agrees |
| `station_produces_a_buffer` | drift |
| `station_opens_a_buffer` | agrees |

### panes_are_distinct — drift

`distinctPanes` renders the sentence exactly. The drift is the *other*
annotation: `splitFocus` also carries `@models REQ-SCREEN.panes_are_distinct`,
and it can mint a colliding identity.

Input: `opened = [b]`, `layout = pane "pane0" b`, `focus = "pane0"`,
`nextPane = 0`. This screen is `coherent`. `splitFocus across` computes
`fresh = "pane" ++ toString 0 = "pane0"` and answers

    split across [(1, pane "pane0" b), (1, pane "pane0" b)]

Two panes, one identity. The clause demands none; the model produced one.

The model never states the invariant that makes the mint safe — that `nextPane`
exceeds every `paneN` already placed. `coherent` says nothing about `nextPane`,
so a screen satisfying every stated invariant still breaks the clause one split
later. The requirement body ties identity-minting to `ARCH-DETERMINISM` and to
the counter, but no clause states the counter's own obligation.

Nothing could catch it. `splitFocus` and `split_focus` mint the same way, so the
differential run agrees; the generator draws pane ids from `"a"`, `"b"`, `"c"`
and `""` (`differential_screen.rs::pane_id`), so `"pane0"` never appears as an
existing id and the colliding case is unreachable by construction. The one
assertion that a split keeps panes distinct is a hand-built unit test
(`screen.rs::splitting_keeps_the_buffer_in_both_halves`) whose screen is
`pane0` / `next_pane: 1` — precisely the state where collision cannot happen.
This is the same shape of hole as the `place` gap the model's prose records
finding: both implementations agree because both are wrong in the same way.

Either state the invariant (`nextPane` is above every placed `paneN`, and
`coherent` checks it), or mint an identity that cannot collide.

### close_collapses_the_pane — drift

"Closing shall drop the buffer from the opened set…" is unconditional. The model
has an exception.

Input: `opened = [b]`, `layout = pane p b`, `focus = p`; `closeBuffer b`.
`remaining = []`, so the match answers `screen` **unchanged** — `b` is still in
the opened set and still on screen. The clause says it is dropped.

The clause is unsatisfiable on that input: you cannot both drop the last opened
buffer and leave no region without a pane. So the model chose a horn where the
requirement contradicts itself. The fix is almost certainly words, not code —
say that closing the last opened buffer answers unchanged — but as written the
model does not say what the clause says.

### resize_has_a_floor — drift

"…and a resize that would [take a pane below one] shall leave it at one." The
model leaves it at zero when it was already at zero.

Input: `layout = split across [(5, pane a), (0, pane b)]`, `focus = a`,
`resizeFocus 3`. In `shiftAt`, `room := Int.ofNat (next.1 - 1)`; `next.1` is `0`
and Nat subtraction truncates, so `room = 0` and `moved = 0`. Nothing moves and
`b` stays at weight `0`. The clause says the resize should leave `b` at one.

The Rust suite encodes the same exception explicitly — `assert!(*before == 0 ||
*after >= 1)` in `generation_reaches_the_floor_and_the_far_side_of_a_split` — so
implementation, model and test agree with one another and all three differ from
the sentence. Zero weights are not an accident of the generator either: `shares`
has a written rule for them and the schema seeds `0` among the weights.

Two smaller notes on the same clause. The floor is on a *part*, and a part may
be a nested split holding many panes; "a pane['s] weight" is not a thing the
model has. And the previous reviewer's point stands: confirm the floor was meant
to be on the weight rather than on the characters.

### one_arrangement_path — unmodelable

"Every change to the arrangement shall go through one function" is a claim about
*call sites*. No argument to `arrange` can falsify it: a frontend that computed
its own weights would violate the clause while `arrange` stayed exactly as it
is. A value-to-value function can offer the one path; it cannot assert that it
is the only one. This wants an architectural check (no caller outside `arrange`
reaches `splitFocus`, `closeBuffer`, `resizeFocus`, `focusStep`, `showBuffer`),
of the kind `tests/architecture.rs` already does elsewhere.

The differential test makes this visible: it asserts `arrange(how, …)` equals
calling the underlying function directly — which is `arrange`'s own body
restated, true for any dispatcher whatsoever.

And read charitably as "there is one such function", the model still falls
short: **`openBuffer` changes the layout and is not reachable through
`arrange`**. `Arrangement` has no variant for opening a buffer that is not yet
opened, and `openBuffer` calls `showBuffer`, which calls `setBuffer` on the
layout. So the model's own `station_opens_a_buffer` path is by construction a
second way in. Either `Arrangement` gains an `open` variant, or the clause
should say which changes it governs.

### station_produces_a_buffer — drift

The clause: "Pressing a station shall produce the buffer that station stands
for." The model: `stationKind : String → Option BufferKind`.

Two concrete gaps.

*A kind is not a buffer.* Press `requirements`. The clause demands the
requirement index — its text and its spans. `stationKind` answers
`BufferKind.menu "requirements"`, a tag with no text and no spans, and no
modelled function carries that tag to a `Buffer`. For `project` the gap is not
closable by a pure function at all: the buffer is a reading of the project tree
on disk, and `stationKind` is `String → Option BufferKind` with nothing to read
it from. (It also hardcodes `"."`, so the project station stands for the working
directory whatever is open.)

*Nothing about pressing.* No function takes a station press to a screen.
`arrange` has no station variant; the shell path
(`act.rs::"screen.station."` → `Intent::Display { what }`) hands a *kind* on and
leaves production to somebody else.

So the model says something narrower and different: which of four names is a
station, and what kind of thing each stands for. That is worth having, but it is
a name→kind map, not production.

The theorem is also weaker than it looks. `every_station_produces` says the four
keys of `stationEntries` all map to `some`. Both lists are literals a few lines
apart in one file; the theorem is a consistency check between two constants,
failing only if someone edits one and not the other. It cannot fail for any
input, because there is no input.

The honest reading is that this clause is still outstanding work — which is what
the requirement body already says — and that `stationKind` should be bound to a
clause about what a station *stands for* rather than to this one.

### Notes on clauses that agree

**`screen_is_a_value`** — agrees, but it is not a law. `Screen` has the three
fields, so no value can fail it and no run can. Contrast `every_pane_is_opened`
and `focus_is_placed`, which the prompt groups with it: those two *are*
falsifiable — `Screen` values exist with a pane showing an unopened buffer, or a
focus naming no pane, and `coherent` answers `false` for them. The difference is
real, and `screen_is_a_value` is the structural one. Second the previous
reviewer's `@structural` / L2 suggestion. Note also that `--judge` shows
`Layout` for this clause; `Screen` carries the same annotation and is the half
the sentence is actually about.

**`every_pane_is_opened`, `focus_is_placed`** — agree as predicates, but the
model proves no function preserves them. `coherent` is stated and never
discharged: there is no theorem that `openBuffer`, `closeBuffer`, `splitFocus`
or `arrange` carry a coherent screen to a coherent one. Checking them by hand,
they do — except `splitFocus`, per the drift above. A preservation theorem would
have caught it.

**`layout_tiles_the_region`** — `place` has the property; the model does not
*say* it. The tiling law appears only as Rust
(`differential_screen.rs::tiles`). Worth knowing that a pane of weight `0`
beside a non-zero one gets a rectangle of zero extent — it is given a rectangle
and the region is still covered, so the sentence holds, but "gives every pane a
rectangle" is met degenerately there.

**`resize_moves_one_divider`** — agrees on the letter: at most two parts change
and the total is preserved. Which divider, though, is a decision the clause does
not make. `resizeAt` shifts at the *outermost* split holding the focus, so with
`split across [(2, split down [A, B]), (3, C)]` and focus `A`, resizing moves
the divider between the column and `C` — never the one between `A` and `B`. The
clause says "one divider" without saying which. Confirm the outermost is meant.

**`strip_is_the_opened_set`** — agrees, with the reading that "the action that
shows it" means the action, not an action naming the buffer. Every row carries
the bare `screen.show`; which buffer it means comes from the cursor
(`REQ-ACT.focus_is_carried`). Fine, and the docstring says so — but the clause's
"shows **it**" reads as though the row identified its own buffer.

**`stations_are_constant`** — agrees. Half the sentence ("produced for every
state") is free: every Lean function is total. The content is constancy, and
that half is properly proved.

**`station_opens_a_buffer`** — agrees. `setBuffer` rewrites only the pane whose
id matches the focus and rebuilds every other part unchanged. Two edges the
clause does not cover: a focus naming no pane opens the buffer without showing
it anywhere, and if two panes share the focus's identity — reachable via the
`panes_are_distinct` drift — `setBuffer` changes both, which is "another way"
the layout changed.

### Prose obligations with no clause

Three sentences in the body state obligations the clause keys do not carry.

- "the icon's accessible name is the row's own text" — a frontend obligation,
  and probably `REQ-VIEW`'s to state, but nothing here or there keys it.
- "minting one stays a function of the value (`ARCH-DETERMINISM`)" — the
  counter's obligation, and exactly where the `panes_are_distinct` drift lives.
- "a drag is converted to an amount before it arrives rather than after" — the
  shell's half of `one_arrangement_path`, unkeyed and unmodelled.

## Review by claude-review, simulating the human approver (2026-10-09)

claude-review produced this review, simulating the human approver. It
supersedes the advice above for every clause it lists. The verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.
Advice, not a record (`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `screen_is_a_value` | agrees |
| `opened_outlives_shown` | agrees |
| `layout_tiles_the_region` | agrees |
| `every_pane_is_opened` | agrees |
| `panes_are_distinct` | agrees, with a note |
| `split_keeps_the_buffer` | agrees |
| `close_collapses_the_pane` | drift |
| `resize_moves_one_divider` | agrees, with a note |
| `resize_has_a_floor` | drift |
| `focus_is_placed` | agrees |
| `one_arrangement_path` | unmodelable |
| `focus_follows_geometry` | agrees |
| `strip_is_the_opened_set` | drift |
| `stations_are_constant` | agrees |
| `station_produces_a_buffer` | drift |
| `station_opens_a_buffer` | agrees |
| `workbench_has_three_places` | agrees |
| `buffer_goes_home` | drift |

### `strip_is_the_opened_set`: drift (new)

The rows are the opened buffers, in order. The action a row carries does not
show the row's buffer, though. Each row carries the bare `screen.show`, and
`dispatch` resolves it through `showUnder` from `focus.under`, which is the
**text of the row span**: `menuLine` gives `"1  a.rs"`, a key and a title. That
text is not a buffer id. So `dispatch "screen.show" {kind := menu "opened",
under := some "1  a.rs"}` is `arrange (showBuffer "1  a.rs")`, and
`showBuffer` answers the screen unchanged because no opened buffer has that id.
The editor works because it bypasses `dispatch`: `Editor::act_in_bar`
special-cases `screen.show` and maps the row number to `screen.opened[row]`
(`opened_at_row`). That bypass is also a second path for an action
(`REQ-ACT.one_path`). Fix it in the model: put the buffer id where `under`
reads it, or give each row an action that names its buffer.

### `buffer_goes_home`: drift (new)

The clause says "a menu or a record in the side panel". `homeOf` sends a record
to the **document** pane when its title starts with any entry in
`documentRecords` (`requirement `, `judge `, `context `, `definitions of `,
`uses of `, `search `, `keys`). For example, `record "requirement REQ-X"` goes
to `document`. The choice is reasonable, since such records read like files,
but the sentence does not say it. Change the clause to name these records, or
change the model.

### `close_collapses_the_pane`: drift (still)

The rewording accepted earlier fixed the region half. It did not fix the
last-buffer case. With `opened = [b]` and `layout = pane p b`, `closeBuffer b`
answers the screen unchanged, so `b` stays opened, while the clause makes
dropping it unconditional. The body does not mention the exception either. Add
"except the last opened buffer, which stays" to the clause.

### `resize_has_a_floor`: drift (still)

Take `split across [(5, a), (0, b)]` with focus on `a`, and apply
`resizeFocus 3`. `room = Int.ofNat (0 - 1) = 0`, so `b` stays at `0`. The clause
says a resize that would take a pane below one leaves it at one. The model only
refuses to *move* weight out of a part at or below one. It never restores one.
A resize cannot reach 0 from the shipped screens (workbench 1/2/1, split 1/1),
so this is about starting states. Either state that weights are at least one
as an invariant (`coherent` does not check it), or reword the clause as "shall
not take weight from a part at one".

### `station_produces_a_buffer`: drift (still)

`stationKind` maps a name to a `BufferKind` tag: `requirements → menu
"requirements"`, and `project → directory "."` whatever project is open. No
modelled function turns that tag into the buffer's text and spans, so the model
does not show that pressing a station produces the buffer the station stands
for. `every_station_produces` checks two literal lists against each other and
has no input that could make it fail. The body also still says "Four entries"
while `stationEntries` now has six (`trace` and `history` were added).

### `one_arrangement_path`: unmodelable (still)

The clause claims that every change to the arrangement goes through `arrange`.
That depends on call sites, and no argument to `arrange` can falsify it.
`openBuffer` changes the layout without going through `Arrangement`, and
`buffer_goes_home` (focus, then show) happens outside it too.

### Notes on clauses that agree

**`panes_are_distinct`**: the drift from the previous review is fixed.
`splitFocus` now mints with `freeFrom`, which skips every `paneN` already
placed. With `fuel = placed + 1`, among that many consecutive candidates at
least one is free, so the unchecked `fuel = 0` branch is unreachable. A split of
a distinct layout stays distinct. A layout that already has duplicates is not
repaired. The clause does not ask for repair.

**`resize_moves_one_divider`**: the divider is the outermost one beside the
focus. With `split across [(2, split down [A, B]), (3, C)]` and focus on `A`,
weight moves between the column and `C`. The clause leaves this open, so
confirm it is what is meant.

**`focus_follows_geometry`**: in a tiling, "nearest pane wholly beyond and
overlapping across" is the adjacent pane. Ties are broken in reading order,
which the clause does not specify.

**`screen_is_a_value`**: structural, and true by the `Screen` type. The
previous `@structural`/L2 suggestion still applies.

**`station_opens_a_buffer`**: a focus that names no pane opens the buffer
without showing it anywhere. `focus_is_placed` excludes that state.

### Evidence that survives this review

Before this review, `resize_has_a_floor` and `strip_is_the_opened_set` each had
an `agrees` record by `independent-review-agent` (2026-09-21). Both are stale
("the requirement changed"). A drift verdict records no evidence, so those
files remain and `--stale` still asks for them to be redone.
