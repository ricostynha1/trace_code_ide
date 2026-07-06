# Requirements Document

## Introduction

Fix correctness gaps in undo tree, snapshots, and persistence (`commands.rs`, `undo_tree.rs`, `persistence.rs`). Remove redundant `Batch` variant, turn commit points into first-class snapshot nodes reachable from the undo-tree UI, compact repeated char-level edits, run a (stub) validation pipeline on snapshot, and persist the tree compactly instead of full-checkpoint + flat log.

## Glossary

- **Command**: enum in `commands.rs`, one state mutation + its inverse.
- **UndoTree / UndoNode**: tree structure in `undo_tree.rs`; node = one applied command.
- **Snapshot**: user-triggered node marking a save point (replaces today's `CommitPoint`/`commit_point` naming).
- **ValidationPipeline**: fn run at snapshot creation, returns pass/fail + test counts.
- **AppState**: `state.rs`, sole mutation path is `apply(Command)`.
- **Compaction**: merging adjacent single-char Insert/Delete commands into one word-level command.

## Requirements

### Requirement 1: Remove `Batch` command variant

**User Story:** As a maintainer, I want `Batch` removed from `Command`, so atomic multi-command groups are expressed as tree structure, not a redundant enum wrapper.

#### Acceptance Criteria

1. THE `Command` enum SHALL NOT contain a `Batch` variant.
2. WHEN multiple commands must apply atomically, THE UndoTree SHALL represent them as sibling-free sequential nodes pushed together (single parent, chained children) rather than as one `Batch` command.
3. THE UndoTree SHALL expose a method to push a sequence of commands as one atomic group (single undo/redo step across the group).
4. Existing callers of `Command::Batch` SHALL be migrated to the new atomic-group push method with no behavior change to callers' undo/redo semantics.

### Requirement 2: Snapshot nodes distinct from ordinary undo nodes

**User Story:** As a user, I want save points marked as snapshots distinct from ordinary edit nodes, so I can navigate directly to them in the undo-tree UI.

#### Acceptance Criteria

1. THE `UndoNode` SHALL carry a field distinguishing snapshot nodes from ordinary nodes (e.g. `is_snapshot: bool` or `snapshot: Option<Snapshot>`), replacing today's `commit_point: Option<CommitPoint>`.
2. WHEN a node is a snapshot, THE `Snapshot` metadata SHALL include: timestamp, label, touched files, test results.
3. THE existing `CommitPoint` struct and `commit_point` field/naming SHALL be renamed to `Snapshot`/`snapshot` throughout `undo_tree.rs` and any UI-facing serialization.
4. THE undo-tree query/visualization API SHALL allow filtering or listing nodes where `is_snapshot` is true, so the UI's "commit" navigation view can list snapshots instead.

### Requirement 3: Snapshot trigger and metadata capture

**User Story:** As a user, I want to create a snapshot only via explicit button press, with metadata captured automatically.

#### Acceptance Criteria

1. Snapshot creation SHALL only be invocable via an explicit user-triggered command (no automatic/periodic snapshotting).
2. WHEN a snapshot is created, THE system SHALL record: current timestamp, a user-supplied or default label, the set of files touched since the previous snapshot (or since tree start if none).
3. WHEN a snapshot is created, THE system SHALL record test results as `0 tests passed` (stub) until a real test runner exists.

### Requirement 4: Validation pipeline on snapshot

**User Story:** As a maintainer, I want snapshot creation to call a validation pipeline, so future gating (parse/compile/test) has a hook point now.

#### Acceptance Criteria

1. WHEN a snapshot is created, THE system SHALL call a validation function before or during snapshot recording.
2. THE stub validation function SHALL return `ValidationResult { tests_passed: 0, tests_total: 0, status: Pass }` unconditionally for now.
3. THE validation function SHALL be structured (e.g. as a trait or ordered stages) so parsing-check, compilation-check, and test-execution stages can be added later without changing the snapshot-creation call site's signature.
4. THE `ValidationResult` SHALL be stored on or alongside the `Snapshot` metadata.

### Requirement 5: Compaction of contiguous single-char edits

**User Story:** As a maintainer, I want repeated single-char inserts/deletes compacted into word-level commands, so the undo tree doesn't balloon with one node per keystroke.

#### Acceptance Criteria

1. WHEN a new `Insert` command targets the same file and its offset is immediately adjacent (end of previous insert) to the current node's `Insert` command, AND the current node is not a snapshot, THE UndoTree SHALL merge the new insert into the current node's text instead of pushing a new node.
2. WHEN a new `Delete` command targets the same file and its offset is immediately adjacent to the current node's `Delete` command, AND the current node is not a snapshot, THE UndoTree SHALL merge it the same way.
3. Compaction SHALL NOT merge across a snapshot node (a snapshot always starts a fresh node boundary).
4. Compaction SHALL NOT merge an `Insert` into a `Delete` node or vice versa.
5. Merging SHALL update the node's `inverse` command to remain the correct inverse of the merged (word-level) command.

### Requirement 6: Compact undo-tree persistence

**User Story:** As a maintainer, I want the undo tree persisted compactly, so disk usage and startup replay stay small as history grows.

#### Acceptance Criteria

1. THE persistence format SHALL store snapshot nodes with full metadata (as today's checkpoint does for full state).
2. THE persistence format SHALL store non-snapshot nodes in a compact encoding rather than one full JSON `Command` object per keystroke-level edit.
3. A concrete compact encoding SHALL be proposed and reviewed before implementation (see design doc) — not implemented speculatively without sign-off.
4. THE persisted format SHALL preserve tree branch structure (parent/child links), not just a flat replay-order list.
5. Loading a persisted tree SHALL reconstruct an `UndoTree` identical (nodes, parent/child links, current position) to the one that was saved.

### Requirement 7: Test coverage

**User Story:** As a maintainer, I want the above behaviors covered by tests.

#### Acceptance Criteria

1. THE test suite SHALL cover insert/delete compaction correctness (adjacent merges happen; non-adjacent/cross-file/cross-snapshot do not).
2. THE test suite SHALL cover branching: undo then new edit creates a new branch (sibling), original branch remains reachable.
3. THE test suite SHALL cover snapshot creation: metadata stored correctly, validation pipeline invoked.
4. THE test suite SHALL cover persistence round-trip: save tree → reload → identical nodes, links, and current position.
5. THE test suite SHALL cover agent-generated edits (via a mock agent / constructed `ToolCall`s, no real provider) being undoable deterministically.
