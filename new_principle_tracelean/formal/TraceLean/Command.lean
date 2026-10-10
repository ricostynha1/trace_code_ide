import Lean

/-!
# The command algebra

Models `REQ-CMD`. A workspace is a list of named files; a command is a
data-to-data function on it, so the round-trip law is a statement about
functions rather than about a filesystem.
-/

namespace TraceLean.Command

open Lean (Json ToJson FromJson)

/-- A workspace: file name to contents. -/
structure Workspace where
  files : List (String × String) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Workspace

/--
Collapse the list into a map: at most one entry per name, sorted by name.

A workspace *is* a map. The list is only its encoding, and a list admits states
a map cannot be in -- two entries for one path. Differential testing found
exactly that: a generated workspace with a duplicate name, where the model kept
both entries and the implementation, whose workspace is a map, kept one.

The model was wrong. A model that admits states the thing it models cannot
reach makes claims about situations that do not exist, and the fix belongs here
rather than in the generator: restricting the generator would have hidden the
gap instead of closing it.

Later entries win, which is what building a map from the list does.
-/
def canon (w : Workspace) : Workspace :=
  let deduped := w.files.foldl
    (fun acc (name, content) => (acc.filter (·.1 != name)) ++ [(name, content)]) []
  ⟨deduped.mergeSort (fun a b => a.1 ≤ b.1)⟩

def get (w : Workspace) (path : String) : Option String :=
  (w.files.find? (·.1 == path)).map (·.2)

def contains (w : Workspace) (path : String) : Bool :=
  (w.get path).isSome

/-- Insert or replace, keeping the list sorted by name so that equality of
workspaces does not depend on the order things were created in. -/
def set (w : Workspace) (path content : String) : Workspace :=
  let rest := w.files.filter (·.1 != path)
  ⟨(rest ++ [(path, content)]).mergeSort (fun a b => a.1 ≤ b.1)⟩

def remove (w : Workspace) (path : String) : Workspace :=
  ⟨w.files.filter (·.1 != path)⟩

end Workspace

/--
A command that destroys information carries what it destroyed, so its inverse
needs no other source.

@models REQ-CMD.witness_carried
-/
inductive Command where
  | insert (file : String) (offset : Nat) (text : String)
  | delete (file : String) (offset : Nat) (deleted : String)
  | createFile (path : String)
  | deleteFile (path : String) (content : String)
  | renameFile (from_ to : String)
  | batch (commands : List Command)
  deriving Repr, Inhabited, ToJson, FromJson

/-- Why a command does not fit the state it was applied to. -/
inductive Refusal where
  | noSuchFile (path : String)
  | fileExists (path : String)
  | offsetOutOfRange (path : String) (offset len : Nat)
  | witnessMismatch (path : String) (offset : Nat)
  | notACharBoundary (path : String) (offset : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The name of a refusal, without its detail.

Differential testing compares behaviour, and a refusal's *detail* is a
diagnostic: requiring the model to reproduce an offset or a path in a message
would couple it to wording rather than to what it did. The name is the
behaviour. -/
def Refusal.kind : Refusal → String
  | .noSuchFile _ => "noSuchFile"
  | .fileExists _ => "fileExists"
  | .offsetOutOfRange _ _ _ => "offsetOutOfRange"
  | .witnessMismatch _ _ => "witnessMismatch"
  | .notACharBoundary _ _ => "notACharBoundary"

/-- What a command did: a workspace, or the name of a refusal. -/
inductive Outcome where
  | ok (w : Workspace)
  | refused (kind : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Split a string at a character offset. Character offsets, not byte offsets:
the model does not have a byte representation, and the implementation refuses
any offset that is not a character boundary, so the two agree exactly where the
implementation accepts. -/
def splitAt (s : String) (n : Nat) : String × String :=
  let cs := s.toList
  (String.mk (cs.take n), String.mk (cs.drop n))

/--
Apply a command, or refuse it.

@models REQ-CMD.single_path
@models REQ-CMD.total_or_refused
-/
def apply (w : Workspace) (c : Command) : Except Refusal Workspace :=
  match c with
  | .insert file offset text =>
    match w.get file with
    | none => .error (.noSuchFile file)
    | some content =>
      if offset > content.length then
        .error (.offsetOutOfRange file offset content.length)
      else
        let (before, after) := splitAt content offset
        .ok (w.set file (before ++ text ++ after))
  | .delete file offset deleted =>
    match w.get file with
    | none => .error (.noSuchFile file)
    | some content =>
      if offset + deleted.length > content.length then
        .error (.offsetOutOfRange file (offset + deleted.length) content.length)
      else
        let (before, rest) := splitAt content offset
        let (removed, after) := splitAt rest deleted.length
        if removed != deleted then
          .error (.witnessMismatch file offset)
        else
          .ok (w.set file (before ++ after))
  | .createFile path =>
    if w.contains path then .error (.fileExists path)
    else .ok (w.set path "")
  | .deleteFile path content =>
    match w.get path with
    | none => .error (.noSuchFile path)
    | some actual =>
      if actual != content then .error (.witnessMismatch path 0)
      else .ok (w.remove path)
  | .renameFile from_ to =>
    match w.get from_ with
    | none => .error (.noSuchFile from_)
    | some content =>
      if w.contains to then .error (.fileExists to)
      else .ok ((w.remove from_).set to content)
  | .batch commands =>
    -- Applied to a copy, so a refusal partway leaves nothing behind.
    commands.attach.foldlM (fun acc c => apply acc c.val) w
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/--
The command that undoes this one.

The inverse of a batch is the reversed sequence of its members' inverses:
undoing in the order they were done would reapply the state each depended on.

@models REQ-CMD.inverse_exists
@models REQ-CMD.batch_reverses
-/
def inverse : Command → Command
  | .insert file offset text => .delete file offset text
  | .delete file offset deleted => .insert file offset deleted
  | .createFile path => .deleteFile path ""
  | .deleteFile path content =>
    .batch [.createFile path, .insert path 0 content]
  | .renameFile from_ to => .renameFile to from_
  -- `attach` carries the proof that each member is a subterm, which is what
  -- lets this be an ordinary structural recursion rather than a partial
  -- definition -- and a partial definition would not reduce, so the theorems
  -- below could not be stated about it.
  --
  -- The termination hint below is the one construct in this project written
  -- *against* the grammar rather than around it: Lean 4.12 cannot read this
  -- recursion structurally, and every alternative -- `mutual`, fuel, `partial`
  -- -- changes the definition rather than its spelling. So this file's
  -- annotations stay capped at L1, and ADR-0008 says why.
  | .batch commands => .batch ((commands.attach.map (fun c => inverse c.val)).reverse)
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/--
What a command is about, as a row of waiting changes names it: the file it
edits, makes or deletes, `from → to` for a rename, and a batch's parts joined
by commas. Recursion as `inverse` has it, for the same reason.
-/
def touched : Command → String
  | .insert file _ _ => file
  | .delete file _ _ => file
  | .createFile path => path
  | .deleteFile path _ => path
  | .renameFile from_ to => from_ ++ " → " ++ to
  | .batch commands => String.intercalate ", " (commands.attach.map (fun c => touched c.val))
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/--
Apply a command and then its inverse.

The law is one call, so both sides of a differential test answer the same
question.

@models REQ-CMD.round_trip
-/
def roundTrip (w : Workspace) (c : Command) : Except Refusal Workspace := do
  let after ← apply w c
  apply after (inverse c)

/-- `roundTrip`, in the shape the conformance protocol exchanges. -/
def roundTripOutcome (w : Workspace) (c : Command) : Outcome :=
  match roundTrip w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

/-- `apply`, in the same shape. -/
def applyOutcome (w : Workspace) (c : Command) : Outcome :=
  match apply w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

/-! ## Properties -/

/--
Inverting twice is the identity on every command that is not a file deletion.

`deleteFile` is the exception by construction: its inverse is a batch that
recreates and refills the file, and inverting *that* gives a batch rather than
the original single command. The two commands have the same effect, so the
round-trip law is unaffected, but they are not equal as syntax — and saying so
is better than a theorem quietly restricted to the cases that work.

@proves REQ-CMD.inverse_exists
-/
theorem inverse_inverse_insert (file : String) (offset : Nat) (text : String) :
    inverse (inverse (.insert file offset text)) = .insert file offset text := by
  simp [inverse]

theorem inverse_inverse_delete (file : String) (offset : Nat) (deleted : String) :
    inverse (inverse (.delete file offset deleted)) = .delete file offset deleted := by
  simp [inverse]

theorem inverse_inverse_rename (a b : String) :
    inverse (inverse (.renameFile a b)) = .renameFile a b := by
  simp [inverse]

/--
A batch's inverse runs its members in the opposite order.

@proves REQ-CMD.batch_reverses
-/
theorem batch_inverse_is_reversed (commands : List Command) :
    inverse (.batch commands) = .batch ((commands.map inverse).reverse) := by
  simp [inverse, List.attach_map_val]

/-- A workspace of two files, to state the laws below on. -/
def sample : Workspace := ⟨[("a.rs", "hello"), ("b.rs", "")]⟩

/-- Each kind of command, applied and then inverted, restores the workspace it
began from. The differential test generalises this to generated workspaces.

@proves REQ-CMD.round_trip -/
theorem each_command_round_trips :
    roundTripOutcome sample (.insert "a.rs" 2 "XY") = .ok sample ∧
    roundTripOutcome sample (.delete "a.rs" 1 "ell") = .ok sample ∧
    roundTripOutcome sample (.renameFile "a.rs" "c.rs") = .ok sample ∧
    roundTripOutcome sample (.createFile "d.rs") = .ok sample ∧
    roundTripOutcome sample (.deleteFile "a.rs" "hello") = .ok sample ∧
    roundTripOutcome sample (.batch [.insert "a.rs" 0 "J", .renameFile "b.rs" "e.rs"]) = .ok sample := by
  native_decide

/-- A deletion carries the text it deletes: a witness that does not match is
refused, and undoing a file deletion needs nothing but the command itself.

@proves REQ-CMD.witness_carried -/
theorem a_deletion_carries_what_it_deletes :
    applyOutcome sample (.delete "a.rs" 1 "xyz") = .refused "witnessMismatch" ∧
    applyOutcome sample (.deleteFile "a.rs" "other") = .refused "witnessMismatch" ∧
    applyOutcome ⟨[("b.rs", "")]⟩ (inverse (.deleteFile "a.rs" "hello")) = .ok sample := by
  native_decide

/-- A command that does not fit is refused with its reason, and a batch refused
partway applies none of its members.

@proves REQ-CMD.total_or_refused -/
theorem a_misfit_is_refused_whole :
    applyOutcome sample (.insert "zz.rs" 0 "x") = .refused "noSuchFile" ∧
    applyOutcome sample (.insert "a.rs" 9 "x") = .refused "offsetOutOfRange" ∧
    applyOutcome sample (.createFile "a.rs") = .refused "fileExists" ∧
    applyOutcome sample (.batch [.insert "a.rs" 0 "x", .renameFile "q.rs" "r.rs"])
      = .refused "noSuchFile" := by
  native_decide

/-- A batch is no second route to a state: it reaches what its members reach
applied one after the other.

@proves REQ-CMD.single_path -/
theorem a_batch_is_its_members_in_turn (w : Workspace) (a b : Command) :
    apply w (.batch [a, b]) = Except.bind (apply w a) (fun v => apply v b) := by
  rw [apply]
  simp [List.attach, List.foldlM]
  rfl

end TraceLean.Command
