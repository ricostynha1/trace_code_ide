---
id: REQ-ANCHOR
title: Anchors and body hashing
refines: [ARCH-SELFHOST, ARCH-EFFECT-LAW, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  symbol_not_line: The system shall address an anchor to a symbol path or an explicit region, never to a line number.
  stable_under_move:
    text: When an edit does not change the anchored body, the system shall leave the anchor's identity unchanged.
    regions: The system shall identify a declaration or a whole file by its file and its name alone, and an explicit region, which has no name, by its file and the lines between its markers, so that an edit above a region changes its identity.
  hash_tracks_body: The system shall change the body hash if and only if the normalised body changes.
  comments_excluded: The system shall exclude comment text from the body hash, so that editing an annotation cannot invalidate its own evidence.
  whitespace_normalised: The system shall normalise whitespace outside literals before hashing, and shall not normalise whitespace inside literals.
  imprecise_capped: If no grammar is available for a file, then the system shall anchor to the whole file and cap links through it at the lowest evidence level.
---

# Anchors and body hashing

Line numbers rot on the first edit above them, while
`crates/core/src/evidence.rs::assurance` survives reordering, reindentation and
moving a function within its file. A region has no name to survive by, which is
why claims prefer a declaration.

`hash_tracks_body` is a biconditional because both directions fail differently:
a hash that does not move leaves stale evidence looking valid, and one that
moves when the body did not makes people stop annotating. The annotation lives
in a comment beside what it anchors, so if comments counted, writing it would
invalidate the evidence it carries. Symbol extraction needs a parser the model
cannot run, so it is axiomatised under
[ARCH-EFFECT-LAW](../arch/ARCH-EFFECT-LAW.md) with the law that equal
normalised bodies hash equally.
