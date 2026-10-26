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

theorem protected_is_not_mirrored (path : String) :
    classify path = Class.«protected» -> isMirrored path = false := by
  intro h
  simp [isMirrored, h]

def Inside (path : String) : Prop :=
  path ≠ "" ∧ path.startsWith "/" = false ∧ path.startsWith (String.mk [Char.ofNat 92]) = false ∧
    path.toList.contains (Char.ofNat 0) = false ∧
    (path.length >= 2 && path.toList.getD 1 ' ' == ':') = false

/-- @proves REQ-SBX.protected_never_mirrored -/
theorem a_path_under_a_protected_root_is_protected (path first : String) (rest : List String)
    (inside : Inside path) (resolved : resolveFrom (path.splitOn "/") = some (first :: rest))
    (root : protectedRoots.contains first = true) :
    classify path = Class.«protected» := by
  obtain ⟨h1, h2, h3, h4, h5⟩ := inside
  unfold classify
  simp only [beq_iff_eq, h1, if_false, h2, h3, h4, h5, Bool.false_eq_true, resolved, root, if_true]

/-- @proves REQ-SBX.passthrough_not_mirrored -/
theorem a_regenerable_directory_is_passed_through (path first : String) (rest : List String)
    (inside : Inside path) (resolved : resolveFrom (path.splitOn "/") = some (first :: rest))
    (notProtected : protectedRoots.contains first = false)
    (regenerable : passThroughAtRoot.contains first = true ∨
      (first :: rest).any (fun s => passThroughAnywhere.contains s) = true) :
    classify path = .passThrough := by
  obtain ⟨h1, h2, h3, h4, h5⟩ := inside
  unfold classify
  simp only [beq_iff_eq, h1, if_false, h2, h3, h4, h5, Bool.false_eq_true, resolved, notProtected]
  rcases regenerable with h | h
  · have hm : first ∈ passThroughAtRoot := by simpa using h
    simp [hm]
  · by_cases hr : passThroughAtRoot.contains first = true
    · have hm : first ∈ passThroughAtRoot := by simpa using hr
      simp [hm]
    · simp only [hr, Bool.false_eq_true, if_false, h, if_true]

/-- @proves REQ-SBX.passthrough_not_mirrored -/
theorem a_source_directory_named_like_a_build_one_is_mirrored_below_the_root
    (path first : String) (rest : List String)
    (inside : Inside path) (resolved : resolveFrom (path.splitOn "/") = some (first :: rest))
    (notProtected : protectedRoots.contains first = false)
    (notAtRoot : passThroughAtRoot.contains first = false)
    (noneAnywhere : (first :: rest).any (fun s => passThroughAnywhere.contains s) = false) :
    classify path = .mirrored := by
  obtain ⟨h1, h2, h3, h4, h5⟩ := inside
  unfold classify
  simp only [beq_iff_eq, h1, if_false, h2, h3, h4, h5, Bool.false_eq_true, resolved, notProtected,
    notAtRoot, noneAnywhere]

end TraceLean.Policy
