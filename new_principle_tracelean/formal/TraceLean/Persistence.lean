import TraceLean.Command

/-!
# Persistence and replay

Models `REQ-PERSIST`. A history that does not reproduce its own state is a log,
not a record.

Two properties, and both are about equivalences rather than features. Replaying
a log must reach exactly the state it was recorded from. And a checkpoint --
which exists only to make that faster -- must reach the same state as replaying
from the beginning, which is why it is specified against the unoptimised path
instead of on its own terms.

The third is about failure. A process killed mid-write leaves a half entry.
Refusing to open the project is unrecoverable; silently accepting the half entry
writes corruption over a good state. Replaying up to the last complete entry and
saying so is the only option that loses nothing.
-/

namespace TraceLean.Persistence

open TraceLean.Command

open Lean (ToJson FromJson Json fromJson?)

/-- A recorded history: a base state and how much of the log it accounts for. -/
structure Checkpoint where
  workspace : Workspace
  entries : Nat
  deriving Repr, Inhabited, ToJson, FromJson

/-- What a log yielded, and what was wrong with it. -/
structure Log where
  commands : List Command
  /-- Set when the record ends mid-entry.

  @models REQ-PERSIST.truncated_is_reported -/
  truncatedAfter : Option Nat
  deriving Repr, Inhabited, ToJson, FromJson

/-- One log entry, or nothing. -/
def readEntry (line : String) : Option Command :=
  match Json.parse line with
  | .error _ => none
  | .ok j => match fromJson? (α := Command) j with
    | .ok c => some c
    | .error _ => none

/-- One line of a log, read into what has been read so far.

A definition rather than a lambda with a typed binder holding a `match`, neither
of which the grammar that reads these annotations can read (ADR-0008). -/
private def parseLogStep (acc : Log × Nat × Bool) (line : String) : Log × Nat × Bool :=
  let log := acc.1
  let index := acc.2.1
  let stopped := acc.2.2
  match stopped with
  | true => (log, index + 1, true)
  | false =>
    match line.trim == "" with
    | true => (log, index + 1, false)
    | false =>
      match readEntry line with
      | some c => ({ log with commands := log.commands ++ [c] }, index + 1, false)
      | none => ({ log with truncatedAfter := some index }, index + 1, true)

/-- Parse an append-only log of one command per line.

The first unreadable entry ends the history: everything after it was written
after a failure and cannot be trusted to follow from what came before. Blank
lines are skipped but still counted, so the reported index is a line number.

@models REQ-PERSIST.append_only -/
def parseLog (lines : List String) : Log :=
  let start : Log × Nat × Bool := ({ commands := [], truncatedAfter := none }, 0, false)
  (lines.foldl parseLogStep start).1

/-- Apply one command to a replay that has not already refused. -/
private def replayStep (acc : Except Refusal Workspace) (c : Command)
    : Except Refusal Workspace :=
  match acc with
  | .error e => .error e
  | .ok w => apply w c

/-- Replay commands onto a base state.

@models REQ-PERSIST.replay_exact
@models ARCH-DETERMINISM.replay_exact -/
def replay (base : Workspace) (commands : List Command) : Except Refusal Workspace :=
  commands.foldl replayStep (.ok base.canon)

/-- Take a checkpoint after `entries` commands. -/
def checkpoint (base : Workspace) (commands : List Command) (entries : Nat)
    : Except Refusal Checkpoint :=
  match replay base (commands.take entries) with
  | .error e => .error e
  | .ok w => .ok { workspace := w, entries := entries }

/-- Replay from a checkpoint, skipping the entries it already accounts for.

@models REQ-PERSIST.checkpoint_equivalent -/
def replayFrom (point : Checkpoint) (commands : List Command) : Except Refusal Workspace :=
  replay point.workspace (commands.drop point.entries)

/-- What replaying a log produced, both ways. -/
structure ReplayReport where
  commands : List Command
  truncatedAfter : Option Nat
  direct : Outcome
  viaCheckpoint : Outcome
  deriving Repr, Inhabited, ToJson, FromJson

def outcomeOfExcept : Except Refusal Workspace → Outcome
  | .ok w => .ok w
  | .error r => .refused r.kind

/--
Parse a log, then replay it twice: straight through, and from a checkpoint.

@models REQ-PERSIST.replay_exact
@models ARCH-DETERMINISM.replay_exact
@models REQ-PERSIST.checkpoint_equivalent
@models REQ-PERSIST.truncated_is_reported
-/
def replayReport (base : Workspace) (lines : List String) (checkpointAt : Nat) : ReplayReport :=
  let log := parseLog lines
  let direct := outcomeOfExcept (replay base log.commands)
  let taken := min checkpointAt log.commands.length
  let viaCheckpoint :=
    match checkpoint base log.commands taken with
    | .error r => Outcome.refused r.kind
    | .ok point => outcomeOfExcept (replayFrom point log.commands)
  { commands := log.commands, truncatedAfter := log.truncatedAfter,
    direct := direct, viaCheckpoint := viaCheckpoint }

/-- A checkpoint that accounts for the whole log has nothing left to replay, so
it is the state the log reached. The differential test generalises this to every
cut point.

@proves REQ-PERSIST.checkpoint_equivalent -/
theorem a_checkpoint_over_the_whole_log_replays_nothing
    (w : Workspace) (commands : List Command) :
    replayFrom { workspace := w, entries := commands.length } commands = .ok w.canon := by
  simp [replayFrom, replay]

end TraceLean.Persistence
