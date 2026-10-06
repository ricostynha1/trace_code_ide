---
adr: 10
title: A binding's entry point takes owned arguments
date: 2026-09-17
status: accepted
supersedes-part-of: ADR-0006
affects: [REQ-DRT-RUST, REQ-DRT-BIND]
---

## Context

ADR-0006 had the generated runner deserialise each argument with `from_str` over
its raw JSON slice, so that an implementation taking `&str` could be bound
without a wrapper. That works until a generated string contains an escape.

The differential test for the signature parser found it on the first run with a
source containing a newline:

```
argument `source`: invalid type: string "fn ff(x: u8) {}\np", expected a borrowed string
```

This is not fixable by lifetimes or buffering. The unescaped bytes of `"a\nb"`
do not exist contiguously anywhere in the JSON text, so there is nothing for a
borrow to point at.

## Decision

A binding's entry point takes owned arguments. Where the natural function takes
references, the project adds an `_of` wrapper beside it — the same pattern
already used for `&self` methods and for functions whose result shape needs
flattening for the wire.

ADR-0006's mechanism stays: it is still what makes `&RawValue` per-argument
dispatch work, and borrowed arguments still succeed whenever no escape is
involved. What changes is that they are no longer something a binding may rely
on.

## Consequence

The failure is loud, immediate and names the argument, so a binding that breaks
this rule is reported the first time it runs rather than producing a wrong
answer. That is why it is left as a runtime error rather than a check at
binding-resolution time — resolution deliberately does not read types
(`REQ-DRT-RUST.types_inferred`), so it could not detect this without giving that
up.
