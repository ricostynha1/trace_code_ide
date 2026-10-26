import Lean

namespace TraceLean.Setting

def isHex (c : Char) : Bool :=
  c.isDigit || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')

/-- @models REQ-LOOK.setting_edited_in_place -/
def isColour (value : String) : Bool :=
  if value.startsWith "#" then
    let digits := value.toList.drop 1
    [3, 4, 6, 8].contains digits.length && digits.all isHex
  else false

/-- @proves REQ-LOOK.setting_edited_in_place -/
theorem colour_iff (value : String) :
    isColour value = true ↔
      value.startsWith "#" = true ∧ (value.toList.drop 1).length ∈ [3, 4, 6, 8] ∧
      ∀ c ∈ value.toList.drop 1, isHex c = true := by
  unfold isColour
  split
  · rename_i h
    simp [h, List.all_eq_true]
  · rename_i h
    simp [h]

/-- @proves REQ-LOOK.setting_edited_in_place -/
theorem non_colour_refused (value : String)
    (h : value.startsWith "#" = false ∨ ∃ c ∈ value.toList.drop 1, isHex c = false) :
    isColour value = false := by
  cases hc : isColour value
  · rfl
  · obtain ⟨hs, _, hall⟩ := (colour_iff value).mp hc
    rcases h with h | ⟨c, hm, hx⟩
    · rw [hs] at h; cases h
    · rw [hall c hm] at hx; cases hx

end TraceLean.Setting
