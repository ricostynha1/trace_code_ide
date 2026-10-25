import TraceLean.Command
import TraceLean.Policy
import TraceLean.Hash
import Lean

namespace TraceLean.Mirror

open TraceLean.Command
open TraceLean.Policy

open Lean (ToJson FromJson)

def touchedPaths (before after : Workspace) : List String :=
  let names := (before.files.map (·.1) ++ after.files.map (·.1)).filter isMirrored
  (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) []).mergeSort (· <= ·)

private def mirrorStep (before after : Workspace) (path : String)
    : List Command × List Command × List Command :=
    match before.get path, after.get path with
    | some old, none => ([Command.deleteFile path old], [], [])
    | none, some new =>
      ([], Command.createFile path ::
        (if new == "" then [] else [Command.insert path 0 new]), [])
    | some old, some new =>
      if old == new then ([], [], [])
      else
        ([], [],
          (if old == "" then [] else [Command.delete path 0 old]) ++
          (if new == "" then [] else [Command.insert path 0 new]))
    | none, none => ([], [], [])

def mutations (before after : Workspace) : List Command :=
  let parts := (touchedPaths before after).map (mirrorStep before after)
  (parts.bind (·.1)) ++ (parts.bind (·.2.1)) ++ (parts.bind (·.2.2))

structure Mirroring where
  reached : Workspace
  refused : List Command
  deriving Repr, Inhabited, ToJson, FromJson

private def mirrorApply (acc : Mirroring) (c : Command) : Mirroring :=
  match apply acc.reached c with
  | .ok next => { acc with reached := next }
  | .error _ => { acc with refused := acc.refused ++ [c] }

/-- @models REQ-MIRROR.apply_reproduces -/
def mirrored (before after : Workspace) : Mirroring :=
  let b := before.canon
  let a := after.canon
  (mutations b a).foldl mirrorApply { reached := b, refused := [] }

/-- @models REQ-MIRROR.diff_is_pure
@models REQ-MIRROR.minimal
@models REQ-MIRROR.ordering_defined
@models REQ-MIRROR.protected_excluded -/
def mutationsOf (before after : Workspace) : List Command :=
  mutations before.canon after.canon

structure SelfWrite where
  path : String
  content : String
  age : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Suppression where
  suppressed : Bool
  pending : List SelfWrite
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def decrement (n : Nat) : Nat := n - 1

/-- @models REQ-SELFWRITE.own_writes_ignored
@models REQ-SELFWRITE.suppression_is_consumed
@models REQ-SELFWRITE.content_matched
@models REQ-SELFWRITE.unmatched_is_external -/
def suppress (pending : List SelfWrite) (path content : String) : Suppression :=
  let step := fun (acc : Bool × List SelfWrite) (write : SelfWrite) =>
    if !acc.1 && write.path == path && write.content == content then (true, acc.2)
    else (acc.1, acc.2 ++ [write])
  let result := pending.foldl step (false, [])
  { suppressed := result.1, pending := result.2 }

/-- @models REQ-SELFWRITE.no_deadlock -/
def expire (pending : List SelfWrite) : List SelfWrite :=
  (pending.map (fun w => { w with age := decrement w.age })).filter (·.age != 0)

/-- @proves REQ-SELFWRITE.unmatched_is_external -/
theorem nothing_pending_is_external (path content : String) :
    suppress [] path content = { suppressed := false, pending := [] } := by
  rfl

/-- @proves REQ-SELFWRITE.suppression_is_consumed -/
theorem one_write_suppresses_one_observation (path content : String) :
    (suppress [{ path := path, content := content, age := 3 }] path content).suppressed = true ∧
    (suppress (suppress [{ path := path, content := content, age := 3 }] path content).pending
      path content).suppressed = false := by
  simp [suppress]

inductive FileState where
  | absent
  | text (content : String)
  | opaque (hash : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Change where
  | mirror (command : Command)
  | reportOnly (path : String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- @models REQ-MIRROR.binary_handled -/
def changeAt (path : String) (before after : FileState) : List Change :=
  if !isMirrored path then []
  else
    match before, after with
    | .absent, .absent => []
    | .opaque old, .opaque new => if old == new then [] else [.reportOnly path]
    | .opaque _, _ => [.reportOnly path]
    | _, .opaque _ => [.reportOnly path]
    | .absent, .text content =>
      Change.mirror (Command.createFile path) ::
        (if content == "" then [] else [.mirror (Command.insert path 0 content)])
    | .text content, .absent => [.mirror (Command.deleteFile path content)]
    | .text old, .text new =>
      if old == new then []
      else
        (if old == "" then [] else [Change.mirror (Command.delete path 0 old)]) ++
        (if new == "" then [] else [Change.mirror (Command.insert path 0 new)])

def Change.isReportOnly : Change → Bool
  | .mirror _ => false
  | .reportOnly _ => true

/-- @proves REQ-MIRROR.binary_handled -/
theorem an_opaque_file_is_never_mirrored (path hash : String) (after : FileState) :
    (changeAt path (.opaque hash) after).all Change.isReportOnly = true := by
  unfold changeAt
  cases after
  all_goals split
  all_goals simp [Change.isReportOnly]
  all_goals split
  all_goals simp [Change.isReportOnly]

/-- @proves REQ-MIRROR.binary_handled -/
theorem a_changed_binary_is_reported :
    (changeAt "logo.png" (.opaque "1") (.opaque "2")).length = 1 ∧
    (changeAt "logo.png" (.opaque "1") (.opaque "1")).length = 0 := by
  native_decide

def commandPath : Command → String
  | .insert file _ _ => file
  | .delete file _ _ => file
  | .createFile path => path
  | .deleteFile path _ => path
  | .renameFile _ _ => ""
  | .batch _ => ""

def touchedByAgent (base : List (String × String)) (agent : Workspace) (path : String) : Bool :=
  (agent.get path).map TraceLean.Hash.digest != (base.reverse.find? (·.1 == path)).map (·.2)

/-- @models REQ-OBS.only_what_the_tool_changed -/
def changedSince (base : Option (List (String × String))) (project agent : Workspace) : List Command :=
  match base with
  | none => []
  | some b => (mutationsOf project agent).filter
      (fun c => commandPath c == "" || touchedByAgent b agent.canon (commandPath c))

/-- @proves REQ-OBS.only_what_the_tool_changed -/
theorem an_untouched_copy_offers_nothing :
    changedSince (some [("a.rs", TraceLean.Hash.digest "one")])
      ⟨[("a.rs", "one, since edited"), ("new.md", "x")]⟩ ⟨[("a.rs", "one")]⟩ |>.isEmpty := by
  native_decide

def beforeRun : Workspace :=
  ⟨[("gone.rs", "x"), ("same.rs", "s"), ("edit.rs", "old"), ("empty.rs", "e"), (".git/HEAD", "a")]⟩

def afterRun : Workspace :=
  ⟨[("same.rs", "s"), ("edit.rs", "new"), ("empty.rs", ""), ("made.rs", "m"), (".git/HEAD", "b")]⟩

def asText (commands : List Command) : List String :=
  commands.map (fun c => (Lean.toJson c).compress)

def applyOrKeep (w : Workspace) (c : Command) : Workspace :=
  match apply w c with
  | .ok next => next
  | .error _ => w

def mirroredPart (w : Workspace) : Workspace := ⟨w.canon.files.filter (fun f => isMirrored f.1)⟩

/-- @proves REQ-MIRROR.apply_reproduces -/
theorem mirroring_reaches_the_observed_tree :
    mirroredPart (mirrored beforeRun afterRun).reached = mirroredPart afterRun ∧
    (mirrored beforeRun afterRun).refused.isEmpty = true ∧
    (mirrored beforeRun afterRun).reached.get ".git/HEAD" = some "a" := by
  native_decide

/-- @proves REQ-MIRROR.diff_is_pure -/
theorem the_diff_is_of_the_trees_alone :
    asText (mutationsOf ⟨beforeRun.files.reverse⟩ ⟨afterRun.files.reverse⟩)
      = asText (mutationsOf beforeRun afterRun) := by
  native_decide

/-- @proves REQ-MIRROR.minimal -/
theorem no_mutation_is_spare :
    (List.range (mutationsOf beforeRun afterRun).length).all (fun i =>
      mirroredPart (((mutationsOf beforeRun afterRun).eraseIdx i).foldl applyOrKeep beforeRun.canon)
        != mirroredPart afterRun) = true := by
  native_decide

/-- @proves REQ-MIRROR.protected_excluded -/
theorem a_protected_path_is_not_mirrored :
    (mutationsOf beforeRun afterRun).all (fun c => isMirrored (commandPath c)) = true := by
  native_decide

/-- @proves REQ-MIRROR.ordering_defined -/
theorem mutations_run_in_an_order_that_fits :
    asText (mutationsOf beforeRun afterRun)
      = asText [.deleteFile "gone.rs" "x", .createFile "made.rs", .insert "made.rs" 0 "m",
                .delete "edit.rs" 0 "old", .insert "edit.rs" 0 "new", .delete "empty.rs" 0 "e"] ∧
    (mirrored beforeRun afterRun).refused.isEmpty = true := by
  native_decide

/-- @proves REQ-SELFWRITE.own_writes_ignored -/
theorem an_own_write_is_suppressed (path content : String) :
    (suppress [⟨path, content, 3⟩] path content).suppressed = true := by
  simp [suppress]

/-- @proves REQ-SELFWRITE.content_matched -/
theorem other_content_at_the_path_is_external (path written seen : String) (h : written ≠ seen) :
    suppress [⟨path, written, 3⟩] path seen = { suppressed := false, pending := [⟨path, written, 3⟩] } := by
  simp [suppress, h]

/-- @proves REQ-SELFWRITE.no_deadlock -/
theorem an_unobserved_write_expires (path content : String) :
    expire (expire [⟨path, content, 2⟩]) = [] ∧ expire [⟨path, content, 1⟩] = [] := by
  simp [expire, decrement]

end TraceLean.Mirror
