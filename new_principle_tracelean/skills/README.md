# Working on a TraceLean project

You have been given a repository that uses the TraceLean methodology. These
documents tell you how to work in it. They are about the **method**, not about
any one repository — TraceLean's own tree is simply one project that uses it,
and everything here applies to any other.

Read [00-the-method.md](00-the-method.md) first. It is short and nothing else
makes sense without it.

| | |
|---|---|
| [00-the-method.md](00-the-method.md) | What the project claims, and the vocabulary |
| [01-orienting.md](01-orienting.md) | What to read first in a tree you have not seen |
| [02-annotations.md](02-annotations.md) | The only mechanism linking anything to anything |
| [03-requirements.md](03-requirements.md) | Reading, writing and changing a requirement |
| [04-models-and-bindings.md](04-models-and-bindings.md) | Adding a model and comparing it to the code |
| [05-evidence.md](05-evidence.md) | What a claim is worth, and why you cannot write that down |
| [06-making-a-change.md](06-making-a-change.md) | The loop to run for any change |
| [07-findings.md](07-findings.md) | Reading the checker's output |

## The one-paragraph version

A TraceLean project says what it means for itself to be *proper*: requirements
with identity, a formal model saying what each clause means, an implementation
claiming to realise it, and the claim **checked rather than believed**. Links
between those exist only where somebody wrote an annotation — never inferred
from a filename or a directory. A checker reads the tree and reports what is
missing. Evidence is *earned* by a backend that ran something, never asserted in
a comment.

## The rules you will break first

These are the mistakes an agent new to a TraceLean tree makes. Read them now,
not after.

1. **Do not write code without an annotation linking it to a clause.** Untraced
   code is the failure the method exists to prevent. If there is no clause for
   what you are writing, write the clause first ([03](03-requirements.md)).
2. **Do not invent a link from a filename.** `src/auth.rs` does not implement
   `REQ-AUTH` because of its name. It implements it because a comment in it says
   `@implements REQ-AUTH.some_clause`.
3. **Do not write an evidence level anywhere.** A level is earned by a run and
   folded into the lock ([05](05-evidence.md)). Typing `L3` in a comment claims
   something you did not establish.
4. **Do not mark a clause exempt to make a finding disappear.** An exemption
   needs a reason and an approver, and the checker reports one that has neither.
   `Unmodeled` is information about work not yet done, not an error to silence.
5. **Do not test a function by supplying the value the code under test was
   supposed to produce.** That is how a whole-system bug survives a green suite.
   Drive the real thing from the outside where you can.
6. **Run the checker before you say you are done.** It is fast, it reads the
   tree, and it is the project's own definition of whether your change is
   finished.

## If something here contradicts the repository

The repository wins. These documents describe the method; a project may have
made local decisions about it, and those are written down in its own
`docs/decisions/` (or equivalent). Read them before assuming this page is
authoritative for that tree.
