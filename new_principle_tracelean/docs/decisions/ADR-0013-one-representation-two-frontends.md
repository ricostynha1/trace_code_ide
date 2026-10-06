---
adr: 13
title: The core produces one representation; frontends only render it
status: accepted
affects: [REQ-VIEW, REQ-MYTH, ARCH-CORE-SHELL]
---

# ADR-0013 — One representation, frontends only render

## Context

The tree this port came from had a TUI and a Tauri frontend. The TUI was dead:
only the Tauri surface worked, because each frontend computed its own view of
everything and only one of them was maintained. Two surfaces meant two answers
to "what is on screen", and the unmaintained answer rotted silently — nothing
could fail, because nothing compared them.

## Decision

The core produces **one representation** and a frontend renders it.

- A **buffer** is an identity, a kind and text. Everything shown is one: a file,
  a directory listing, a diff under review, a menu.
- **Structure** is spans over that text. A span says what a region *is*, never
  how it looks, and carries the **actions** available there — the same names the
  keymap dispatches.
- Structure read from a parser and structure produced by the core have **one
  type**. A directory listing does not need a grammar; the core emits its spans
  directly.
- A frontend may render a span as richly as its medium allows. It may not add an
  affordance the representation does not carry.

## Consequences

The surface becomes a value, so it comes under the same methodology as the rest:
modelled, differentially tested, graded. This is the half of the system that has
never been checked, and the reason the TUI could die unnoticed.

Frontends get small — a total function from the representation to their medium —
and a second frontend stops being a second implementation.

The cost is indirection, and a real risk: a representation that carries only text
and positions would make the GUI a terminal in a webview. That is why spans carry
meaning and actions rather than styling. A rich frontend lifts a diff span into a
side-by-side view because it knows what the span *is*; it does not need the core
to describe the view.

A frontend is then checkable twice over: by what it says it drew, and by what it
actually painted. The second exists because the first believes the frontend —
a program that draws in one code path and reports in another would pass it — and
because neither check subsumes the other: a screen carries text but not
affordances, a report carries both but only on the frontend's word.

## Rejected

*Each frontend owns its view model.* That is what was there, and it is what died.

*Check only what a frontend reports.* Cheaper, and it is the check a frontend
can satisfy without drawing anything at all. A frontend that agrees with itself
has established nothing, which is the same failure the TUI died of.

*Make synthetic buffers go through tree-sitter so everything has a grammar.*
Writing a grammar for a directory listing to avoid a second producer inverts the
cost: the type is what has to be shared, not the way it is produced.
