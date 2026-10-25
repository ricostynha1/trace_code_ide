import TraceLean.Command

namespace TraceLean.Persistence

open TraceLean.Command

open Lean (ToJson FromJson Json fromJson?)

structure Checkpoint where
  workspace : Workspace
  entries : Nat
  deriving Repr, Inhabited, ToJson, FromJson

structure Log where
  commands : List Command
  truncatedAfter : Option Nat
  deriving Repr, Inhabited, ToJson, FromJson

def readEntry (line : String) : Option Command :=
  match Json.parse line with
  | .error _ => none
  | .ok j => match fromJson? (α := Command) j with
    | .ok c => some c
    | .error _ => none

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

/-- @models REQ-PERSIST.append_only -/
def parseLog (lines : List String) : Log :=
  let start : Log × Nat × Bool := ({ commands := [], truncatedAfter := none }, 0, false)
  (lines.foldl parseLogStep start).1

private def replayStep (acc : Except Refusal Workspace) (c : Command)
    : Except Refusal Workspace :=
  match acc with
  | .error e => .error e
  | .ok w => apply w c

def replay (base : Workspace) (commands : List Command) : Except Refusal Workspace :=
  commands.foldl replayStep (.ok base.canon)

def checkpoint (base : Workspace) (commands : List Command) (entries : Nat)
    : Except Refusal Checkpoint :=
  match replay base (commands.take entries) with
  | .error e => .error e
  | .ok w => .ok { workspace := w, entries := entries }

def replayFrom (point : Checkpoint) (commands : List Command) : Except Refusal Workspace :=
  replay point.workspace (commands.drop point.entries)

structure ReplayReport where
  commands : List Command
  truncatedAfter : Option Nat
  direct : Outcome
  viaCheckpoint : Outcome
  deriving Repr, Inhabited, ToJson, FromJson

def outcomeOfExcept : Except Refusal Workspace → Outcome
  | .ok w => .ok w
  | .error r => .refused r.kind

/-- @models REQ-PERSIST.replay_exact
@models ARCH-DETERMINISM.replay_exact
@models REQ-PERSIST.checkpoint_equivalent
@models REQ-PERSIST.truncated_is_reported -/
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

/-- @proves REQ-PERSIST.checkpoint_equivalent -/
theorem a_checkpoint_over_the_whole_log_replays_nothing
    (w : Workspace) (commands : List Command) :
    replayFrom { workspace := w, entries := commands.length } commands = .ok w.canon := by
  simp [replayFrom, replay]

/-- @proves REQ-PERSIST.replay_exact
@proves ARCH-DETERMINISM.replay_exact -/
theorem replay_is_each_command_in_turn (base : Workspace) (commands : List Command) (last : Command) :
    replay base [] = .ok base.canon ∧
    replay base (commands ++ [last]) = replayStep (replay base commands) last := by
  constructor
  · simp [replay]
  · simp [replay, List.foldl_append]

end TraceLean.Persistence
