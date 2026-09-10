# Bug -1 when having some semi big files, typing very fast sends me to the beginning of the buffer with the cursos
We will have to investigate the root cause of that and see if we maange to create tests that measure this time degradation 
(it was an md file, so it can also envole tree sitter, undo command, lots of possible causes really, filly written as a buffer)


## Bug -1 — typing fast in a semi-big file teleports the cursor to the start

**Root cause, ranked:**

**1. A global `undo-tree-changed` event causes a full-buffer stomp that races with in-flight typing — this is the actual teleport mechanism.**
- `gui_backend/src/ipc/editor.rs:35` — `apply_command` fires `app.emit("undo-tree-changed", ())` after *every single keystroke's command*, with no payload saying which file/command triggered it.
- `react_frontend/components/Editor.tsx:386-399` — every mounted `Editor` reacts unconditionally by calling `syncFromBackend()`, regardless of which file actually changed.
- `Editor.tsx:594-612` (`syncFromBackend`) — fetches `get_file_content` (an unserialized async call, *not* part of the ordered `sendQueue`), and if the result differs from the live CodeMirror doc, does a **full-buffer replace**: `{from: 0, to: doc.length, insert: content}`.
- Failure mechanism: while typing fast, `get_file_content` races the user's own next keystrokes. By the time it resolves, the buffer has usually moved on, so the content mismatches and CodeMirror does a delete-and-reinsert of the *entire* document. A cursor sitting inside a fully-deleted range gets remapped to the start of the new insertion — offset 0. That's the teleport.

**2. "Semi big" files specifically widen the race window — per-keystroke cost is O(file size).**
- `core/src/state.rs:327-329` (`content_hash`) — FNV-1a hash over the *entire* file, recomputed synchronously on every `apply_command`.
- `core/src/state.rs:201-203` — char→byte index conversion scans from buffer start, done twice per edit.
- All of this runs inside the `apply_command` Tauri handler while holding the state lock (`editor.rs:16-42`). Bigger files → slower round trip → wider window for (1)'s race to land mid-keystroke-burst.

**3. Markdown highlighting is unusually expensive and non-incremental — plausible secondary contributor, and why the user noticed this on a `.md` file specifically.**
- `core/src/parser.rs:231-262` (`get_highlight_captures_markdown`) runs **two full tree-sitter parses** (block + inline grammar) on every call, each via `parser.parse(content, None)` — the `None` means tree-sitter's incremental-edit API is never used, so it's a from-scratch full-file reparse, doubled versus single-grammar languages. Triggered by `Editor.tsx:364-368`'s 150ms debounce after every doc change, adding CPU contention on both sides during a fast-typing burst.

**Minor/low-confidence:** `core/src/persistence.rs:95-106` does a full state clone + serialize + synchronous `fs::write` every 100 log entries inside `apply_command` — could cause an occasional hitch but doesn't explain why the teleport is always *to zero* the way (1) does.

**Fix direction:** scope `undo-tree-changed` to carry a file path (or the triggering command's target), and have `Editor.tsx` ignore the event for the file the *local* edit just came from — diff against a content hash instead of blindly fetch-and-swapping. This is a correctness fix, not just perf; (2) and (3) are worth fixing regardless since they widen the window.

**Suggested tests:**
- A `tests/` crate benchmark timing `apply_command` + `get_highlights` round-trip on a synthetic ~50KB `.md` file under 100 rapid single-char edits with no artificial delay, asserting p99 latency stays under a threshold — same shape as the existing numeric-regression tests in `integration_cost_reduction.rs`. Catches (2)/(3) regressions numerically.
- (1) is a race condition, not really unit-testable without a CodeMirror-level harness — a scripted rapid-keystroke reproduction (WebDriver/Playwright dispatching N transactions with <20ms gaps) asserting `view.state.selection.main.head` never resets to 0 mid-burst is the more direct signal.

### User stories
- As a **Dev**, I want to type quickly in a large Markdown/code file without my cursor
  jumping to line 1, so that I don't lose my place and silently scatter keystrokes into
  the top of the document.
- As a **Dev**, I want editing latency in a ~50KB file to stay imperceptible, so that
  the editor feels native rather than laggy.
- As a **Dev**, I want a regression guard on this, so that a future change to the
  edit/sync path can't silently reintroduce the teleport.

### Approach
Root cause (see research doc): `apply_command` emits an untargeted `undo-tree-changed`
after every keystroke; every `Editor.tsx` reacts by fetching `get_file_content` and, on
mismatch, doing a full-document `{from:0, to:doc.length, insert}` replace — which remaps
a cursor inside the deleted range to offset 0. Rapid typing makes the fetched content
almost always stale, so the stomp fires constantly. O(file-size) per-keystroke work
(full-file hash + char→byte scans) and non-incremental double markdown parsing widen the
window on bigger `.md` files.

Three layers, independently valuable:
1. **Correctness (the teleport itself):** give `undo-tree-changed` a payload naming the
   file/command that caused it, and have `Editor.tsx` ignore the event for the file its
   *own* local edit just produced. When it must reconcile, diff against a content hash
   and apply a *minimal* CodeMirror change (or skip if equal) instead of a blind
   full-buffer swap that discards the selection. (no need also to compute hashes they are too expensive, only compute on Check points, commit points).
2. **Latency (why big files):** make `content_hash` and char→byte conversion incremental
   or cached rather than full-scan-per-keystroke; consider moving the periodic
   checkpoint `fs::write` off the `apply_command` critical path.
3. **Markdown parse cost:** pass the previous tree into `parser.parse(content, Some(old_tree))`
   so tree-sitter uses its incremental-edit API instead of a from-scratch double parse.

### Suggested implementation priority
1. **Do layer 1 first, alone.** It's the actual bug the user reported and it's a
   correctness fix — the teleport can happen even on a small file if the async fetch
   loses the race, so fixing latency without fixing the stomp would only make it *rarer*,
   not gone. Ship + verify with the scripted rapid-keystroke reproduction before touching
   anything else.
2. **Layer 2 next**, gated behind the benchmark below so the win is measured, not assumed.
3. **Layer 3 last** — it's the smallest real-world contributor (it adds CPU contention but
   doesn't itself move the cursor), and incremental tree-sitter is fiddly to get right;
   not worth risking a highlighting regression until 1–2 have removed the actual pain.
   (this must be done, tree-sitter is already a project made for incremental parsing!!)

Implement All this but first create the test that will validate the performance gains on editing.

Test: a `tests/`-crate benchmark timing `apply_command` + `get_highlights` over 100
rapid single-char edits on a synthetic ~50KB `.md` file, asserting p99 stays under a
threshold (same numeric-regression shape as `integration_cost_reduction.rs`). The
teleport itself will be only user validated.

At the end commit your changes