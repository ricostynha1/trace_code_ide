import Lean

namespace TraceLean.Explorer

open Lean (ToJson FromJson)

structure Row where
  depth : Nat
  name : String
  path : String
  folder : Bool
  isOpen : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def insertSorted (s : String) : List String → List String
  | [] => [s]
  | x :: rest => if s == x then x :: rest else if s < x then s :: x :: rest else x :: insertSorted s rest

def split (files : List String) (pre : String) : List String × List String :=
  files.foldl (fun (acc : List String × List String) (file : String) =>
    if file.startsWith pre then
      match (file.drop pre.length).splitOn "/" with
      | folder :: _ :: _ => (insertSorted folder acc.1, acc.2)
      | _ => (acc.1, insertSorted (file.drop pre.length) acc.2)
    else acc) ([], [])

def level (files opened : List String) : Nat → String → Nat → List Row
  | 0, _, _ => []
  | fuel + 1, pre, depth =>
    let parts := split files pre
    let folders := parts.1.bind fun folder =>
      let path := pre ++ folder
      let isOpen := opened.contains path
      { depth := depth, name := folder, path := path, folder := true, isOpen := isOpen } ::
        (if isOpen then level files opened fuel (path ++ "/") (depth + 1) else [])
    let leaves := parts.2.map fun leaf =>
      ({ depth := depth, name := leaf, path := pre ++ leaf, folder := false, isOpen := false } : Row)
    folders ++ leaves

/-- @models REQ-SHOW.listing_is_a_tree -/
def rows (files opened : List String) : List Row :=
  level files opened (files.foldl (fun m f => max m f.length) 0 + 2) "" 0

def Placed (opened : List String) (all : List Row) (pre : String) (depth : Nat) (r : Row) : Prop :=
  depth ≤ r.depth ∧ (r.depth = depth → r.path = pre ++ r.name) ∧
    (r.folder = true → r.isOpen = opened.contains r.path) ∧
    (depth < r.depth → ∃ f ∈ all, f.folder = true ∧ f.isOpen = true ∧ f.depth + 1 = r.depth ∧
      r.path = f.path ++ "/" ++ r.name)

theorem level_placed (files opened : List String) (fuel : Nat) :
    ∀ pre depth, ∀ r ∈ level files opened fuel pre depth,
      Placed opened (level files opened fuel pre depth) pre depth r := by
  induction fuel with
  | zero => intro pre depth r h; simp [level] at h
  | succ fuel ih =>
    intro pre depth r h
    have sub : ∀ folder ∈ (split files pre).1, opened.contains (pre ++ folder) = true →
        ∀ x ∈ level files opened fuel (pre ++ folder ++ "/") (depth + 1),
          x ∈ level files opened (fuel + 1) pre depth := by
      intro folder hf ho x hx
      simp only [level, List.mem_append, List.mem_bind]
      have ho' : pre ++ folder ∈ opened := List.mem_of_elem_eq_true ho
      exact Or.inl ⟨folder, hf, List.mem_cons_of_mem _ (by simp [ho', hx])⟩
    have frow : ∀ folder ∈ (split files pre).1,
        ({ depth := depth, name := folder, path := pre ++ folder, folder := true,
           isOpen := opened.contains (pre ++ folder) } : Row) ∈ level files opened (fuel + 1) pre depth := by
      intro folder hf
      simp only [level, List.mem_append, List.mem_bind]
      exact Or.inl ⟨folder, hf, List.mem_cons_self _ _⟩
    have h' := h
    simp only [level, List.mem_append, List.mem_bind, List.mem_map] at h'
    rcases h' with ⟨folder, hf, hr⟩ | ⟨leaf, _, rfl⟩
    · rcases hr with _ | ⟨_, hr⟩
      · refine ⟨Nat.le_refl _, fun _ => rfl, fun _ => rfl, fun lt => absurd lt (Nat.lt_irrefl _)⟩
      · split at hr
        · rename_i ho
          obtain ⟨le, top, isOpen, deeper⟩ := ih (pre ++ folder ++ "/") (depth + 1) r hr
          refine ⟨by omega, fun e => by omega, isOpen, fun _ => ?_⟩
          by_cases d : r.depth = depth + 1
          · exact ⟨_, frow folder hf, rfl, ho, d.symm, by rw [top d]⟩
          · obtain ⟨f, hfm, ff, fo, fd, fp⟩ := deeper (by omega)
            exact ⟨f, sub folder hf ho f hfm, ff, fo, fd, fp⟩
        · exact absurd hr (List.not_mem_nil _)
    · exact ⟨Nat.le_refl _, fun _ => rfl, fun e => by simp at e,
        fun lt => absurd lt (Nat.lt_irrefl _)⟩

/-- @proves REQ-SHOW.listing_is_a_tree -/
theorem the_listing_is_a_tree_of_whole_paths (files opened : List String) :
    ∀ r ∈ rows files opened,
      (r.depth = 0 → r.path = r.name) ∧
      (r.folder = true → r.isOpen = opened.contains r.path) ∧
      (0 < r.depth → ∃ f ∈ rows files opened, f.folder = true ∧ f.isOpen = true ∧
        f.depth + 1 = r.depth ∧ r.path = f.path ++ "/" ++ r.name) := by
  intro r h
  obtain ⟨_, top, isOpen, deeper⟩ := level_placed files opened _ "" 0 r h
  refine ⟨fun e => ?_, isOpen, deeper⟩
  rw [top e]
  cases r.name
  rfl

end TraceLean.Explorer
