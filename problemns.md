## Current implementation priorities

The main risk in the project is state correctness. Undo history, checkpoints, and persistence need to be trustworthy before more advanced agent and collaboration features can be stable.

## 1. Undo tree, checkpoints, and persistence

This is the core state model, so it should be the first system that behaves deterministically.

### What exists today

- Global undo tree
- Per-file undo tree
- Commit snapshots
- Batch grouping

### My opinion

The batch concept looks redundant. A list of commands already behaves like a batch, so  must be removed

Commit snapshots are not being used like if it were git, it would be something outside (trigger when user clicks button to save a nsapshot)

### Suggested implementation

- Keep two layers of history:
  - compact command log for ordinary edits
  - explicit snapshot records for user-triggered save points
- Compress repeated low-level edits:
  - merge single-character inserts into word inserts when contiguous
  - merge single-character deletes into word deletes when contiguous
- Store only lightweight metadata for snapshots:
  - label or commit message
  - timestamp
  - touched files
  - optional test/coverage results
- Persist the command log on disk so startup can rebuild the current state without replaying every UI detail.
- Add tests under a dedicated undo-history test area (testes are missing) for:
  - insert/delete compaction
  - snapshot restoration
  - persistence round-trips
  - agent-generated edits being undoable deterministically


Create a snapshot will trigger a validation pipeline (for now that is a dummy function that will return everyghin is in order, but will more in front do validation, parsing, compilation, testing)

## 2. Agent harness correctness

This comes next because all AI-assisted changes depend on the command pipeline being predictable.

### Suggested implementation

- Add tests for every harness command to confirm tool calls happen exactly as expected.
- Ensure file writes always go through shadow state, diff generation, and command application.
- Prevent direct mutation of workspace state from agent code paths. (for these potentially in the docker we would have more control, basically agents will not be run with the user that detains those files, he would only be allowed to directly read them.  He will perform its changes on a buffer of the file, that is then delegated to the aplication, The aplliaation performs the diffs and finds the agent edit equivalnet in platform commands and then yes it edits the files).
- Create narrow delegation agents that can read only the minimum slice of code they need.

## 3. Trace graph

The trace graph is currently a visibility problem. If it does not render the full chain from requirements to code to tests, the rest of the system becomes harder to trust.

### Suggested implementation

- Fix rendering first, before adding more graph features.
- Add minimal end-to-end tests that prove requirement, spec, code, and test links appear.


## 4. Multi-agent collaboration in separate tasks

Undo history will likely be the foundation of multi-agent collaboration, but the merge model needs to be thought through carefully.

### Problem

Software features are usually built in parallel and merged later. In this model, the history does not branch. There is only one shared command log, so the problem is not merging branches, but replaying independent edits safely into the same linear history.

### Suggested implementation

- Treat each agent session as commands anchored to a base state, but always integrate them into the same shared log.
- When an edit arrives, convert it into a replayable form against the current state.
- Prefer diff-based rebasing over raw command concatenation.
- Preserve the ability to compute reverse operations for rollback.
- Keep the history linear; user-specific undo is a projection on top of that log, not a separate branch.


I would separate agent execution from workspace integration. Let agents work independently, but require a controlled rebase phase before their edits become part of the single shared history.

## 5. Remote execution and live collaboration

Remote collaboration adds ordering, identity, and permission concerns on top of the undo system.

### Suggested implementation

- Transport command events between collaborators, not raw file mutations.
- Keep a total order of applied commands so every participant can reconstruct the same history.
- Use authenticated remote access, likely over SSH or an equivalent secure channel.
- Model each user’s undo scope separately, even if the global history is shared.

### Caveats

- Events may arrive out of order.
- Commands that are reversible in isolation may not be trivial to reverse after later dependent edits.
- Operations like move, rename, or create can become hard to rewrite cleanly across collaborators.

### My opinion

The hardest part is not transport; it is semantic ownership. A user should only undo their own intent, but the shared history still has to stay coherent for everyone else.

That does not mean the global command application layer disappears. The workspace still needs one canonical way to apply commands, validate them, and build the shared state. What should not be global is the undo action itself. Undo should be projected per user on top of the shared history, not treated as a single shared rewind button.

### What I would do differently

I would not try to solve this by directly rewriting the whole tree on every undo. Instead, I would keep a shared canonical log plus per-user undo projections layered on top of it.

In practice, that means:

- The global system stores the ordered command log and materializes the current workspace state from it.
- Each user gets a personal view of which commands they own and which reverse operations are valid for them.
- When a user undoes something, the system emits a new command into the shared log that compensates for that user's prior edit, rather than mutating history in place.

  Example:
  - Alice inserts the text `hello`.
  - That insert becomes part of the shared log.
  - Later, Alice presses undo.
  - The system does not erase the original insert from history.
  - Instead, it appends a new command like “delete `hello` at this position” as a compensation command.
  - Everyone still sees the full command history, but the net workspace state goes back to what it was before Alice's edit.
- The global application layer still exists, but it becomes a reducer and validator for commands, not a place where a user's undo can rewrite everyone else's past.

This keeps collaboration consistent without pretending that all users share the same undo intent.

## Ideas worth exploring next

- A branch comparison tool for undo-tree history with cherry-pick support.
- A preview-only agent mode that simulates edits before applying them.
- A stricter permission model with read-only analysis agents and narrow edit agents.
