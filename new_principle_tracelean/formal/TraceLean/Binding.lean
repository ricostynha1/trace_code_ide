namespace TraceLean.Binding

/-- @models REQ-DRT-BIND.derived_unless_written -/
def overlaid (derived written : List (String × String)) : List (String × String) :=
  derived.filter (fun d => !written.any (·.1 == d.1)) ++ written

/-- @proves REQ-DRT-BIND.derived_unless_written -/
theorem a_written_field_replaces_its_derived_one (derived written : List (String × String)) (k v : String) :
    (k, v) ∈ overlaid derived written ↔ (k, v) ∈ written ∨ ((k, v) ∈ derived ∧ ∀ w ∈ written, w.1 ≠ k) := by
  simp only [overlaid, List.mem_append, List.mem_filter, Bool.not_eq_true', Bool.eq_false_iff, ne_eq,
    List.any_eq_true, beq_iff_eq, not_exists, not_and]
  constructor
  · rintro (⟨hd, hw⟩ | hw)
    · exact Or.inr ⟨hd, fun w hw' e => hw w hw' e⟩
    · exact Or.inl hw
  · rintro (hw | ⟨hd, hw⟩)
    · exact Or.inr hw
    · exact Or.inl ⟨hd, fun w hw' e => hw w hw' e⟩

end TraceLean.Binding
