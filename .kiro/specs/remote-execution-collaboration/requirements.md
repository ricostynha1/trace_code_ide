# Requirements — Remote Execution & Collaboration

Source: docs/IMPLEMENTATION.md MVP8 (all unchecked) + problemns.md section 5 (compensation-command undo model, semantic ownership concerns).

## Requirements

| ID | Requirement |
|----|-------------|
| RC-01 | Local IDE streams Commands (not raw file mutations) to remote/peer machines. |
| RC-02 | Command stream uses a compact binary serialization protocol. |
| RC-03 | Transport is WebSocket, local ↔ remote and peer ↔ peer. |
| RC-04 | Remote machine replays the same command stream and reaches identical `AppState`. |
| RC-05 | Heavy tasks (Lean compile, full test suite, coverage) can be routed to remote execution, configurable per-task-type. |
| RC-06 | Remote task completion returns a result command (`SetCompileOutput`, `SetTestResults`) applied through the normal `apply()` pipeline on both ends. |
| RC-07 | On disconnect, local buffers outgoing commands; on reconnect, buffered commands sync in order. No data loss. |
| RC-08 | Multiple users can bidirectionally stream commands to collaborate on the same project. |
| RC-09 | Concurrent command ordering is deterministic across all peers, keyed by `(timestamp, user_id)`. |
| RC-10 | The shared history is a single canonical, append-only log. No branch rewriting for undo. |
| RC-11 | Per-user undo does not mutate or remove entries from the shared log. Instead, undo emits a new **compensation command** that reverses the net effect of the user's prior edit. |
| RC-12 | Compensation commands are computed against current state at undo time, not naively replaying the stored inverse blindly — later dependent edits by other users must be accounted for (best-effort; conflicts flagged, not silently corrupted). |
| RC-13 | Each command carries a `user_id` so per-user undo scope (which commands are "mine") is derivable from the shared log. |
| RC-14 | Presence: cursor/selection commands from other users are displayed live in the editor. |
| RC-15 | Optional hub/relay container rebroadcasts commands for NAT traversal or larger teams; hub has no authority over ordering or content. |
| RC-16 | All remote/collaboration connections are authenticated and encrypted (SSH or TLS/token-based). |

## Explicit design decision (supersedes earlier branch-based per-user undo idea)

Per problemns.md section 5: undo must NOT be a rewind of shared history. Global history stays linear and canonical. Per-user undo is a **projection**: emit compensating commands, never rewrite the past. This directly resolves REQ-61 (per-user undo) in REQUIREMENTS.md without introducing per-user branches in the shared log.

## Out of scope

- CRDT-based conflict-free merging (explicitly rejected in favor of deterministic ordering + compensation).
- Offline-first multi-day divergence resolution beyond simple reconnect-and-replay.
