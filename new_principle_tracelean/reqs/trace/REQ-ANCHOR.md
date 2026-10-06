---
id: REQ-ANCHOR
title: Anchors and body hashing
refines: [ARCH-SELFHOST, ARCH-EFFECT-LAW, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  symbol_not_line: An anchor shall address a symbol path or an explicit region, never a line number.
  stable_under_move: An anchor's identity shall be unchanged by edits that do not change the anchored body.
  hash_tracks_body: The body hash shall change if and only if the normalised body changes.
  comments_excluded: Comment text shall be excluded from the body hash, so editing an annotation cannot invalidate its own evidence.
  whitespace_normalised: Whitespace outside literals shall be normalised before hashing, and whitespace inside literals shall not be.
  imprecise_capped: A file with no available grammar shall anchor to the whole file, and links through it shall be capped at the lowest evidence level.
---

# Anchors and body hashing

Line numbers rot on the first edit above them.
`crates/core/src/evidence.rs::assurance` survives reordering, reindentation and
moving a function within its file.

`hash_tracks_body` is a biconditional because both directions are soundness
properties and fail differently. A hash that does not move when the body does
leaves stale evidence looking valid; one that moves when the body did not makes
people stop annotating.

`comments_excluded` closes an otherwise vicious loop: the annotation lives in a
comment attached to what it anchors, so if comments counted, writing the
annotation would invalidate the evidence it carries.

Symbol extraction needs a parser the model cannot run, so it is axiomatised under
[ARCH-EFFECT-LAW](../arch/ARCH-EFFECT-LAW.md) with the law that equal normalised
bodies hash equally.
