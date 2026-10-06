# ADR-0011 — A symbol path names one declaration, and precision is per anchor

*2026-09-17*

## Two defects, one cause

`properties.rs::a_document_resolves_its_target_the_way_an_annotation_does` asks
that a document and an annotation resolve the same target the same way. It
failed, and the reason was that a file could hold two declarations with the same
symbol path:

- **Rust.** `impl CallSpec` was emitted as a declaration named `CallSpec`,
  alongside `struct CallSpec`. Two ranges, one path.
- **Lean.** `def Kind.progress` took its name from the grammar's first
  identifier child, which is `Kind`. The inductive and all three of its
  functions shared the path `TraceLean::Kind`.

An ident that names two things makes `doclink::current_hashes` a coin flip:
whichever declaration the scan ended on wins.

## Decision

An `impl` block opens a scope and is not itself a declaration, so `CallSpec` is
the struct and `CallSpec::split_entry` is the method. A Lean name is read out of
the source rather than taken from the grammar's tokenisation, so a dotted name
stays whole. An annotation written directly above `impl Foo` now anchors to the
first method inside it, which is the only declaration it could sensibly mean.

## Precision belongs to the anchor

The same work exposed a second thing worth fixing. A file whose parse contained
any error marked *every* annotation in it imprecise and capped them at L1. For
the Lean models that was almost all of them: `tree-sitter-lean4` 0.3.0 cannot
read tactic combinators (`<;>`), some unicode operators (`∉`), or several
`fun (x : T) =>` forms, and recovery wraps a large region — in the worst case
the whole file — in one error node.

An anchor is now precise when the declaration it names parsed, and when no
unparsed region *begins* between the annotation and that declaration. A region
that merely encloses both says nothing: the declarations inside it were still
found. A region that starts in the gap is the real risk, because it could be
hiding the declaration that should have been the target.

This halved the capped annotations, from 124 to 58, with no change to what is
being claimed. `tree-sitter-lean4` 0.3.0 is the latest published version, so the
remainder stays until the grammar improves — ADR-0008 records that constraint.

## Also

Annotation parse problems were collected and never reported. A misspelt
`@implments` produced no link and no complaint, so the clause read as uncovered
and nothing said why. `index::build` now passes them through.
