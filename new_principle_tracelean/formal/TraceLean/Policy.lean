import Lean

namespace TraceLean.Policy

open Lean (ToJson FromJson)

inductive Class where
  | «protected»
  | mirrored
  | passThrough
  | outside
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def protectedRoots : List String := [".git", ".tracelean"]

def passThroughAnywhere : List String :=
  ["target", "node_modules", "__pycache__", ".lake", ".venv",
   ".cache", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".gradle", ".tox"]

def passThroughAtRoot : List String := ["build", "dist", "venv"]

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

/-- @models REQ-SBX.classification_total
@models REQ-SBX.protected_never_mirrored
@models REQ-SBX.passthrough_not_mirrored -/
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
      else if passThroughAtRoot.contains first then .passThrough
      else if (first :: rest).any (fun s => passThroughAnywhere.contains s) then .passThrough
      else .mirrored

def isMirrored (path : String) : Bool :=
  classify path == .mirrored

/-- @proves REQ-SBX.protected_never_mirrored -/
theorem protected_is_not_mirrored (path : String) :
    classify path = Class.«protected» -> isMirrored path = false := by
  intro h
  simp [isMirrored, h]

/-- @proves REQ-SBX.classification_total -/
theorem every_path_has_one_of_four_classes (path : String) :
    classify path = Class.«protected» ∨ classify path = Class.mirrored ∨
      classify path = Class.passThrough ∨ classify path = Class.outside := by
  cases classify path
  all_goals simp

/-- @proves REQ-SBX.passthrough_not_mirrored -/
theorem regenerable_is_not_mirrored (path : String) :
    classify path = .passThrough → isMirrored path = false := by
  intro h
  simp [isMirrored, h]

end TraceLean.Policy
