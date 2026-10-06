import TraceLean.Command
import TraceLean.Policy
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
yet exist: deletions first, then creations, then content changes.

@models REQ-MIRROR.diff_is_pure
@models REQ-MIRROR.minimal
@models REQ-MIRROR.ordering_defined
-/
def mutations (before after : Workspace) : List Command :=
  let parts := (touchedPaths before after).map (mirrorStep before after)
  (parts.bind (·.1)) ++ (parts.bind (·.2.1)) ++ (parts.bind (·.2.2))

/-- Apply one mirrored command, keeping the state a refusal left untouched. -/
private def mirrorApply (acc : Workspace) (c : Command) : Workspace :=
  match apply acc c with
  | .ok next => next
  | .error _ => acc

/--
The state mirroring reaches.

The law in one call, so both sides of a differential test answer the same
question.

@models REQ-MIRROR.apply_reproduces
-/
def mirrored (before after : Workspace) : Workspace :=
  let b := before.canon
  let a := after.canon
  -- The step is a definition rather than a lambda holding a `match` over
  -- several lines, which the grammar that reads these annotations cannot read
  -- (ADR-0008).
  (mutations b a).foldl mirrorApply b

/-- `mutations`, over canonical snapshots.

A `Workspace` here is a list, so it admits a state a real tree cannot have: one
path listed twice. Canonicalising first is what `mirrored` already does, and the
comparison is against an implementation whose workspace is a map -- so this is
the same question asked of both, not a concession by either.

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
back. -/
structure SelfWrite where
  path : String
  content : String
  /-- Observations this may still suppress. -/
  remaining : Nat
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

At most one suppression is spent per observation, and it is the first match in
order: two identical pending writes suppress two observations, not one.

@models REQ-SELFWRITE.own_writes_ignored
@models REQ-SELFWRITE.suppression_is_consumed
@models REQ-SELFWRITE.content_matched
@models REQ-SELFWRITE.unmatched_is_external
-/
def suppress (pending : List SelfWrite) (path content : String) : Suppression :=
  let step := fun (acc : Bool × List SelfWrite) (write : SelfWrite) =>
    if !acc.1 && write.path == path && write.content == content then
      let left := decrement write.remaining
      (true, if left == 0 then acc.2 else acc.2 ++ [{ write with remaining := left }])
    else
      (acc.1, acc.2 ++ [write])
  let result := pending.foldl step (false, [])
  { suppressed := result.1, pending := result.2 }

/-- Age every pending suppression by one round, dropping those that expired.

@models REQ-SELFWRITE.no_deadlock -/
def expire (pending : List SelfWrite) : List SelfWrite :=
  (pending.map (fun w => { w with remaining := decrement w.remaining })).filter (·.remaining != 0)

/-- An observation matching nothing pending is external, and leaves the pending
set exactly as it was.

@proves REQ-SELFWRITE.unmatched_is_external -/
theorem nothing_pending_is_external (path content : String) :
    suppress [] path content = { suppressed := false, pending := [] } := by
  rfl

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
  /-- Present, and not representable as text. -/
  | opaque
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
@models REQ-MIRROR.protected_excluded
-/
def changeAt (path : String) (before after : FileState) : List Change :=
  if !isMirrored path then []
  else
    match before, after with
    | .absent, .absent => []
    -- Two opaque snapshots are indistinguishable to this layer, so the honest
    -- answer is that nothing is known to have changed.
    | .opaque, .opaque => []
    | .opaque, _ => [.reportOnly path]
    | _, .opaque => [.reportOnly path]
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
theorem an_opaque_file_is_never_mirrored (path : String) (after : FileState) :
    (changeAt path .opaque after).all Change.isReportOnly = true := by
  unfold changeAt
  cases after <;> split <;> simp [Change.isReportOnly]

end TraceLean.Mirror
