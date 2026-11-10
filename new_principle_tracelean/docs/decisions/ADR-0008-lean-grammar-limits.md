---
adr: 8
title: Lean is read at the level of its commands, and an unparsed file caps its own claims
status: accepted
affects: [REQ-ANCHOR, REQ-ANNOT]
---

# ADR-0008 — Lean is read at the level of its commands

## Context

Anchoring needs to know, for each Lean file, which declarations it makes, where
each begins and ends, and under which name and namespace; where its comments
and string literals are; and whether any of that is in doubt. It does not need
the structure of a term or a tactic proof.

Lean's terms, tactics and notation are extensible by the very files that use
them (`syntax`, `macro`, `notation`, `elab`), so no fixed grammar of terms reads
all of Lean. What is fixed is the layer above: a file is a sequence of commands,
and a command starts on a line of its own.

## Decision

The grammar in `vendor/tree-sitter-lean4` reads commands. A declaration is its
keyword, its name and its body, and the body is a run of tokens (identifiers,
numbers, strings, characters, name literals, operators, brackets) up to the
next command. The external scanner (`src/scanner.c`) decides where that is, by
the first word of the next line:

- at column 0, any word except those that continue a body there (`where`,
  `termination_by`, `decreasing_by`, `deriving` without `instance`, `|` and the
  term keywords), and `@[`, `#…` or a doc comment — except a doc comment followed
  by `|`, which documents a constructor;
- indented, a declaration keyword after any attributes and modifiers, a scoping
  command (`variable`, `section`, `namespace`, `end`, `mutual`, …), or `open`,
  `set_option` or `attribute` unless the line ends in `in` (a proof's
  `open Classical in`).

A syntax quotation `` `(…) `` is a group whose parentheses are matched, so a
command quoted inside a body is data and ends nothing. Block comments nest, as
Lean's do. A declaration's extent ends at its last token (`anchor.rs::last_token`):
the comments and blank lines before the next command are not part of it.

What is still unreadable is a file malformed at the level of commands: a
declaration with no name, a block comment or quotation never closed. Such a parse
contains an error node and is **imprecise**: the annotations it affects anchor to
the whole file, are capped at the lowest evidence level, and the file is reported
under its own finding kind, `Imprecise`, at warning severity. Not an error: the
claims are real and still recorded, only not precisely placed, and Lean itself
reports the malformation when the models are built.

Scopes are read from the text, because `namespace … end` is a pair of siblings
rather than a node containing its declarations. Only an `end` that *names* a
scope closes one, read from its `name` field: Lean writes a bare `end` to close a
`mutual` block or an anonymous section.

## Consequences

The measure is Lean's own sources, in the toolchain named by
`formal/lean-toolchain`: every one of its 1,222 files parses without an error
node, and every line that starts a named declaration outside comments, strings
and quotations is a declaration the grammar found, with none found that the text
does not start (`lean_grammar.rs::lean_s_own_sources_parse_and_every_declaration_is_found`).
`formal/` is checked the same way on every run of the fast suite.

A body is not checked against Lean's syntax, so a term Lean would reject still
parses here; that is Lean's to report, at `lake build`, and the differential
tests run the model Lean accepted. Highlighting colours tokens (keywords,
names, strings, numbers, comments, operators) and the names of what a command
declares, not the structure of a term.

`tracelean-trace --unparsed <file>` prints the regions a file could not be read
in, so a cap can be looked at rather than guessed about.

## Rejected

*A grammar of Lean's terms and tactics.* It cannot be complete, because the
files it reads extend the syntax it would have to know, and each construct
outside it cost every declaration after it its precision.

*A line-based scan for files that fail to parse.* It would recover declaration
names by pattern-matching text, and be wrong in exactly the cases that made the
parse fail. A binding that cannot be placed is better than one that appears to be.
