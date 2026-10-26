---
id: REQ-DOCLINK
title: Documentation is linked and goes stale
refines: [REQ-STALE, REQ-ANCHOR, ARCH-SELFHOST, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  declares_target: The system shall require a document to declare what it describes, as an anchor.
  records_hash: The system shall require a document to record what its target hashed to when the document was written.
  hash_moves_review: When a document's target's hash has changed, the system shall report the document as in review.
  review_is_not_error: The system shall keep in review a state distinct from both current and dangling, and shall not block on it.
  dangling_reported: If a document's target no longer exists, then the system shall report the document as dangling.
  confirmation_is_human: The system shall return a document to current only when a person re-records the hash, never automatically.
  same_anchor_machinery: The system shall resolve a document's target by the same anchor mechanism as an annotation's.
  decisions_exempt: Where a document is a decision record, the system shall take it as describing a moment rather than a subsystem, and shall require it to declare the requirements it affects instead of a hashed target.
---

# Documentation is linked and goes stale

Documentation rots because nothing connects it to what it describes. A
requirement already carries a hash and drops back to review when it moves, and
nothing in that is specific to requirements, so a document is a link too.

In review means unconfirmed since the thing underneath changed, not known to be
wrong: blocking would make people stop writing documentation, and hiding it
would make the documentation worthless. A linter that checks names resolve
cannot notice that a function still exists and now does something else, which
is when documentation actively misleads; the hash covers that and costs nothing
new.
