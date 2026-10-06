# Judging advice — REQ-SHOW

Produced by an assistant asked to read a clause against its model. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**: nothing
here is evidence until a person enters the verdict with `--judge … --verdict …
--by <name>`, and the record is then theirs.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-SHOW.<clause>`.

Only the three clauses the stations added are covered. The other ten have no
advice yet, which is an absence and not a pass.

| Clause | Advice |
|---|---|
| `index_from_requirements` | agrees |
| `graph_from_refinement` | **drift** — see below |
| `sandbox_from_observation` | agrees, with a note |

## drift — `graph_from_refinement`

> The refinement relation shall become a buffer whose text is that relation
> drawn as an indented graph, and a requirement that refines something shall
> appear under it.

The second half is false of `designBuffer`, and knowingly so. The walk starts
at the roots — requirements that refine nothing — and descends four levels. A
requirement that refines something unreachable from any root appears nowhere,
and a requirement at the fifth level appears without its children. Both are
states the generator reaches often: `a requirement no root reaches, drawn
nowhere` is a declared situation with a floor of 970 in 3,000.

The bound is not the drift — a bound is necessary, because `refines` is data
and a cycle in it is representable, so an unbounded walk would not be a
function. The drift is that the clause promises something the bound makes
untrue, and a clause that the implementation is *allowed* to fail is not a
clause.

Suggested rewording, for the person who owns this bond to accept or reject:

> The refinement relation shall become a buffer whose text is that relation
> drawn as an indented graph from its roots, to a stated depth, and a
> requirement drawn under another shall be one that refines it.

That is falsifiable in the direction that matters — nothing appears under
something it does not refine — and it stops claiming the completeness the walk
does not provide. What it gives up is the guarantee that everything appears at
all, which is a real loss and is the reason this is a person's decision rather
than an edit.

## `sandbox_from_observation`

*Note — "in that order" is the whole of the clause, and it is the part worth
reading twice.* The order is the order of trust: what the workspace changed is
what happened, what the tool said is its account of it, what it cost is a
reading of that account against a table. A judge should satisfy themselves that
this is an ordering anybody would infer from the clause text, because the model
fixes it and nothing else states why.

## Independent review — 2026-09-20

Second opinion, produced by a reviewer who did not write these models. Advice,
not a record (`REQ-JUDGE.advice_is_not_evidence`).

All thirteen clauses, read against `formal/TraceLean/Produce.lean` and
`formal/TraceLean/View.lean`.

| Clause | Verdict |
|---|---|
| `core_produces` | unmodelable |
| `file_from_text` | agrees |
| `listing_from_entries` | agrees |
| `review_from_change` | drift |
| `menu_from_keymap` | agrees |
| `record_from_events` | agrees |
| `index_from_requirements` | agrees |
| `graph_from_refinement` | drift |
| `sandbox_from_observation` | agrees |
| `producers_are_total` | drift |
| `producer_is_pure` | unmodelable |
| `well_formed_by_construction` | agrees |
| `window_is_a_buffer` | agrees |

### `core_produces` — unmodelable

> Every buffer shall be produced by the core; a frontend shall construct none.

This is a claim about where code lives, not about values. No function from a
state to a buffer can say that no *other* function anywhere returns a `Buffer`.
The requirement body already concedes it — "checkable by looking" — so the
obligation belongs to a repository check, not to Lean.

The bound model is `actionsFor`, which is a role-to-affordances table. Its own
docstring says what it is for ("one place, so a path in a listing and a path in
a message offer the same thing") and that is `REQ-VIEW.affordances_named`, not
this. The binding looks like a place to hang a clause that had nowhere to go. A
person should decide whether to drop the `@models` and carry the clause by a
structural check instead.

### `review_from_change` — drift

> A change between two workspace states shall become a buffer whose text shows
> what changed and whose spans mark what was added and what was removed.

The model's input is not a workspace state. `reviewBuffer (target : String)
(before after : String)` takes two texts of **one** file.

Concretely: an agent edits `a.rs` and `b.rs`. The clause says that change
becomes *a* buffer showing what changed. The model has no function from a set
of files to a review buffer — only one per file, with no way to fold them
together. `Role.heading` is documented in `View.lean` as "a file name at the top
of a diff", and no review producer ever emits it, which reads as the multi-file
case having been intended and not built. The place a multi-file change actually
lands in the model is `sandboxEvents`, as a list of bare paths with no diff.

Two ways out, both a person's call: narrow the clause to "a change to a file
between two workspace states", or add a producer that folds per-file diffs under
heading rows. What is not sustainable is the present reading, where a two-file
change is inside the clause and outside the model.

Diff behaviour itself is fine: `diffLines` keeps the common ends, marks removed
then added, and `lineSpans` gives spans to exactly those two roles.

### `graph_from_refinement` — drift

I reach the same verdict as the section above, independently, and on a sharper
input than an unreachable node.

`childrenOf` matches on `node.refines.contains parent` and the walk only ever
visits ids that are themselves nodes. So a requirement whose `refines` names an
id that does not exist — a typo, or a document deleted without its children
being updated — is not a root (its `refines` is non-empty) and is never anyone's
child. It appears in `designBuffer` nowhere at all. That is the ordinary
consequence of an ordinary edit, not a pathological state.

Depth: `expandAll 3` gives indents 0..3. A chain of five requirements loses the
fifth and everything under it, and the clause says the fifth shall appear under
the fourth.

Worth recording that neither failure is visible on this project today: the 43
requirements have 6 roots, no dangling `refines`, and a maximum depth of 2. The
bond being judged is requirement-to-model, and the model is a function over all
node lists, so the counterexamples stand — but a person should know that fixing
this changes no buffer they can currently look at.

On the suggested rewording: I would accept its direction and amend it twice.

1. "to a stated depth" states no depth. Under it, `expandAll 0` — roots only —
   still conforms. Name the number: "to four levels".
2. The completeness the rewording gives up is recoverable, and should be said
   out loud rather than lost. `requirementsBuffer` takes the whole node list and
   emits a row per node unconditionally, so `index_from_requirements` already
   guarantees that nothing disappears from the editor. The design station may
   cut its view short precisely because the index does not.

So: "The refinement relation shall become a buffer whose text is that relation
drawn as an indented graph from its roots, to four levels, and a requirement
drawn under another shall be one that refines it." That is false the moment
`childrenOf` stops filtering on `refines`, which is the direction that matters.

### `producers_are_total` — drift

> Every buffer kind shall have a producer, and a producer shall answer for every
> state it is given rather than failing on some.

Both halves fail to be said, for different reasons.

*First half.* Nothing in the model is indexed by `BufferKind`. Add a seventh
constructor to `BufferKind` with no producer behind it: the clause is violated,
and not one definition in `Produce.lean` changes, fails to compile, or reports
anything. The model cannot notice the thing the clause forbids. A single
`produce : BufferKind → State → Buffer` dispatch would make Lean's exhaustiveness
check do the work; five separate producers cannot.

*Second half.* Every Lean definition is total. "A producer shall answer for
every state" is therefore true of `tidy`, of `window`, and of any function
anybody writes in this file, including a wrong one. No state can falsify it, so
no run can fail it. The bound models say nothing the ambient logic did not
already give; `a_window_is_total` checks two concrete windows.

A reader who prefers to split them may reasonably call the second half
*unmodelable* on its own. I have entered one verdict because the clause is one
clause, and by silence on the first half it is drift either way.

### `producer_is_pure` — unmodelable

> A producer shall be a function of the state it is given and shall read nothing
> else.

There is no way to write a Lean value-to-value function that reads a disk, so
this is an ambient truth about every definition in the file rather than
something any of them asserts. `fileBuffer` satisfies it in the same way a
constant does. No generated state can fail it.

The clause is not empty — it is just that the whole of it bears on the Rust
side, where `produce.rs` could take a `&Path` and read it. It is checkable
there, by signature. On the model side there is nothing to check, and the eight
`@models producer_is_pure` annotations record no obligation.

### `well_formed_by_construction` — agrees, with two notes

The property is true. `tidy` clamps to `size`, sorts by start, then drops any
span that is empty, backwards, or begins before the previous one's stop — so
every surviving span has `start ≥ previous.stop`, `stop > start`, `stop ≤ size`,
which is exactly the negation of all four `Fault` constructors. Every producer
in `Produce.lean` ends with it, `sandboxBuffer` included by way of
`recordBuffer`.

Note one. The clause says "whatever state", and the model demonstrates it on
three literals under `native_decide` (`a_produced_file_is_well_formed`,
`a_review_is_well_formed`, and one window). The universal statement —
`∀ t s, faults { text := t, spans := tidy t.length s, .. } = []` — is provable
by induction on `keepsOrder` with the obvious strengthened hypothesis, and is
the clause itself rather than an instance of it. The requirement body pushes the
universality onto the generator; a ∀-theorem would be cheaper and stronger.

Note two. `directoryBuffer` is the one producer that does **not** go through
`tidy`: it lives in `View.lean` and builds `listingSpans` directly. The module
header of `Produce.lean` asserts "Everything goes through `tidy`, which is why a
produced buffer has no faults whatever it was produced from" — that sentence is
false of `directoryBuffer`, and `directoryBuffer` carries no
`@models well_formed_by_construction`. I checked it by hand and it is fault-free
anyway (each span sits at `at_+indent .. at_+line.length`, and the next starts a
full separator later), but it holds by arithmetic that nobody stated, not by the
argument the model gives. An entry with an empty name yields a zero-width span,
which `faults` does not report and `tidy` would have dropped.

### Notes on clauses that agree

`file_from_text` — the parse is not modelled; marks arrive as an argument, so
"whose spans come from parsing it" is assumed rather than said. Note also that
`tidy` silently *discards* a parser's overlapping or backwards mark and clamps
one that overruns. That is right for well-formedness and it means a file's spans
are a subset of what the parser found, which the clause does not mention.

`index_from_requirements` — agrees. Edge worth knowing: a node with an empty
`id` produces a zero-width identifier span, which `tidy` drops, so that row is
left with only its level marked. There is no identifier text there to mark, so I
do not read it as a failure.

`sandbox_from_observation` — agrees; the order is real and falsifiable, which is
more than several clauses here manage. Two things for a person:

- The order carries the trust argument, but the rows do not. `Event` is
  `kind × text`, and `said` events keep whatever kind the tool gave them. A
  transcript event with `kind := "changed"` renders identically to an observed
  workspace change, and one with `kind := "cost"` identically to the estimate.
  The prose says the account never decides anything; the buffer gives a frontend
  no way to tell the account from the observation except position.
- `sandboxBuffer` is `recordBuffer "sandbox"`, so its id is the constant
  `"record:sandbox"`. Two sandboxed agents share one buffer identity, and
  `REQ-VIEW.changes_are_deltas` keys on id.

`record_from_events` — agrees. Every event row is given
`actions := ["observe.diff"]` unconditionally, so the sandbox cost row offers a
diff of itself. Harmless, but it is an affordance the clause did not ask for.

`window_is_a_buffer` — agrees, and satisfied by the return type: `window`
returns a `Buffer`, so the first half cannot fail. The "so that" half — that a
frontend showing part of something still draws everything it was given — is
never connected: no theorem relates `window` to `conformance`, and none states
that a window's text is a contiguous run of `plainText buffer`. Those two are
the falsifiable content and neither is written. The clipping arithmetic is
correct as far as I traced it, including `count = 0` and a start past the last
line, both of which come out empty rather than wrong.

### Annotations that landed on the wrong declaration

Not verdicts, but they distort what the bond looks like from outside and are
cheap to fix.

- `Produce.lean:169-176` — the docstring reading "A change between two states of
  a file, as a buffer a person can accept or reject", with
  `@models review_from_change` and `@models producer_is_pure`, sits immediately
  above `private def lineText (line : DiffLine) : String := line.text`.
  `reviewBuffer`, six lines below, carries no annotation at all. The trace report
  confirms it: `review_from_change -> TraceLean.Produce::lineText`. The producer
  the clause is about is unbound on the model side, and a field projection is
  recorded as modelling it. The Rust side binds `review_buffer` correctly, so
  the two sides of the same clause name different functions.
- `Produce.lean:493-497` — `@proves REQ-SHOW.menu_from_keymap` sits above
  `private def threeLines : Buffer`, a constant. The theorem it was written for,
  `a_menu_offers_what_its_keys_dispatch`, is at line 518 and is unannotated. The
  report records `@proves menu_from_keymap -> threeLines`: a value, claimed as a
  proof.
