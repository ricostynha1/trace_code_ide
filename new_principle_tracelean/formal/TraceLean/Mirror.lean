import TraceLean.Command
import TraceLean.Policy
import TraceLean.Hash
import Lean

/-!
# Mirroring a workspace

Models `REQ-MIRROR`. The diff is a function from two tree snapshots to a list of
mutations, and its law is that applying the output to the first yields the
second. A subsystem that looks inherently effectful is therefore an ordinary
differential test.
-/

namespace TraceLean.Mirror

open TraceLean.Command
open TraceLean.Policy

open Lean (ToJson FromJson)

/-- Every path either snapshot mentions, sorted and without repeats. -/
def touchedPaths (before after : Workspace) : List String :=
  let names := (before.files.map (·.1) ++ after.files.map (·.1)).filter isMirrored
  (names.foldl (fun acc n => if acc.contains n then acc else acc ++ [n]) []).mergeSort (· <= ·)

/-- What one path contributes to a mirroring, split by the order it must run in:
deletions, then creations, then content changes.

A definition rather than a typed lambda binder inside `mutations`, which the
grammar that reads these annotations cannot read (ADR-0008). -/
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

/--
The commands that carry `before` to `after`, for the paths that are mirrored.

Ordered so that applying them in sequence never depends on a state that does not
yet exist: deletions first, then creations, then content changes. -/
def mutations (before after : Workspace) : List Command :=
  let parts := (touchedPaths before after).map (mirrorStep before after)
  (parts.bind (·.1)) ++ (parts.bind (·.2.1)) ++ (parts.bind (·.2.2))

/-- What mirroring reached, and every derived command that was refused on the
way. A refusal means the mutations did not fit the state they were derived
from -- the law failing -- and is named rather than skipped. -/
structure Mirroring where
  reached : Workspace
  refused : List Command
  deriving Repr, Inhabited, ToJson, FromJson

/-- Apply one mirrored command; a refusal leaves the state as it was and is
recorded. -/
private def mirrorApply (acc : Mirroring) (c : Command) : Mirroring :=
  match apply acc.reached c with
  | .ok next => { acc with reached := next }
  | .error _ => { acc with refused := acc.refused ++ [c] }

/--
The state mirroring reaches, and what was refused.

The law in one call, so both sides of a differential test answer the same
question: `reached` is the observed tree on every mirrored path and the original
on every other, and `refused` is empty.

@models REQ-MIRROR.apply_reproduces
-/
def mirrored (before after : Workspace) : Mirroring :=
  let b := before.canon
  let a := after.canon
  -- The step is a definition rather than a lambda holding a `match` over
  -- several lines, which the grammar that reads these annotations cannot read
  -- (ADR-0008).
  (mutations b a).foldl mirrorApply { reached := b, refused := [] }

/-- `mutations`, over canonical snapshots.

A `Workspace` here is a list, so it admits a state a real tree cannot have: one
path listed twice. Canonicalising first is what `mirrored` already does, and the
comparison is against an implementation whose workspace is a map -- so this is
the same question asked of both, not a concession by either.

Applying them never depends on a state that does not yet exist: deletions
first, then creations, then content changes.

@models REQ-MIRROR.diff_is_pure
@models REQ-MIRROR.minimal
@models REQ-MIRROR.ordering_defined
@models REQ-MIRROR.protected_excluded -/
def mutationsOf (before after : Workspace) : List Command :=
  mutations before.canon after.canon

/-!
## Self-writes

`REQ-SELFWRITE`. The editor writes to the same tree it observes, so its own
writes come back as observations. Ignoring them by path would swallow a genuine
external change to a file the editor happened to touch; ignoring them forever
would mean a write that is never observed suppresses the next real change for
good. The model is therefore a small consumable: matched on content, spent on
use, and aged out.
-/

/-- A write this system performed, to be ignored when it is observed coming
back.

One write, one suppression: a matching observation consumes it whole. How long
it may wait for that observation is a separate count, `age`, spent by `expire`
and never by a match -- one field serving as both let a write with rounds to
spare suppress as many observations as it had rounds. -/
structure SelfWrite where
  path : String
  content : String
  /-- Rounds this may still wait to be observed before it expires. -/
  age : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether an observation was this system's own write, and what is left
pending. -/
structure Suppression where
  suppressed : Bool
  pending : List SelfWrite
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One less, never below zero. -/
def decrement (n : Nat) : Nat := n - 1

/--
Whether an observation is this system's own write coming back, and the pending
set with that suppression consumed.

One pending write is spent per observation, the first match in order, and it is
removed whatever its age: two identical pending writes suppress two
observations, one suppresses one.

@models REQ-SELFWRITE.own_writes_ignored
@models REQ-SELFWRITE.suppression_is_consumed
@models REQ-SELFWRITE.content_matched
@models REQ-SELFWRITE.unmatched_is_external
-/
def suppress (pending : List SelfWrite) (path content : String) : Suppression :=
  let step := fun (acc : Bool × List SelfWrite) (write : SelfWrite) =>
    if !acc.1 && write.path == path && write.content == content then (true, acc.2)
    else (acc.1, acc.2 ++ [write])
  let result := pending.foldl step (false, [])
  { suppressed := result.1, pending := result.2 }

/-- Age every pending suppression by one round, dropping those that expired.

@models REQ-SELFWRITE.no_deadlock -/
def expire (pending : List SelfWrite) : List SelfWrite :=
  (pending.map (fun w => { w with age := decrement w.age })).filter (·.age != 0)

/-- An observation matching nothing pending is external, and leaves the pending
set exactly as it was.

@proves REQ-SELFWRITE.unmatched_is_external -/
theorem nothing_pending_is_external (path content : String) :
    suppress [] path content = { suppressed := false, pending := [] } := by
  rfl

/-- A write with rounds to spare suppresses the first observation of it and not
the second: the case the old single counter got wrong.

@proves REQ-SELFWRITE.suppression_is_consumed -/
theorem one_write_suppresses_one_observation (path content : String) :
    (suppress [{ path := path, content := content, age := 3 }] path content).suppressed = true ∧
    (suppress (suppress [{ path := path, content := content, age := 3 }] path content).pending
      path content).suppressed = false := by
  simp [suppress]

/-! ## Files the editor cannot represent

`binary_handled`. Three states, not two. A file that exists and is not text — a
binary, a file with invalid UTF-8, anything the editor has no representation for
— is neither absent nor content, and collapsing it into either is how a mirror
either loses a change or writes rubbish over one.

A change involving such a file is *reported* — it happened, and the user needs
to know — but never turned into commands. The alternative, silently skipping
it, is a change the user is told nothing about, which is worse than a change
they cannot review in place.
-/

/-- What a snapshot saw at one path. -/
inductive FileState where
  | absent
  | text (content : String)
  /-- Present, and not representable as text: known by a hash of its bytes, so
  that a changed binary can be told from an unchanged one. -/
  | opaque (hash : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What the mirror does about one path. -/
inductive Change where
  /-- A command the editor can apply. -/
  | mirror (command : Command)
  /-- Something changed that cannot be carried across as text. -/
  | reportOnly (path : String)
  -- No `DecidableEq`: `Command` is recursive and does not derive one, and this
  -- type is only ever compared through its JSON encoding.
  deriving Repr, Inhabited, ToJson, FromJson

/--
What the mirror does about one path, given what the two snapshots saw.

@models REQ-MIRROR.binary_handled
-/
def changeAt (path : String) (before after : FileState) : List Change :=
  if !isMirrored path then []
  else
    match before, after with
    | .absent, .absent => []
    -- Two opaque snapshots are told apart by their hashes: the same bytes are
    -- no change, different bytes are one, reported.
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

/-- Whether a change only reports, rather than writing anything. -/
def Change.isReportOnly : Change → Bool
  | .mirror _ => false
  | .reportOnly _ => true

/-- A file the editor cannot represent is never mirrored, whatever happened to
it. Reported, but never turned into a command that would write its bytes into a
text buffer and corrupt it on the way back.

@proves REQ-MIRROR.binary_handled -/
theorem an_opaque_file_is_never_mirrored (path hash : String) (after : FileState) :
    (changeAt path (.opaque hash) after).all Change.isReportOnly = true := by
  unfold changeAt
  cases after
  all_goals split
  all_goals simp [Change.isReportOnly]
  all_goals split
  all_goals simp [Change.isReportOnly]

/-- A binary whose bytes changed between snapshots is reported. Before the hash
it was not: two opaque snapshots were the same snapshot.

@proves REQ-MIRROR.binary_handled -/
theorem a_changed_binary_is_reported :
    (changeAt "logo.png" (.opaque "1") (.opaque "2")).length = 1 ∧
    (changeAt "logo.png" (.opaque "1") (.opaque "1")).length = 0 := by
  native_decide

/-! ## What the agent changed

`REQ-OBS.only_what_the_tool_changed`. The copy is not the project: a project
edited after its copy was made differs from the copy wherever it was edited,
and `mutations` alone would offer those edits, reversed, as the agent's work.
-/

/-- The file a command acts on; none for a rename or a batch, which
`mutations` never makes. -/
def commandPath : Command → String
  | .insert file _ _ => file
  | .delete file _ _ => file
  | .createFile path => path
  | .deleteFile path _ => path
  | .renameFile _ _ => ""
  | .batch _ => ""

/-- Whether the agent changed `path`: its content in the copy, as a hash or
absent, is not what the copy started with. A path the start lists twice is
read at its last entry, as a map built from the list would. -/
def touchedByAgent (base : List (String × String)) (agent : Workspace) (path : String) : Bool :=
  (agent.get path).map TraceLean.Hash.digest != (base.reverse.find? (·.1 == path)).map (·.2)

/-- What carrying the project to the copy changes, kept to the paths the agent
changed since the copy started (`base`, each path's hash); nothing when the
start is unknown.

@models REQ-OBS.only_what_the_tool_changed -/
def changedSince (base : Option (List (String × String))) (project agent : Workspace) : List Command :=
  match base with
  | none => []
  | some b => (mutationsOf project agent).filter
      (fun c => commandPath c == "" || touchedByAgent b agent.canon (commandPath c))

/-- A copy the agent never touched offers nothing, however far the project
moved on since it was made.

@proves REQ-OBS.only_what_the_tool_changed -/
theorem an_untouched_copy_offers_nothing :
    changedSince (some [("a.rs", TraceLean.Hash.digest "one")])
      ⟨[("a.rs", "one, since edited"), ("new.md", "x")]⟩ ⟨[("a.rs", "one")]⟩ |>.isEmpty := by
  native_decide

/-! ## What holds of mirroring -/

/-- A tree before an agent ran, and after: a file deleted, one created, one
changed, one emptied, one untouched, and a protected file written. -/
def beforeRun : Workspace :=
  ⟨[("gone.rs", "x"), ("same.rs", "s"), ("edit.rs", "old"), ("empty.rs", "e"), (".git/HEAD", "a")]⟩

def afterRun : Workspace :=
  ⟨[("same.rs", "s"), ("edit.rs", "new"), ("empty.rs", ""), ("made.rs", "m"), (".git/HEAD", "b")]⟩

/-- Commands as text, to compare: they have no decidable equality. -/
def asText (commands : List Command) : List String :=
  commands.map (fun c => (Lean.toJson c).compress)

/-- One command applied, a refusal leaving the state as it was. -/
def applyOrKeep (w : Workspace) (c : Command) : Workspace :=
  match apply w c with
  | .ok next => next
  | .error _ => w

/-- The mirrored part of a tree: what mirroring is asked to reproduce. -/
def mirroredPart (w : Workspace) : Workspace := ⟨w.canon.files.filter (fun f => isMirrored f.1)⟩

/-- Applying the derived mutations to the original reproduces the observed tree
on every mirrored path, with nothing refused, and leaves every other path as it
was.

@proves REQ-MIRROR.apply_reproduces -/
theorem mirroring_reaches_the_observed_tree :
    mirroredPart (mirrored beforeRun afterRun).reached = mirroredPart afterRun ∧
    (mirrored beforeRun afterRun).refused.isEmpty = true ∧
    (mirrored beforeRun afterRun).reached.get ".git/HEAD" = some "a" := by
  native_decide

/-- The mutations are a function of the two trees alone: the same trees listed
in another order give the same mutations.

@proves REQ-MIRROR.diff_is_pure -/
theorem the_diff_is_of_the_trees_alone :
    asText (mutationsOf ⟨beforeRun.files.reverse⟩ ⟨afterRun.files.reverse⟩)
      = asText (mutationsOf beforeRun afterRun) := by
  native_decide

/-- Leaving out any one mutation no longer reproduces the observed tree.

@proves REQ-MIRROR.minimal -/
theorem no_mutation_is_spare :
    (List.range (mutationsOf beforeRun afterRun).length).all (fun i =>
      mirroredPart (((mutationsOf beforeRun afterRun).eraseIdx i).foldl applyOrKeep beforeRun.canon)
        != mirroredPart afterRun) = true := by
  native_decide

/-- Nothing under a protected path is among the mutations, though it changed.

@proves REQ-MIRROR.protected_excluded -/
theorem a_protected_path_is_not_mirrored :
    (mutationsOf beforeRun afterRun).all (fun c => isMirrored (commandPath c)) = true := by
  native_decide

/-- Deletions, then creations, then content changes: no command waits on a file
a later one makes, and none is refused for it.

@proves REQ-MIRROR.ordering_defined -/
theorem mutations_run_in_an_order_that_fits :
    asText (mutationsOf beforeRun afterRun)
      = asText [.deleteFile "gone.rs" "x", .createFile "made.rs", .insert "made.rs" 0 "m",
                .delete "edit.rs" 0 "old", .insert "edit.rs" 0 "new", .delete "empty.rs" 0 "e"] ∧
    (mirrored beforeRun afterRun).refused.isEmpty = true := by
  native_decide

/-- The system's own write, observed coming back, is not taken in as a change.

@proves REQ-SELFWRITE.own_writes_ignored -/
theorem an_own_write_is_suppressed (path content : String) :
    (suppress [⟨path, content, 3⟩] path content).suppressed = true := by
  simp [suppress]

/-- A different content at the same path is not suppressed: matching is on what
was written, not on where.

@proves REQ-SELFWRITE.content_matched -/
theorem other_content_at_the_path_is_external (path written seen : String) (h : written ≠ seen) :
    suppress [⟨path, written, 3⟩] path seen = { suppressed := false, pending := [⟨path, written, 3⟩] } := by
  simp [suppress, h]

/-- A write never observed runs out of rounds and is dropped, so it cannot hold
back a later change for ever.

@proves REQ-SELFWRITE.no_deadlock -/
theorem an_unobserved_write_expires (path content : String) :
    expire (expire [⟨path, content, 2⟩]) = [] ∧ expire [⟨path, content, 1⟩] = [] := by
  simp [expire, decrement]

end TraceLean.Mirror
