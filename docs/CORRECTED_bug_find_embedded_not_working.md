# Search tooling rewrite — implementation plan

Two problems motivated this, both from the old 9-parameter `find` tool:

1. **Semantic search never worked.** The embeddings pipeline is fully implemented but
   never called — `execute_tool` hardcodes `&None` for the index, so every semantic
   query fell back to the stale error "embeddings index not initialized. Build it first
   (rebuild context button)" (a button that no longer exists).
2. **`find` is too complex to call correctly.** 9 params (`query`, `mode`,
   `case_sensitive`, `max_results`, `offset`, `path`, `file_filter`, `max_depth`,
   `context`) with overlapping semantics. Models pass `*.py` into the regex `mode` and
   get a cryptic parse error, or confuse `query` (content) with `file_filter` (filename).

## Decision

Frontier models are already fluent with `grep`/`find` in a shell, and plain
content/filename search adds nothing over bash. The one thing bash *cannot* do is
meaning-based search. So:

- **Delete `find` entirely** — no backwards-compat alias.
- Indicate on system prompt to : Route exact/glob/regex search to the **shell** (`run_shell` with `grep`/`find`).
- Keep one dedicated search tool, **`find_semantic`**, backed by the auto-built
  embeddings index.

This only works if large shell output can't blow the context window and if the shell
sees current file content — hence the two supporting changes below.

## Work items

### 1. Delete `find`, add `find_semantic`
- Remove the `find` tool entry from `tracelean/data/tools.json` and its executor
  (`execute_find`) from `tool_executor.rs`.
- Add `find_semantic` — params: `query`,  optional `path`, REST PRAMTER OPTIONAL WITH DEFAULT max_results: 'results' default 30, 'is_recursive' defualt false, glob_filter_for_file_types defaulr * (so no filter). Backed by the embeddings
  index (item 2). Small schema, name disambiguates intent.
- Update the system prompt / tool docs: use the shell (`grep`, `find`) for
  exact/glob/regex search; use `find_semantic` for meaning-based search. Note that
  large shell output is auto-written to a file (item 3).

### 2. Auto-build the embeddings index on project open
- Building is **local and free** (fastembed `AllMiniLML6V2`, cached) — safe to auto-run,
  no API budget spent.
- Hook `EmbeddingsIndex::build` into `service.rs::open_project` on a background task.
- Store into a `SharedIndex` on `SharedApp`/`AiService` (mirror the
  `pending_diffs`/`resolved_diffs` pattern) and thread it into
  `execute_tool_with_index`, replacing the hardcoded `&None`.
- Delete the stale "rebuild context button" error text.

### 3. Spill large shell output to a file
Currently `execute_run_shell` returns raw combined output (capped only at the 4 MB
`MAX_CAPTURE_BYTES` reader limit); the advertised `head_lines`/`tail_lines` params are
**not implemented**. With search moving to the shell, unbounded `grep` output must be
contained.

- When combined output exceeds **500 lines or 30 KB**, write the full combined
  stdout+stderr to a file under **`.tracelean/shell_logs/`** and return a short preview
  plus a pointer: *"full output (N lines) written to `<path>` — use `read_file` with
  offset to inspect."*
- stdout+stderr are already combined in `shell_sandbox.rs` (`stdout` + `--- stderr ---`
  + `stderr`); write that same combined stream to the log file. Both streams always
  captured, spilled or not.
- Write the log file **outside the overlay** (real `.tracelean/shell_logs/`, not the
  sandboxed tree) so it isn't captured as a tracked mutation and doesn't pollute the
  review/diff stream.

### 4. Flush dirty buffers to disk before running a shell command
The shell reads files from disk, but edits can live in memory ahead of disk:
- **AI auto-apply edits** already `save_eff` to disk on every edit — safe.
- **AI review-staged edits** live only in the `ReviewSink`, in neither buffer nor disk —
  must *not* be force-saved (defeats review isolation); flushing buffers won't touch them.
- **Human editor edits** update buffers via `apply_command` but are **not** written to
  disk until explicit save (Ctrl+S) — this is the real gap: `grep`/`find` would read
  stale disk content.

Fix: before running a shell command, flush **dirty open buffers** (buffer content
differing from disk) to disk — a scoped "save all". No-op for already-saved AI edits,
safe for review mode (staged changes aren't in buffers). Ignore the rare
buffer-behind-external-disk-change case for now.

### 5. Remove pagination from all tools except `read_file`
`read_file` stays the one paginated tool (offset + capped `max_results`, truncation
hint). Everything else drops `offset`/`max_results` pagination — when output is large,
item 3's spill-to-file kicks in and the model reads the file back through the
pagination-protected `read_file`. This shrinks tool surface: the model only needs to
know `read_file` well.

## Sequencing
1. **Spill-to-file (item 3)** first — unblocks everything, independently useful.
2. **Flush-before-shell (item 4)** — makes shell search correct.
3. **Embeddings auto-build (item 2)** — must land before `find_semantic` has anything
   to query.
4. **Delete `find` + add `find_semantic` (item 1)** + **remove pagination (item 5)** —
   the visible tool-surface change, last.
