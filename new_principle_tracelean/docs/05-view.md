---
describes: [REQ-VIEW, REQ-MYTH]
described_hash:
  REQ-VIEW: 4694bd7184911203
  REQ-MYTH: 0120c37ad5d78c7b
---

# The representation

One value says what is on screen. A **buffer** is an identity, a kind and text;
**spans** over that text say what a region *is* and what can be done there.

```
Buffer { id, kind: file | directory | review | menu | record, text, spans }
Span   { start, stop, role, actions }
```

Offsets are characters, not bytes: a byte offset means two frontends disagree
about where a span is the moment anything is not ASCII.

## Why

Two frontends computing their own view give two answers to "what is on screen",
and the unmaintained answer rots with nothing failing. That is what happened to
the TUI this port came from. One value has one answer — and a value can be
modelled, differentially tested and graded, which pixels cannot.

## What a frontend may do

Render the text, as richly as its medium allows: Tauri may draw a span with an
action as a button where a terminal binds it to a key. What it may not do is
show text the buffer does not contain, or offer an action no span declares.

`actions` are the names [REQ-MYTH](../reqs/surface/REQ-MYTH.md)'s keymap
dispatches, so the two layers share one vocabulary and a key and a button reach
the same code.

## How a frontend is tested

Three rungs, each checking something the one below cannot.

**1. What it says it drew.** A frontend answers with

```
Rendering { lines, offered: [(offset, action)] }
```

and `conformance(buffer, rendering)` returns what it got wrong — a line that is
not the buffer's, an action invented, an action dropped. A pure function of two
values, so it is modelled in Lean and differentially tested like everything
else, and it runs against any frontend that answers over the
[DRT protocol](../reqs/drt/REQ-DRT-PROTO.md). `drt::frontend::check`.

**2. What it actually painted.** Rung 1 believes the frontend; a program that
draws in one code path and reports in another passes it while showing something
else. So `drt::frontend::capture` runs the frontend in paint mode, reads the
bytes a terminal would have received, strips the escapes and checks the text —
the frontend gets no chance to describe itself. It cannot check what is
*offered*, because a screen does not carry affordances, so the two rungs are
worth more together than either alone.

`tracelean-render-text --two-faced` is the control: it reports the buffer's own
text and paints something else. It passes rung 1 and fails rung 2, which is the
whole argument for rung 2 existing.

What rung 2 reads is the **accessible** text, not the glyphs. That is what lets
the window paint a station as 📁 where the buffer says `project`: a frontend may
present a region as a symbol provided the symbol carries the region's own text
as its name, and the harness reads names. A screen reader reads the same thing,
so the check and the person get one guarantee rather than two.

```
Presented { painted, name }     accessible(row) = the names, joined
```

The painting is never consulted — `a_symbol_is_read_as_its_name` is that as a
theorem. Which glyph is the frontend's choice, made from the actions the core
declared and never from the text: a frontend recognising its own strings is a
second answer to what is on screen. `web/src/frontend.ts --mislabel` is the
control here, naming each region after its own glyph; it must fail rung 2.

**3. How it looks.** Not machine-checkable and not pretended to be: judged by a
person, which the evidence ladder already has a rung for (L2).

Rungs 1 and 2 are `@structural`: a live process disagreeing with a buffer is not
a value either side computes, so the evidence comes from running it rather than
from a differential pair.

To be checkable a real frontend needs two entry points and nothing else: one
that reads a buffer and answers with a `Rendering`, one that reads a buffer and
paints. A web view paints by serialising its rendered DOM text; the harness does
not care which, because it only ever reads bytes.

## Synthetic buffers

A directory listing has no grammar and needs none: `directoryBuffer` emits the
spans directly, and they are the same spans a parser produces for a file. One
type, two producers — writing a grammar for a listing would invert the cost
(ADR-0013).
