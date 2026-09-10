## Bug 0 — three bundled symptoms

### A. Save checkpoint doesn't sync open buffers to disk

`save_checkpoint` (`gui_backend/src/ipc/editor.rs:233-250`) only calls `mark_commit_point` and `persistence::save_checkpoint`, which serializes the **in-memory** `AppState` (all open buffers) to `.tracelean/commands/checkpoint.json` — TraceLean's own undo-tree snapshot, not the real project files. Ordinary edits applied via `apply_command` only mutate the in-memory buffer; they're never flushed to the actual file path except via explicit `save_file` (Ctrl+S, `Editor.tsx:669-683`) or `sync_file_operations_to_disk`, which only reconciles create/delete/rename paths, not ordinary content edits. So a dirty buffer the user hasn't hit Ctrl+S on is invisible to anything reading the real filesystem — including the agent's `run_shell`.

**Fix direction:** in `save_checkpoint`, before persisting, iterate open files and write each buffer's current content to disk (reuse the write body `save_file` already has at `service.rs:123-140`), then persist the checkpoint as today.

### B. Diff staleness error on shell-driven edits

The error (`EditorDiffBar.tsx:271-276`) fires on `AppState::apply`'s witness-mismatch check — a staged `Replace`'s baked-in "original" text no longer matches the live buffer. Root cause: `run_overlay` (`core/src/ai/shell_sandbox.rs:315+`) spins up a **fresh** overlayfs against the real on-disk tree on *every single shell invocation*. In review mode, `materialize_sandbox_run` (`tool_executor.rs:1526-1540`) stages each mutated file's pre/post straight from that overlay run and never touches the in-memory buffer. So if the same `run_shell` command fires multiple times before any diff is accepted (see Symptom C — this is a direct side effect of it), every run diffs against the same real-tree "pre," producing several diffs with identical baked-in originals. Accepting the first applies it to buffer+disk; every other still-pending diff's stored original is now stale, and accepting *those* trips the witness check.

**Fix direction:** either (a) reject/queue a new diff for a path that already has an unresolved pending diff, or (b) re-diff `PendingDiff`s against the current buffer at accept time and auto-rebase the hunks instead of hard-failing.

**Suggested Implementation priority:**

---
### C. Agent repeats the identical `run_shell` call, including after rejection

No duplicate-call detection exists anywhere in `core/src/agent/runtime.rs` (confirmed by grep — nothing tracks repeated `(tool_name, arguments)` pairs). The rejection message itself is unambiguous (`"[run_shell rejected by user — command was NOT executed: {cmd}]"`, `runtime.rs:802-805`). The more likely trigger is the *prior*, accepted-but-still-staged results: `materialize_sandbox_run`'s success note is just `"{path} (modified)"` (`tool_executor.rs:1556`) — under review mode the change is only staged, not applied yet, and that terse note doesn't say so, so a model could plausibly read it as "not confirmed, retry to be sure." This directly compounds Symptom B (each retry stages another diff against the same stale "pre").

**Fix direction:** (1) a lightweight same-turn duplicate-tool-call guard — after N identical `(name, args)` pairs, inject a system-authored note telling the model the call already ran, stop retrying; (2) make the shell-mutation note state explicitly say `"{path} modified — staged for review user accepted / or rejected changes (134 chard added, 20 deleted on)"` instead of the ambiguous `"(modified)"`.


### User stories
- As an **Agent-user**, I want "Save checkpoint" to persist every open dirty buffer to
  disk, so that when the agent then reads/edits those files via `run_shell` it sees the
  same content I see on screen.
- As an **Agent-user**, I want to actually be able to *accept* an agent's shell-driven
  edit, so that review mode isn't a dead end that only ever shows "this diff no longer
  matches the file."
- As an **Agent**, I want an unambiguous, information-rich result after a mutating
  `run_shell` (what changed, and whether it's applied or still staged), so that I don't
  re-issue the same command in a loop thinking it didn't take.
- As an **Agent-user**, I want a runaway identical-tool-call loop to self-arrest, so that
  a confused model doesn't burn my budget echoing the same command dozens of times.

### Approach (per symptom, from research doc)
- **A (checkpoint):** in `save_checkpoint`, before persisting, iterate open files and
  write each buffer's current content to the real path (reuse `save_file`'s write body,
  `service.rs:123-140`), then persist the checkpoint as today.
- **B (stale diff):** the same `run_shell` restages a diff against a fresh overlay's
  pre-image every call, so once one is accepted the others' baked-in originals are stale.
  Fix: either (a) refuse/queue a *new* pending diff for a path that already has an
  unresolved one, or (b) re-diff pending hunks against the current buffer at accept time
  and auto-rebase instead of hard-failing. Prefer (a) — it also structurally prevents the
  duplicate-staging that symptom C produces.
- **C (retry loop):** two parts —
  1. A same-turn duplicate-tool-call guard in `runtime.rs`: track recent `(name, args)`
     pairs; after N identical calls inject a system-authored note ("this exact call already
     ran; its result is above — do not repeat it") and skip re-execution.
  2. Replace the terse `"{path} (modified)"` note. Per the user's own edit to the research
     doc, the message should state the *outcome and magnitude*, e.g.
     `"{path} — staged for review (≈134 chars added, 20 removed); (note the agent will never be given the message waiting for user input, the part of the program responsible for giving the agent a new reponse will just block, and when user aproves rejects everything than the answer is geenrated for the agent) and, once resolved, report the accept/reject result with the same char-delta detail —
     reusing the `ResolvedDiffOutcome` plumbing already built for Bug 2 last session
     (`applied_message` / accepted-vs-total hunks) rather than inventing a new channel.

### Suggested implementation priority
1. **C.2 (the message) first.** It's the cheapest change and, per the research, the most
   *likely* actual trigger of the loop the user saw — the model misreading "(modified)" as
   "not confirmed." Fixing the signal may resolve the observed loop on its own.
2. **C.1 (the guard) second**, as the safety net for when a model loops for *other*
   reasons — belt-and-suspenders, but it needs the guard to be conservative (only fire on
   *identical* args, several times) so it never blocks legitimate repeated commands.
3. **B (stale diff) third.** Higher effort (touches the sandbox/overlay staging path) and
   option (a) partly depends on the same "one pending diff per path" bookkeeping the guard
   in C.1 makes natural. Doing C first shrinks how often B even triggers.
4. **A (checkpoint sync) last** of this group — it's independent and simple, but it's the
   least urgent of the four since Ctrl+S already gives the user a manual workaround today;
   it's a papercut, not a dead end like B.
   
   So impleemnt only C.2 and C.1