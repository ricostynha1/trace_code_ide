# Judging advice — REQ-VIEW

Produced by an assistant asked to read a clause against its model. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**: nothing
here is evidence until a person enters the verdict with `--judge … --verdict …
--by <name>`, and the record is then theirs.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-VIEW.<clause>`.

Only the two clauses the icon amendment touched are covered. The other ten have
no advice yet, which is an absence and not a pass.

| Clause | Advice |
|---|---|
| `screen_is_readable` | agrees, with a note |
| `presentation_may_be_symbolic` | agrees, with two notes |

## `screen_is_readable`

Reworded from "readable as the text it drew" to "readable as the **accessible**
text of what it drew". Modelled by `presentedConformance`, which reads a
frontend's rows by their names and then hands them to the same `conformance`
every other rendering goes through.

*Note.* The clause is about a live process and stays `@structural` on the
capture side: what a terminal or a browser actually emitted is not a value
either side computes. The model says what the reading *is*; the evidence that a
real frontend passes it comes from running one.

## `presentation_may_be_symbolic`

The clause is two obligations and the model answers them in two places, which
is worth a person seeing before they judge.

*Note one — the permission and the proviso are modelled separately.* The
permission — a frontend may paint what it likes, and the glyphs are never read
— is `accessible` together with `a_symbol_is_read_as_its_name`, which says
repainting every region with one glyph changes no verdict. The proviso — the
name must be the region's own text — is not in `accessible` at all; it is
`presentedConformance` failing when a name is not the buffer's. Neither half
alone is the clause. A person judging this should satisfy themselves that both
are present, because reading only the first would make the clause look like a
blanket permission.

*Note two — the model does not say the symbol is apt.* Nothing here stops a
frontend painting 🗑 where the buffer says `project`. That rendering is
conformant: its name is the text, and the harness reads the text. Whether a
glyph *means* its name is a judgement about how a thing looks, which is the L2
rung, and the requirement deliberately does not pretend otherwise. If the
project wants aptness to be an obligation rather than a taste, that is a
requirement change and it is a person's.

## Independent review — 2026-09-20

Second opinion, produced by a reviewer who did not write these models. Advice,
not a record (`REQ-JUDGE.advice_is_not_evidence`). All twelve clauses, read
against `formal/TraceLean/View.lean`. The two clauses above were re-read
independently and reach a different answer.

| Clause | Verdict |
|---|---|
| `one_representation` | agrees |
| `everything_is_a_buffer` | agrees |
| `text_is_the_content` | drift |
| `structure_over_text` | agrees |
| `structure_has_one_type` | agrees |
| `affordances_named` | drift |
| `frontend_adds_nothing` | agrees |
| `rendering_is_total` | agrees |
| `changes_are_deltas` | agrees |
| `frontend_is_checkable` | drift |
| `screen_is_readable` | drift |
| `presentation_may_be_symbolic` | drift |

### `text_is_the_content` — drift

The clause: nothing shall be displayed that the text does not contain. The
model displays two things the text does not contain.

One: a glyph. `presentedConformance buffer [[{painted := "📁", name := "project"}]] []`
is conformant, and 📁 is not in the buffer's text. The requirement body admits
this ("Read literally, `text_is_the_content` forbade an icon") and resolves it
in prose, but the clause was never reworded. The amendment moved the line from
*painted* to *named* in `presentation_may_be_symbolic` and left this clause
saying *displayed*. As the two clauses now stand the model cannot satisfy both.

Two: `Role.level grade`. The grade is a second source of what is shown, and no
modelled function ties it to the characters it spans. A buffer whose span text
reads `L3` under `Role.level .L1` has `faults = []`; `web/src/render.ts:50`
turns the payload into `role-level-l1` and paints L1's colour over `L3`. The
clause says the text is the *only* source.

The remedy for both is a requirement edit, not a model edit, and it is a
person's.

### `affordances_named` — drift

The clause has two halves: affordances are named by action, *using the names
the keymap dispatches*. The model states the first and is silent on the second.
`Span.actions : List String` accepts any string. A span carrying
`actions := ["file.explode"]` has `faults = []`, is returned by `actionsAt`,
and a frontend that offers it is conformant — while `TraceLean.Act.dispatch`
refuses that name (`acts (dispatch "file.explode" …) = false`). Nothing joins
the two. This is expressible: `Act.acts ∘ Act.dispatch` is already the
predicate the clause needs.

### `frontend_is_checkable` — drift

Nothing models it. `tracelean-trace . --judge REQ-VIEW.frontend_is_checkable`
answers "nothing models REQ-VIEW.frontend_is_checkable, so there is nothing to
judge it against", and no `@models` or `@proves` in `formal/` names it.

This is silence, not disagreement, and it is the more surprising kind: the
material exists. `Rendering` is exactly "what a frontend answers with", and
`conformance` is exactly "checked against that buffer without a person looking
at it" — but `Rendering` carries no annotation and `conformance` is bound to
`rendering_is_total`, `frontend_adds_nothing` and `one_representation` instead.
The Rust side is bound (`crates/core/src/drt/frontend.rs:12`,
`crates/core/src/bin/tracelean-render-text.rs:32`), so the clause has an
implementation and a test but no model. Under `ARCH-HONEST.absence_is_not_pass`
this cannot be read as a pass. Two annotations would close it.

### `screen_is_readable` — drift

The reworded half is modelled: `presentedConformance` reads rows by their
accessible names, so "accessible text of what it drew" is what is compared. The
half that gives the clause its reason is not: *without relying on the
frontend's own account of it*.

Concretely. `presentedConformance` takes `rows : List (List Presented)` as an
argument. So does `conformance` take `Rendering.lines`. Nothing in the model
distinguishes them, so the two-code-path frontend the requirement body names —
"a frontend that draws in one code path and reports in another" — passes
`presentedConformance` exactly as it passes `conformance`: it emits `lib.rs` to
the terminal and returns `[{painted := "main.rs", name := "main.rs"}]`, and the
model returns `[]`. The clause exists to catch that frontend and the model
does not.

The previous section reads this as a note ("stays `@structural` on the capture
side"). I think it is a difference rather than a caveat, because it is the
whole delta between this clause and `frontend_is_checkable` — without the
provenance of `rows`, the two clauses model the same thing twice. Where a value
came from is admittedly not a property of the value; this project does have
`Provenance.lean`, so it is at least not obviously beyond a model. A person may
reasonably conclude the obligation is `@structural` and belongs to the harness.
What they should not do is conclude it from the model, because the model is not
where it is.

### `presentation_may_be_symbolic` — drift

The clause is two obligations. The permission is modelled. The proviso — *the
symbol carries that region's own text as its accessible name* — is not, and
cannot be as the types stand: `Presented` has `painted` and `name` and no
position, so "that region's own text" is not a thing the model can refer to.
What `presentedConformance` checks is the concatenation, per line.

Two inputs the clause forbids and the model accepts, on a buffer whose line is
`main.rs`:

- `[{painted := "📁", name := "main.rs"}, {painted := "🗑", name := ""}]` —
  conformant. A painted region carries no accessible name at all, which is the
  precise accessibility failure the clause was written to prevent.
- `[{painted := "📁", name := "mai"}, {painted := "📁", name := "n.rs"}]` —
  conformant. Neither symbol's name is its region's text; only the sum is.

`a_symbol_is_read_as_its_name` proves something weaker than the clause. It
says the verdict is invariant under repainting every region with one glyph —
that is, `painted` is not read. That is the permission and the "never the
glyphs" tail. It is not the proviso, and it would hold of any function that
ignores `painted`, including one that ignored `name` too. The theorem's name
promises the proviso; its statement is glyph-irrelevance.

Also unmodelled, and stated as an obligation in the requirement body: "The
symbol is chosen by something the core declared — the actions a span names —
and never by matching on the text." No modelled function maps a span to a
glyph, so nothing forbids a frontend picking its icon by scanning
`buffer.text`. Such a frontend is conformant under every function here while
being, in the body's own words, "a second answer to what is on screen, which is
what `one_representation` exists to prevent". This is expressible: give the
selection a function and prove it independent of `Buffer.text`.

`symbolic` is defined and nothing consumes it. Its docstring says it exists "so
that a run can say it exercised the symbolic case"; no modelled function asks.

### Notes on clauses judged `agrees`

*`one_representation`.* The second half — a frontend renders the representation
rather than computing its own — has teeth through `conformance`: lines must be
the buffer's text, actions must be the buffer's actions. The first half, that
there is *one* representation, is a type declaration and no run can fail it.
The "rather than computing its own" reading that the body gives under
`presentation_may_be_symbolic` is unmodelled; see that section.

*`structure_has_one_type`.* True by construction — there is one `Span` type and
nothing could contradict it. The clause's content is that two *producers* land
on it, and only one producer is bound here (`directoryBuffer`). The parser side
is `Produce.spansOfMarks`, bound to `REQ-SHOW`. Worth a person seeing that the
clause is carried by a binding that cannot fail.

*`structure_over_text`.* `Role.level (grade : Evidence.Level)` still says what
the region *is*; a grade is a fact about the region, not a way of drawing it,
so the clause's letter holds. Two things a person should weigh anyway. The
payload's only consumer anywhere is a stylesheet class
(`web/src/render.ts:50`) — `Produce.actionsFor` gives every `Role.level _` the
same actions, so no modelled function reads the grade. And the payload
duplicates the span's own characters with nothing checking they agree; that is
the second drift under `text_is_the_content`.

*`frontend_adds_nothing`.* The invention half is properly caught. The dropping
half, which `conformance`'s own docstring claims ("one that drops the last
action on a line has quietly removed a feature"), has a hole: `positions` is
derived from `rendering.offered`, so a frontend that offers *nothing anywhere*
produces no `actionDropped` breach and is conformant. Not this clause's
obligation — dropping is not adding — but the model claims it and does not do
it.

*`rendering_is_total`.* No modelled function matches on `BufferKind` at all, so
"every buffer kind shall render" cannot fail: `plainText` never looks at the
kind. What has teeth is `conformance`'s line comparison, which does catch a
frontend that draws nothing. The totality itself is by construction.

*`changes_are_deltas`.* The clause asks only for expressibility and gets it,
with a real round-trip theorem. Two observations. `delta` carries the whole new
text whenever one character changes, so the stuttering the Lean prose says
deltas exist to prevent is not addressed by this delta. And `applyDelta` keeps
`buffer.id`, so an id change is inexpressible — the theorem takes `sameId` as a
hypothesis rather than handling it. Both are defensible readings (a delta is a
change *to one buffer*; which buffer is shown is `REQ-SCREEN`'s), but they are
readings, and a person should make them deliberately.
