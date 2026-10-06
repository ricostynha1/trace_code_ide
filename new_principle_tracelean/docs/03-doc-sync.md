---
describes: [REQ-DOCLINK]
described_hash:
  REQ-DOCLINK: ca9f26ad87c3cd2e
---

# Why these documents cannot go stale

Documentation rots because nothing connects it to the thing it describes. The
connection is the whole problem, and this project already has a solution to it —
the one it uses between a requirement and its formal model.

A requirement carries a hash of its own content. A model claims to model that
requirement. When the requirement's text changes the hash moves, the evidence
keyed to that link is invalidated, and the bond drops back to needing review:
nobody has judged the pair that now exists, because the pair that now exists is
new. Nothing about that mechanism is specific to requirements and models.

**A document is a link like any other.** It claims to describe something, it
records what that thing hashed to when it was written, and when the hash moves
the document goes *in review*.

## How it works

A document declares what it describes and what that described thing hashed to:

```markdown
---
describes: crates/core/src/evidence.rs::assurance
described_hash: 9f2c1a…
---
```

The target is an anchor — the same symbol-path-plus-normalised-body-hash anchor
the annotations resolve to, so it survives renaming and reindentation and moves
when the code actually changes. A document may describe several things, and a
requirement clause is as valid a target as a symbol.

Three states, and they are the states the rest of the system already has:

| State | Meaning |
|---|---|
| **current** | The recorded hash matches. The document was written about this. |
| **in review** | The hash moved. The document may still be right; nobody has confirmed it since the change. |
| **dangling** | The target no longer exists. |

In review is not an error. It is the same honest position the system takes
everywhere else: something changed underneath a claim, so the claim is no longer
backed until a person looks. Confirming is a human act — read the document
against the changed code, and re-record the hash — which is exactly the judging
workflow in [REQ-JUDGE](../reqs/surface/REQ-JUDGE.md), applied to prose.

## Why this beats the alternatives

The mechanisms normally reached for here are a reference-resolution linter and
regenerated content blocks. Both are weaker, and neither answers the actual
question.

A linter checks that a name still exists. It cannot notice that a function still
exists and now does something else, which is the case where documentation
misleads rather than merely dangles — and misleading documentation is worse than
absent documentation, because it is trusted.

Generated blocks avoid drift by removing the prose, which works only for content
that was a table anyway. The valuable part of a document is the part that
explains *why*, and that part cannot be generated, so it is precisely the part
those mechanisms leave unprotected.

The hash covers what both miss, and it costs nothing new: the anchor machinery,
the normalised body hash and the staleness rules are already built for
`REQ-ANCHOR` and `REQ-STALE`. Documentation gets them by being expressed in the
same terms rather than by having a second system written for it.

## The rule that remains

One rule survives from the weaker design because it is about what a document
should contain rather than how it is checked:

**Documents reference identity, never derived facts.** A document may name a
requirement id, a clause key or a symbol path. It should not restate counts,
coverage figures or evidence levels — not because they would go unchecked, but
because they are noise in prose, change without anybody editing anything, and are
better read from the system that computes them.

## Decision records are immutable

`docs/decisions/ADR-NNNN-*.md` records a decision, its context and what was
rejected, and is never edited after it lands. A decision that changes gets a new
ADR that supersedes it by number; the old one stays, because a superseded ADR is
the only record of why an option was tried.

ADRs therefore do not go in review when code changes — they describe a moment,
not a subsystem. What they declare instead is the requirements they affect, and
those ids must resolve.

Stated as a requirement: [REQ-DOCLINK](../reqs/trace/REQ-DOCLINK.md).
