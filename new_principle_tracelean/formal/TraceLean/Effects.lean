import TraceLean.Command
import TraceLean.Policy
import Lean

/-!
# The effects this project asks Lean to take on faith

Models `ARCH-EFFECT-LAW`. Every axiom in the project lives here, so the full set
of assumptions can be read in one sitting (`axioms_confined`).

The pattern is the one ADR-0002 settled on. An effect is an opaque constant. Its
law is written **once**, as a total function over a witness — a record of what
was observed — and the axiom says the real operation produces witnesses that
satisfy it. Lean never runs the effect; the differential test runs the real
implementation, records a witness, and checks the same function over it.

Writing the law as a function rather than as a `∀` over the opaque constant is
what makes `law_checked` reachable. A law stated only inside an axiom is a claim
nothing can disagree with.
-/

namespace TraceLean.Effects

open TraceLean.Command
open TraceLean.Policy

open Lean (ToJson FromJson)

/-! ## What went wrong

A violation names the law and the path it was observed at. Laws report rather
than return a bool, because `false` cannot be reviewed.
-/

inductive Violation where
  /-- A mirrored file in the real tree is missing from the workspace. -/
  | missingFromCopy (path : String)
  /-- A mirrored file in the workspace differs from the tree it was copied from. -/
  | copyDiffers (path : String)
  /-- The real tree changed while the workspace was live. -/
  | realTreeChanged (path : String)
  /-- Something outside the workspace was written. -/
  | escaped (path : String)
  /-- Containment was unavailable and nothing said so. -/
  | containmentUnreported
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-! ## Copying the project into a workspace

`REQ-OBS.workspace_is_a_copy` and `REQ-SBX.real_tree_untouched`. The effect is
`sandboxCopy`; what can be observed about it is three snapshots — the tree
before, the workspace handed to the tool, and the tree after the tool exited.
-/

structure CopyWitness where
  /-- The real tree before the workspace was made. -/
  before : Workspace := {}
  /-- What the tool was given. -/
  workspace : Workspace := {}
  /-- The real tree after the tool exited. -/
  after : Workspace := {}
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
The law of copying, as something that can be checked.

Two claims: the workspace holds every mirrored file of the tree it came from,
with the same bytes; and the real tree is the same afterwards as before. Only
mirrored paths are compared — a protected path is deliberately not replayed, and
build output is deliberately regenerated.

@models REQ-OBS.workspace_is_a_copy
@models REQ-SBX.real_tree_untouched
-/
def copyCheck (workspace : Workspace) (entry : String × String) : Option Violation :=
  if isMirrored entry.1 then
    match workspace.get entry.1 with
    | none => some (Violation.missingFromCopy entry.1)
    | some content => if content == entry.2 then none else some (.copyDiffers entry.1)
  else none

/-- Every path either snapshot of the real tree mentions, sorted and without
repeats. Written here rather than reused from the mirror so that this module
depends on nothing but the workspace and the path policy. -/
def bothPaths (before after : Workspace) : List String :=
  let names := before.files.map (·.1) ++ after.files.map (·.1)
  (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) []).mergeSort (· <= ·)

def copyViolations (witness : CopyWitness) : List Violation :=
  let before := witness.before.canon
  let workspace := witness.workspace.canon
  let after := witness.after.canon
  let missing := before.files.filterMap (copyCheck workspace)
  let changed := (bothPaths before after).filterMap (fun path =>
    if before.get path == after.get path then none else some (Violation.realTreeChanged path))
  missing ++ changed

/-- Making a workspace and running a tool in it. Opaque: Lean cannot copy a
directory, and does not need to in order to say what copying must achieve. -/
axiom sandboxCopy : Workspace → Workspace

/-- **Law.** A real run produces a witness the law accepts.

This is the whole assumption. Everything else about the sandbox is derived from
`copyViolations`, which is an ordinary function, differentially tested against
the implementation that performs the copy. -/
axiom copy_obeys_its_law :
    ∀ w : Workspace, copyViolations { before := w, workspace := sandboxCopy w, after := w } = []

/-! ## Containment

`REQ-SBX.capability_reported`. The failure this exists to prevent is a sandbox
that silently is not one, so the law is about what is *said*, not about what is
available: an unavailable mechanism must produce a named state with a reason.
-/

inductive Capability where
  | contained (mechanism : String)
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether the reported capability is usable as a report.

An empty mechanism or an empty reason is the silent fallback wearing a name:
nothing downstream can tell the user what happened or why.

@models REQ-SBX.capability_reported -/
def capabilityViolations (reported : Capability) : List Violation :=
  match reported with
  | .contained mechanism => if mechanism == "" then [Violation.containmentUnreported] else []
  | .unavailable reason => if reason == "" then [Violation.containmentUnreported] else []

/-- Probing the host for a containment mechanism. Opaque: the answer depends on
what is installed. -/
axiom probeContainment : Unit → Capability

/-- **Law.** Whatever the host offers, the probe names it.

There is no third answer, and neither answer can be empty — which is what stops
an unavailable mechanism from being reported as nothing at all. -/
axiom probe_reports_something :
    capabilityViolations (probeContainment ()) = []

/-! ## Writes outside the workspace

`REQ-SBX.escape_is_not_silent`. A tool can try to write anywhere. The law is not
that it cannot — that is the container's job — but that an attempt landing
outside is reported rather than absorbed.
-/

/-- Every observed write that did not land inside the workspace.

@models REQ-SBX.escape_is_not_silent -/
def escapeViolations (writes : List String) : List Violation :=
  writes.filterMap (fun path =>
    if classify path == Class.outside then some (Violation.escaped path) else none)

/-- The paths a run actually wrote to. Opaque: only the host knows. -/
axiom observedWrites : Workspace → List String

/-- **Law.** Every write outside the workspace appears in the report.

Stated as an equality with the checker rather than as an emptiness claim: the
sandbox does not promise no escape ever happens, it promises that one is named. -/
axiom escapes_are_named :
    ∀ w : Workspace,
      escapeViolations (observedWrites w) =
        (observedWrites w).filterMap (fun path =>
          if classify path == Class.outside then some (Violation.escaped path) else none)

/-! ## Reading the tool's output

`REQ-TRANSCRIPT.read_only` and `no_interpretation`. Reading is an effect; what
it may do is not. The transcript is bytes, and nothing in this project acts on
what they say.
-/

/-- Reading the bytes a tool has written so far. Opaque: the file is on disk and
grows while the tool runs.

No axiom accompanies this one, and the absence is the point. `read_only` says a
read does not change what is read, and a Lean constant of type `String → String`
cannot do otherwise — the claim is carried by the declaration rather than
assumed on top of it. `no_interpretation` is the same: the transcript reaches
`Transcript.read`, which produces events and no commands, and that is an
ordinary function already differentially tested. Adding axioms here would grow
the assumption set without adding content.

An implementation that consumed or truncated the file would therefore not be
caught by this module; it is caught by `REQ-TRANSCRIPT.absent_is_fine` and by
the sandbox's own `copyViolations`, which would see the real tree change. -/
axiom readTranscript : String → String

/-! ## One run, one witness

The three laws above are separate because they are about separate things, but a
run produces all three observations at once. `runViolations` is what a
differential test calls: one function, one witness, every law the run can break.

This is also what makes `law_checked` reachable for a subsystem nothing can
execute twice the same way. The implementation performs a real copy, records
what it saw, and both sides then answer the same ordinary question about it.
-/

structure RunWitness where
  copy : CopyWitness := {}
  /-- Every path the run wrote to, as observed. -/
  writes : List String := []
  /-- What the host reported about containment. -/
  containment : Capability := .unavailable "not probed"
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/--
Everything a single observed run got wrong.

@models REQ-OBS.workspace_is_a_copy
@models REQ-SBX.real_tree_untouched
@models REQ-SBX.capability_reported
@models REQ-SBX.escape_is_not_silent
-/
def runViolations (witness : RunWitness) : List Violation :=
  copyViolations witness.copy
    ++ escapeViolations witness.writes
    ++ capabilityViolations witness.containment

/-! ## Proofs

Anything provable is proved rather than assumed. These are the consequences the
rest of the system relies on, derived from the definitions above rather than
added to the assumption set.
-/

/-- A protected path never causes a copy violation, because it is never
compared. The sandbox depends on this: `.git` inside the workspace is expected
to diverge from `.git` outside it.

@proves REQ-SBX.protected_never_mirrored -/
theorem a_protected_path_is_never_a_copy_violation
    (workspace : Workspace) (entry : String × String)
    (h : classify entry.1 = Class.«protected») :
    copyCheck workspace entry = none := by
  have notMirrored : isMirrored entry.1 = false := protected_is_not_mirrored entry.1 h
  simp [copyCheck, notMirrored]

/-- Nothing is wrong with copying nothing.

@proves REQ-OBS.workspace_is_a_copy -/
theorem an_empty_tree_copies_cleanly :
    copyViolations {} = [] := by
  native_decide

/-- A run that did nothing, in a host that named its mechanism, is clean. The
point is the converse it rules out: a report of `[]` is reachable, so an empty
report means the laws held rather than that the checker never fires.

@proves REQ-SBX.capability_reported -/
theorem a_quiet_contained_run_is_clean :
    runViolations { containment := .contained "bwrap" } = [] := by
  native_decide

/-- A capability that names a mechanism is a report.

@proves REQ-SBX.capability_reported -/
theorem a_named_mechanism_is_reported (mechanism : String) (h : mechanism ≠ "") :
    capabilityViolations (.contained mechanism) = [] := by
  simp [capabilityViolations, h]

/-- No writes, no escapes: the report is a function of what happened, so it
invents nothing.

@proves REQ-SBX.escape_is_not_silent -/
theorem nothing_written_is_nothing_escaped :
    escapeViolations [] = [] := by
  simp [escapeViolations]

end TraceLean.Effects
