import Lean

namespace TraceLean.Ask

/-- @models REQ-NAV.name_is_asked_for -/
def askStep (answer key : String) : String × String :=
  if key == "Escape" then ("cancelled", answer)
  else if key == "Enter" then
    if answer.trim.isEmpty then ("cancelled", answer) else ("done", answer.trim)
  else if key == "Backspace" then ("typing", String.mk answer.toList.dropLast)
  else if key == "Space" then ("typing", answer ++ " ")
  else if key.length == 1 then ("typing", answer ++ key)
  else ("typing", answer)

/-- @proves REQ-NAV.name_is_asked_for -/
theorem the_name_is_typed_then_confirmed_or_dropped (answer key : String) :
    askStep answer "Escape" = ("cancelled", answer) ∧
    (answer.trim.isEmpty = false → askStep answer "Enter" = ("done", answer.trim)) ∧
    (key.length = 1 → askStep answer key = ("typing", answer ++ key)) ∧
    ((askStep answer key).1 = "done" → key = "Enter" ∧ (askStep answer key).2 = answer.trim) := by
  refine ⟨rfl, ?_, ?_, ?_⟩
  · intro h
    simp [askStep, h]
  · intro h
    have e : key ≠ "Escape" := fun e => by rw [e] at h; simp [String.length] at h
    have n : key ≠ "Enter" := fun e => by rw [e] at h; simp [String.length] at h
    have b : key ≠ "Backspace" := fun e => by rw [e] at h; simp [String.length] at h
    have s : key ≠ "Space" := fun e => by rw [e] at h; simp [String.length] at h
    simp [askStep, e, n, b, s, h]
  · unfold askStep
    by_cases e : key = "Escape"
    · simp [e]
    · by_cases n : key = "Enter"
      · subst n
        by_cases t : answer.trim.isEmpty <;> simp [t]
      · simp only [e, n, beq_iff_eq, if_false]
        split <;> (try split) <;> (try split) <;> simp

end TraceLean.Ask
