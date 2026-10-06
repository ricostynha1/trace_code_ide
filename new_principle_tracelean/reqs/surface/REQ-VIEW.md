---
id: REQ-VIEW
title: One representation, rendered by every frontend
refines: [ARCH-CORE-SHELL, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  one_representation: The core shall produce one representation of what is shown, and every frontend shall render that representation rather than computing its own.
  everything_is_a_buffer: Every shown thing — a file, a directory listing, a diff under review, a menu — shall be a buffer with an identity, a kind and text.
  text_is_the_content: The buffer's text shall be the only source of what is shown; nothing shall be displayed that the text does not contain.
  structure_over_text: Structure shall be spans over that text, and a span shall say what it is rather than how it looks.
  structure_has_one_type: Structure read from a parser and structure produced by the core shall have the same type, so a synthetic buffer needs no grammar of its own.
  affordances_named: What can be done at a span shall be named by action, using the names the keymap dispatches.
  frontend_adds_nothing: A frontend shall not introduce an affordance the representation does not carry.
  rendering_is_total: Every buffer kind shall render in every frontend; a frontend that cannot render a kind richly shall render its text rather than nothing.
  changes_are_deltas: A change to what is shown shall be expressible as a delta from the previous representation.
  frontend_is_checkable: A frontend shall answer, for a buffer it is given, with what it drew, so that what it drew can be checked against that buffer without a person looking at it.
  screen_is_readable: What a frontend puts on the screen shall be readable as the accessible text of what it drew, so that its rendering can be checked against the buffer without relying on the frontend's own account of it.
  presentation_may_be_symbolic: A frontend may present a region as a symbol rather than as its text, provided the symbol carries that region's own text as its accessible name; what is read off a screen shall be those names and never the glyphs.
---

# One representation, rendered by every frontend

The old tree had two frontends. The TUI was dead and only the Tauri editor
worked, because each surface computed its own view of everything and only one of
them was ever maintained. Two surfaces meant two answers to "what is on screen",
and the second answer rotted.

So the core answers once. A buffer is an identity, a kind and text; structure is
spans over that text saying what each region *is* — an entry, a path, a
requirement id, an evidence level — and what can be done there, named by the same
actions [REQ-MYTH](REQ-MYTH.md) dispatches. A directory listing is a buffer whose
spans mark indentation and paths; a menu is a buffer whose spans mark choices.
Code gets its spans from a parser, synthetic buffers get theirs from the core,
and both are the same type.

A frontend's whole job is then to render that value as well as its medium
allows. Tauri may draw a span with an affordance as a button and the TUI may bind
it to a key: neither invents it, so neither can drift from the other. What a
frontend may not do is add an affordance the representation does not carry,
because that is the thing that cannot be checked and the thing that rotted.

The reason this is worth the indirection here rather than being taste: a
representation is a value, so it falls under the same methodology as everything
else — modelled, differentially tested, graded. Pixels are not. The surface has
been the untested half of this system since the beginning, and this is what makes
it testable.

`frontend_is_checkable` is what turns that from an argument into a test. A
frontend answers with the lines it drew and the actions it offered, which is a
value; the check is a function of that value and the buffer it came from, so the
same code checks a terminal, a web view, or anything else somebody writes.

That check believes the answer, though, and a frontend that draws in one code
path and reports in another would pass it while showing something else.
`screen_is_readable` closes that: the frontend paints, the harness reads the
bytes a terminal would have received, and the frontend gets no chance to
describe itself. What a screen cannot carry is what is *offered* — an action is
visible only where a frontend chose to show it — so the two checks are worth
more together than either alone.

What is left over after both is how it *looks*, which a person judges — and the
evidence ladder already has a rung for that.

`presentation_may_be_symbolic` is what lets a window draw an icon. Taken
literally, `text_is_the_content` forbade one: an icon is not the buffer's text,
so the only conformant station bar was a row of words. The amendment moves the
line from *painted* to *named*. A frontend may paint whatever its medium
affords, and what is read off the screen is the accessible name each region
carries — which must be that region's own text. The window draws 📁, a screen
reader says `project`, the harness reads `project`, and the comparison means
what it always meant. It is the same guarantee from both sides: the check and
the person using a screen reader read the same thing.

What the frontend may not do is decide what exists. The symbol is chosen by
something the core declared — the actions a span names — and never by matching
on the text, because a frontend that recognises its own strings is a second
answer to what is on screen, which is what `one_representation` exists to
prevent. A region whose declaration the frontend does not recognise is drawn as
its text; `rendering_is_total` already says so.
