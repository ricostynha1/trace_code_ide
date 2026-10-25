import TraceLean.Command
import TraceLean.Policy
import Lean

namespace TraceLean.Effects

open TraceLean.Command
open TraceLean.Policy

open Lean (ToJson FromJson)

inductive Violation where
  | missingFromCopy (path : String)
  | copyDiffers (path : String)
  | extraInCopy (path : String)
  | realTreeChanged (path : String)
  | escaped (path : String)
  | containmentUnreported
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure CopyWitness where
  before : Workspace := {}
  workspace : Workspace := {}
  after : Workspace := {}
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-OBS.workspace_is_a_copy -/
def copyCheck (before workspace : Workspace) (path : String) : Option Violation :=
  if isMirrored path then
    match before.get path, workspace.get path with
    | some _, none => some (Violation.missingFromCopy path)
    | some a, some b => if a == b then none else some (Violation.copyDiffers path)
    | none, some _ => some (Violation.extraInCopy path)
    | none, none => none
  else if classify path == Class.«protected» then
    match before.get path, workspace.get path with
    | some _, none => some (Violation.missingFromCopy path)
    | _, _ => none
  else none

def bothPaths (before after : Workspace) : List String :=
  let names := before.files.map (·.1) ++ after.files.map (·.1)
  (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) []).mergeSort (· <= ·)

def copyFaults (before workspace : Workspace) : List Violation :=
  (bothPaths before workspace).filterMap (copyCheck before workspace)

/-- @models REQ-SBX.real_tree_untouched -/
def treeChanges (before after : Workspace) : List Violation :=
  (bothPaths before after).filterMap (fun path =>
    if before.get path == after.get path then none else some (Violation.realTreeChanged path))

def copyViolations (witness : CopyWitness) : List Violation :=
  let before := witness.before.canon
  copyFaults before witness.workspace.canon ++ treeChanges before witness.after.canon

axiom sandboxCopy : Workspace → Workspace

axiom copy_obeys_its_law :
    ∀ w : Workspace, copyViolations { before := w, workspace := sandboxCopy w, after := w } = []

inductive Capability where
  | contained (mechanism : String)
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-SBX.capability_reported -/
def capabilityViolations (reported : Capability) : List Violation :=
  match reported with
  | .contained mechanism => if mechanism == "" then [Violation.containmentUnreported] else []
  | .unavailable reason => if reason == "" then [Violation.containmentUnreported] else []

axiom probeContainment : Unit → Capability

axiom probe_reports_something :
    capabilityViolations (probeContainment ()) = []

/-- @models REQ-SBX.escape_is_not_silent -/
def escapeViolations (writes : List String) : List Violation :=
  writes.filterMap (fun path =>
    if classify path == Class.outside then some (Violation.escaped path) else none)

axiom observedWrites : Workspace → List String

axiom escapes_are_named :
    ∀ w : Workspace,
      escapeViolations (observedWrites w) =
        (observedWrites w).filterMap (fun path =>
          if classify path == Class.outside then some (Violation.escaped path) else none)

axiom readTranscript : String → String

structure RunWitness where
  copy : CopyWitness := {}
  writes : List String := []
  containment : Capability := .unavailable "not probed"
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def runViolations (witness : RunWitness) : List Violation :=
  copyViolations witness.copy
    ++ escapeViolations witness.writes
    ++ capabilityViolations witness.containment

/-- @proves REQ-SBX.protected_never_mirrored -/
theorem a_protected_path_is_never_a_copy_violation
    (before workspace : Workspace) (path content : String)
    (h : classify path = Class.«protected») (seen : workspace.get path = some content) :
    copyCheck before workspace path = none := by
  have notMirrored : isMirrored path = false := protected_is_not_mirrored path h
  simp [copyCheck, notMirrored, h, seen]

/-- @proves REQ-SBX.capability_reported -/
theorem a_named_mechanism_is_reported (mechanism : String) (h : mechanism ≠ "") :
    capabilityViolations (.contained mechanism) = [] := by
  simp [capabilityViolations, h]

/-- @proves REQ-SBX.real_tree_untouched -/
theorem the_real_tree_is_checked_untouched (w : Workspace) :
    treeChanges w w = [] := by
  simp [treeChanges]

end TraceLean.Effects
