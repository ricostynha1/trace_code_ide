# Requirements

A requirement is a markdown document with frontmatter carrying its identity and
its clauses, and a body saying why the clauses are what they are.

```markdown
---
id: REQ-CONVERT
title: Converting between temperature scales
refines: [ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  scales_round_trip: Converting a temperature to the other scale and back shall give what went in.
  agreement_is_named: The one temperature at which the scales agree shall be available as a value rather than written into each caller.
---

# Converting between temperature scales

Why these clauses and not others. What failure each one prevents. What this
does *not* claim.
```

## The fields

- **`id`** is the identity. **Never the path.** A document may be moved to a
  different directory without a single annotation changing, and a project that
  inferred identity from a filename would have a link nobody wrote.
- **`title`** is one line, read in the index.
- **`refines`** names the requirements this one decomposes. It builds a graph;
  a cycle in it is reported, and naming a requirement that does not exist is
  reported as `DanglingRefines`.
- **`status`** is the document's own state — typically `draft` or `approved`.
- **`decomposition`** is `complete` or `open`. `complete` means the author
  claims the clauses exhaust the requirement. `open` means nobody has said what
  the denominator is, and coverage for it is rendered as a lower bound. An
  architectural constraint is almost always `open`: nobody can claim to have
  exhausted one.
- **`clauses`** is a map from a key to one sentence.

## Writing a clause

A clause is **one sentence that can be true or false of the system**, and it is
the unit everything else attaches to. Getting them right is most of the work.

- **Use "shall".** It is the convention and it forces a commitment rather than a
  description.
- **One claim per clause.** If a sentence has an "and" joining two independent
  obligations, it is two clauses. You cannot say which half failed otherwise.
- **Say what must be true, not how.** `The bar shall be computed from the keymap
  and shall not be maintained separately` is a law. `Use a HashMap` is not.
- **Prefer a clause you could write a function for.** A clause that is a claim
  about a value gets a model, a binding and L3. A clause that is a claim about
  the tree gets `@structural` and caps at L2. Both are legitimate — but if you
  can phrase the same obligation as a property of a value, do.
- **Name it for the failure it prevents.** `escape_pops_one` is a better key
  than `escape_behaviour`, because when it fails you already know what broke.

The body is where you say why. Write down the failure each clause prevents —
ideally the real one that motivated it. A clause with no story behind it is one
a future reader will delete.

## Adding a requirement

1. **Check it does not exist.** Read the index. Most of the time what you want
   is a clause on an existing document, not a new document.
2. Write the document with an id nothing else uses. A duplicate id is reported.
3. Add it to the index, if the project keeps one.
4. **Every clause now needs answering.** A new document with five clauses adds
   five `Unmodeled` findings on the spot. That is honest, and it is also the
   work you have just signed up for. Either do it or say in the body and in the
   progress document that it is outstanding.
5. Run the checker.

## Adding a clause to an existing requirement

The same, smaller. Note that adding a clause **changes the document's hash**,
which puts any documentation that declares it describes that requirement into
review. That is the mechanism working: a document describing a requirement that
changed should be reread. Find the documents that name it and update them.

## Changing a clause

Changing what a clause *says* invalidates what was established about it. Every
record naming that requirement goes stale, and it should. Expect the evidence
figures to drop, and do not go looking for a way to keep them up.

If you are tempted to reword a clause so that the existing code satisfies it,
stop. That is writing the requirement to fit the implementation, which is the
one move the entire method exists to make visible.

## Deleting a clause

Delete the clause, then delete or retarget every annotation that named it —
otherwise they are `Dangling` and will block. `grep` for the clause identifier
before you delete it, not after.
