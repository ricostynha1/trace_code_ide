---
id: REQ-VIEW
title: One representation, rendered by every frontend
refines: [ARCH-CORE-SHELL, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  one_representation: The system shall produce in the core one representation of what is shown, and every frontend shall render that representation rather than compute its own.
  everything_is_a_buffer: The system shall make every shown thing, whether a file, a directory listing, a diff under review or a menu, a buffer with an identity, a kind and text.
  text_is_the_content: The system shall take what is shown from the buffer's text alone, and shall display nothing the text does not contain.
  structure_over_text: The system shall express structure as spans over that text, each saying what it is rather than how it looks.
  structure_has_one_type: The system shall give structure read from a parser and structure produced by the core the same type, so that a synthetic buffer needs no grammar of its own.
  affordances_named: The system shall name what can be done at a span by action, using the names the keymap dispatches.
  frontend_adds_nothing: The system shall not let a frontend introduce an affordance the representation does not carry.
  rendering_is_total: The system shall render every buffer kind in every frontend, and where a frontend cannot render a kind richly, it shall render its text rather than nothing.
  changes_are_deltas: The system shall make a change to what is shown expressible as a delta from the previous representation.
  frontend_is_checkable: When a frontend is given a buffer, the system shall have it answer with what it drew, so that what it drew can be checked against that buffer without a person looking at it.
  screen_is_readable: The system shall make what a frontend puts on the screen readable as the accessible text of what it drew, so that its rendering can be checked against the buffer without relying on the frontend's own account of it.
  presentation_may_be_symbolic: Where a frontend presents a region as a symbol rather than as its text, the system shall have the symbol carry that region's own text as its accessible name, and shall read off a screen those names and never the glyphs.
---

# One representation, rendered by every frontend

The old tree had two frontends, each computing its own view, and only one was
maintained; two answers to what is on screen meant the second rotted. So the
core answers once. A buffer is an identity, a kind and text; spans say what each
region is (an entry, a path, a requirement id, an evidence mark) and what can be
done there, named by the actions [REQ-MYTH](REQ-MYTH.md) dispatches. Code gets
its spans from a parser and synthetic buffers from the core, in the same type.

A frontend renders that value as well as its medium allows: a window may draw an
affordance as a button and a terminal bind it to a key, and neither invents it.
A representation is a value, so it is modelled, differentially tested and graded
like everything else; pixels are not.

A frontend's answer of what it drew is a value a function can check, but a
frontend that draws in one path and reports in another would pass, so the
harness also reads the bytes the terminal received. A screen cannot show what is
only offered, so the two checks are worth more together; how it looks is left
to a person's judgement. An icon is allowed because the line is drawn at what a
region is named, not what is painted: the window draws 📁, a screen reader and
the harness both read `project`. The symbol is chosen from the actions the core
declared, never by matching on the text, and a declaration the frontend does not
recognise is drawn as its text.
