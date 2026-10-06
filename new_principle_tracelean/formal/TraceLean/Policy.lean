import Lean

/-!
# Sandbox path policy

Models `REQ-SBX`. Which paths a sandboxed workspace may write, and which of
those come back. The whole security content of the feature is this function.
-/

namespace TraceLean.Policy

open Lean (ToJson FromJson)

/-- What may happen to a path. -/
inductive Class where
  /-- Writable inside the workspace, never replayed onto the real tree. -/
  | «protected»
  /-- Writable, and mirrored back. -/
  | mirrored
  /-- Writable and regenerable: build output and caches. -/
  | passThrough
  /-- Not part of the project at all. -/
  | outside
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def protectedRoots : List String := [".git", ".tracelean"]

def passThroughDirs : List String :=
  ["target", "node_modules", "dist", "build", ".venv", "venv", "__pycache__", ".lake"]

/-- Resolve `.` and `..` left to right, or fail when the path climbs out.

A path is what it reaches, not how it was spelled: `src/../.git/config` is the
version-control directory however it was written. -/
def resolveFrom (segments : List String) : Option (List String) :=
  segments.foldl
    (fun acc segment =>
      match acc with
      | none => none
      | some out =>
        if segment == "" || segment == "." then some out
        else if segment == ".." then
          match out.reverse with
          | [] => none
          | _ :: rest => some rest.reverse
        else some (out ++ [segment]))
    (some [])

/--
Classify a project-relative path.

Every path receives exactly one answer, including the paths that are trying not
to be answered: an absolute path, or one that climbs out of the project, is
`outside` rather than being normalised into something that looks local.

@models REQ-SBX.classification_total
@models REQ-SBX.protected_never_mirrored
@models REQ-SBX.passthrough_not_mirrored
@models REQ-SBX.escape_is_not_silent
-/
def classify (path : String) : Class :=
  if path == "" then .outside
  else if path.startsWith "/" then .outside
  else if path.startsWith (String.mk [Char.ofNat 92]) then .outside
  else if path.toList.contains (Char.ofNat 0) then .outside
  else if path.length >= 2 && path.toList.getD 1 ' ' == ':' then .outside
  else
    match resolveFrom (path.splitOn "/") with
    | none => .outside
    | some [] => .outside
    | some (first :: rest) =>
      if protectedRoots.contains first then .«protected»
      else if (first :: rest).any (fun s => passThroughDirs.contains s) then .passThrough
      else .mirrored

/-- Whether a change at this path is replayed onto the real tree.

@models REQ-MIRROR.protected_excluded -/
def isMirrored (path : String) : Bool :=
  classify path == .mirrored

/-- Nothing protected is ever mirrored: the two answers are exclusive by
construction, which is the property the sandbox depends on.

@proves REQ-SBX.protected_never_mirrored -/
theorem protected_is_not_mirrored (path : String) :
    classify path = Class.«protected» -> isMirrored path = false := by
  intro h
  simp [isMirrored, h]

end TraceLean.Policy
