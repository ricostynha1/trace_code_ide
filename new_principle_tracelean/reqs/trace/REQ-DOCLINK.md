---
id: REQ-DOCLINK
title: Documentation is linked and goes stale
refines: [REQ-STALE, REQ-ANCHOR, ARCH-SELFHOST, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  declares_target: A document shall declare what it describes, as an anchor.
  records_hash: A document shall record what its target hashed to when the document was written.
  hash_moves_review: A document whose target's hash has changed shall be reported as in review.
  review_is_not_error: In review shall be a distinct state from both current and dangling, and shall not block.
  dangling_reported: A document whose target no longer exists shall be reported as dangling.
  confirmation_is_human: A document shall return to current only by a person re-recording the hash, never automatically.
  same_anchor_machinery: A document's target shall be resolved by the same anchor mechanism as an annotation's.
  decisions_exempt: A decision record shall describe a moment rather than a subsystem, and shall declare the requirements it affects instead of a hashed target.
---

# Documentation is linked and goes stale

Documentation rots because nothing connects it to what it describes. This project
already has the connection: a requirement carries a hash of its content, a model
claims to model it, and when the text changes the hash moves and the bond drops
back to needing review. Nothing in that is specific to requirements and models.

So a document is a link. It names what it describes, records what that hashed to,
and when the hash moves the document is *in review*.

`review_is_not_error` makes the state usable — the document is not known to be
wrong, only unconfirmed since the thing underneath it changed. Blocking would
make people stop writing documentation; hiding it would make the documentation
worthless.

`same_anchor_machinery` is the point of stating this at all. A linter that checks
names resolve cannot notice that a function still exists and now does something
else, which is the case where documentation actively misleads. Generated blocks
protect only content that was a table anyway, leaving the explanation of *why*
unprotected. The hash covers both, and costs nothing new.
