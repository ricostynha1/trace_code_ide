import Lean

namespace TraceLean.Command

open Lean (Json ToJson FromJson)

structure Workspace where
  files : List (String × String) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

namespace Workspace

def canon (w : Workspace) : Workspace :=
  let deduped := w.files.foldl
    (fun acc (name, content) => (acc.filter (·.1 != name)) ++ [(name, content)]) []
  ⟨deduped.mergeSort (fun a b => a.1 ≤ b.1)⟩

def get (w : Workspace) (path : String) : Option String :=
  (w.files.find? (·.1 == path)).map (·.2)

def contains (w : Workspace) (path : String) : Bool :=
  (w.get path).isSome

def set (w : Workspace) (path content : String) : Workspace :=
  let rest := w.files.filter (·.1 != path)
  ⟨(rest ++ [(path, content)]).mergeSort (fun a b => a.1 ≤ b.1)⟩

def remove (w : Workspace) (path : String) : Workspace :=
  ⟨w.files.filter (·.1 != path)⟩

end Workspace

/-- @models REQ-CMD.witness_carried -/
inductive Command where
  | insert (file : String) (offset : Nat) (text : String)
  | delete (file : String) (offset : Nat) (deleted : String)
  | createFile (path : String)
  | deleteFile (path : String) (content : String)
  | renameFile (from_ to : String)
  | batch (commands : List Command)
  deriving Repr, Inhabited, ToJson, FromJson

inductive Refusal where
  | noSuchFile (path : String)
  | fileExists (path : String)
  | offsetOutOfRange (path : String) (offset len : Nat)
  | witnessMismatch (path : String) (offset : Nat)
  | notACharBoundary (path : String) (offset : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Refusal.kind : Refusal → String
  | .noSuchFile _ => "noSuchFile"
  | .fileExists _ => "fileExists"
  | .offsetOutOfRange _ _ _ => "offsetOutOfRange"
  | .witnessMismatch _ _ => "witnessMismatch"
  | .notACharBoundary _ _ => "notACharBoundary"

inductive Outcome where
  | ok (w : Workspace)
  | refused (kind : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def splitAt (s : String) (n : Nat) : String × String :=
  let cs := s.toList
  (String.mk (cs.take n), String.mk (cs.drop n))

/-- @models REQ-CMD.single_path
@models REQ-CMD.total_or_refused -/
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
    commands.attach.foldlM (fun acc c => apply acc c.val) w
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

/-- @models REQ-CMD.inverse_exists
@models REQ-CMD.batch_reverses -/
def inverse : Command → Command
  | .insert file offset text => .delete file offset text
  | .delete file offset deleted => .insert file offset deleted
  | .createFile path => .deleteFile path ""
  | .deleteFile path content =>
    .batch [.createFile path, .insert path 0 content]
  | .renameFile from_ to => .renameFile to from_
  | .batch commands => .batch ((commands.attach.map (fun c => inverse c.val)).reverse)
decreasing_by
  simp_wf
  exact Nat.lt_trans (List.sizeOf_lt_of_mem c.property) (by omega)

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

/-- @models REQ-CMD.round_trip -/
def roundTrip (w : Workspace) (c : Command) : Except Refusal Workspace := do
  let after ← apply w c
  apply after (inverse c)

def roundTripOutcome (w : Workspace) (c : Command) : Outcome :=
  match roundTrip w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

def applyOutcome (w : Workspace) (c : Command) : Outcome :=
  match apply w.canon c with
  | .ok result => .ok result
  | .error refusal => .refused refusal.kind

/-- @proves REQ-CMD.inverse_exists -/
theorem inverse_inverse_insert (file : String) (offset : Nat) (text : String) :
    inverse (inverse (.insert file offset text)) = .insert file offset text := by
  simp [inverse]

theorem inverse_inverse_delete (file : String) (offset : Nat) (deleted : String) :
    inverse (inverse (.delete file offset deleted)) = .delete file offset deleted := by
  simp [inverse]

theorem inverse_inverse_rename (a b : String) :
    inverse (inverse (.renameFile a b)) = .renameFile a b := by
  simp [inverse]

/-- @proves REQ-CMD.batch_reverses -/
theorem batch_inverse_is_reversed (commands : List Command) :
    inverse (.batch commands) = .batch ((commands.map inverse).reverse) := by
  simp [inverse, List.attach_map_val]

def sample : Workspace := ⟨[("a.rs", "hello"), ("b.rs", "")]⟩

/-- @proves REQ-CMD.single_path -/
theorem a_batch_is_its_members_in_turn (w : Workspace) (a b : Command) :
    apply w (.batch [a, b]) = Except.bind (apply w a) (fun v => apply v b) := by
  rw [apply]
  simp [List.attach, List.foldlM]
  rfl

end TraceLean.Command
