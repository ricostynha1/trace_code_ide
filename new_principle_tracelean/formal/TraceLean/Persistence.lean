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
  (commands.drop point.entries).foldl replayStep (.ok point.workspace)

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

theorem replay_step_error (rest : List Command) (e : Refusal) :
    rest.foldl replayStep (.error e) = .error e := by
  induction rest with
  | nil => rfl
  | cons c rest ih => simpa [List.foldl, replayStep] using ih

/-- @proves REQ-PERSIST.checkpoint_equivalent -/
theorem replaying_from_a_checkpoint_reaches_the_direct_state
    (base : Workspace) (lines : List String) (checkpointAt : Nat) :
    (replayReport base lines checkpointAt).viaCheckpoint =
      (replayReport base lines checkpointAt).direct := by
  simp only [replayReport, checkpoint, replayFrom]
  generalize (parseLog lines).commands = cs
  generalize min checkpointAt cs.length = n
  have split : replay base cs = (cs.drop n).foldl replayStep (replay base (cs.take n)) := by
    simp only [replay]
    rw [← List.foldl_append, List.take_append_drop]
  rw [split]
  cases h : replay base (cs.take n) with
  | error e => simp [replay_step_error, outcomeOfExcept]
  | ok w => rfl

/-- @proves REQ-PERSIST.replay_exact
@proves ARCH-DETERMINISM.replay_exact -/
theorem a_replay_applies_every_recorded_command_in_order
    (base : Workspace) (lines : List String) (checkpointAt : Nat) :
    (replayReport base lines checkpointAt).direct =
      outcomeOfExcept ((replayReport base lines checkpointAt).commands.foldl
        (fun acc c => acc.bind (fun w => apply w c)) (.ok base.canon)) := by
  have e : (fun (acc : Except Refusal Workspace) c => acc.bind (fun w => apply w c)) =
      (fun acc c => match acc with
        | .error e => .error e
        | .ok w => apply w c) := by
    funext acc c
    cases acc <;> rfl
  rw [e]
  rfl

theorem parse_step_extends (acc : Log × Nat × Bool) (line : String) :
    acc.1.commands <+: (parseLogStep acc line).1.commands := by
  obtain ⟨log, i, stopped⟩ := acc
  unfold parseLogStep
  cases stopped
  · simp only
    split
    · exact List.prefix_refl _
    · split
      · exact List.prefix_append _ _
      · exact List.prefix_refl _
  · exact List.prefix_refl _

theorem parse_extends : ∀ (lines : List String) (acc : Log × Nat × Bool),
    acc.1.commands <+: (lines.foldl parseLogStep acc).1.commands := by
  intro lines
  induction lines with
  | nil => intro acc; exact List.prefix_refl _
  | cons l _ ih =>
    intro acc
    exact (parse_step_extends acc l).trans (ih _)

theorem parse_stopped : ∀ (lines : List String) (acc : Log × Nat × Bool), acc.2.2 = true →
    (lines.foldl parseLogStep acc).1 = acc.1 := by
  intro lines
  induction lines with
  | nil => intro acc _; rfl
  | cons l ls ih =>
    intro acc h
    obtain ⟨log, i, stopped⟩ := acc
    simp only at h
    subst h
    exact ih _ rfl

theorem parse_marks : ∀ (lines : List String) (acc : Log × Nat × Bool),
    (acc.2.2 = true ↔ acc.1.truncatedAfter.isSome = true) →
    ((lines.foldl parseLogStep acc).2.2 = true ↔
      (lines.foldl parseLogStep acc).1.truncatedAfter.isSome = true) := by
  intro lines
  induction lines with
  | nil => intro acc h; exact h
  | cons l _ ih =>
    intro acc h
    apply ih
    obtain ⟨log, i, stopped⟩ := acc
    unfold parseLogStep
    cases stopped
    · simp only
      split
      · exact h
      · split
        · exact h
        · simp
    · exact h

/-- @proves REQ-PERSIST.append_only -/
theorem appending_lines_never_rewrites_what_was_recorded (lines more : List String) :
    (parseLog lines).commands <+: (parseLog (lines ++ more)).commands ∧
    ((parseLog lines).truncatedAfter.isSome = true →
      (parseLog (lines ++ more)).commands = (parseLog lines).commands ∧
      (parseLog (lines ++ more)).truncatedAfter = (parseLog lines).truncatedAfter) := by
  unfold parseLog
  simp only [List.foldl_append]
  refine ⟨parse_extends more _, fun ht => ?_⟩
  have hm := parse_marks lines ({ commands := [], truncatedAfter := none }, 0, false) (by simp)
  have hs := parse_stopped more _ (hm.mpr ht)
  rw [hs]
  exact ⟨rfl, rfl⟩

def entryOf (line : String) : Option Command :=
  if line.trim == "" then none else readEntry line

theorem parse_complete : ∀ (good : List String) (log : Log) (i : Nat),
    (∀ l ∈ good, l.trim = "" ∨ (readEntry l).isSome = true) →
    good.foldl parseLogStep (log, i, false) =
      ({ log with commands := log.commands ++ good.filterMap entryOf }, i + good.length, false) := by
  intro good
  induction good with
  | nil => intro log i _; simp
  | cons l ls ih =>
    intro log i hg
    have hl := hg l (List.mem_cons_self _ _)
    have hls : ∀ l ∈ ls, l.trim = "" ∨ (readEntry l).isSome = true :=
      fun l' h' => hg l' (List.mem_cons_of_mem _ h')
    simp only [List.foldl_cons]
    by_cases ht : l.trim = ""
    · have hstep : parseLogStep (log, i, false) l = (log, i + 1, false) := by
        simp [parseLogStep, ht]
      rw [hstep, ih _ _ hls]
      simp [entryOf, ht]
      omega
    · have hsome : (readEntry l).isSome = true := hl.resolve_left ht
      cases he : readEntry l with
      | none => rw [he] at hsome; cases hsome
      | some c =>
        have hstep : parseLogStep (log, i, false) l =
            ({ log with commands := log.commands ++ [c] }, i + 1, false) := by
          have hbeq : (l.trim == "") = false := by simp [ht]
          simp [parseLogStep, hbeq, he]
        rw [hstep, ih _ _ hls]
        have hbeq : (l.trim == "") = false := by simp [ht]
        have hen : entryOf l = some c := by simp [entryOf, hbeq, he]
        simp only [List.filterMap_cons, hen, List.append_assoc, List.singleton_append,
          List.length_cons]
        congr 2
        omega

/-- @proves REQ-PERSIST.truncated_is_reported -/
theorem a_record_cut_mid_entry_is_reported_and_replays_its_complete_entries (base : Workspace)
    (good : List String) (bad : String) (rest : List String) (at_ : Nat)
    (hg : ∀ l ∈ good, l.trim = "" ∨ (readEntry l).isSome = true) (hb : bad.trim ≠ "")
    (hr : readEntry bad = none) :
    (replayReport base (good ++ bad :: rest) at_).truncatedAfter = some good.length ∧
    (replayReport base (good ++ bad :: rest) at_).commands = good.filterMap entryOf ∧
    (replayReport base (good ++ bad :: rest) at_).direct =
      outcomeOfExcept (replay base (good.filterMap entryOf)) := by
  have hlog : parseLog (good ++ bad :: rest) =
      { commands := good.filterMap entryOf, truncatedAfter := some good.length } := by
    unfold parseLog
    simp only []
    rw [List.foldl_append, parse_complete good _ 0 hg, List.foldl_cons]
    have hstep : parseLogStep ({ commands := [] ++ good.filterMap entryOf, truncatedAfter := none },
        0 + good.length, false) bad =
        ({ commands := good.filterMap entryOf, truncatedAfter := some good.length },
          good.length + 1, true) := by
      have hbeq : (bad.trim == "") = false := by simp [hb]
      simp [parseLogStep, hbeq, hr]
    simp only [] at hstep ⊢
    rw [hstep, parse_stopped rest _ rfl]
  simp [replayReport, hlog]

end TraceLean.Persistence
