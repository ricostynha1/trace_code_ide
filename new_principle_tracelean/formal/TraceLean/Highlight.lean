namespace TraceLean.Highlight

def charAt (cs : List Char) (i : Nat) : Char :=
  cs.getD i ' '

def runWhile (cs : List Char) (stop : Nat) (keep : Char → Bool) : Nat → Nat → Nat
  | 0, i => i
  | fuel + 1, i => if i < stop && keep (charAt cs i) then runWhile cs stop keep fuel (i + 1) else i

def upperOrDigit (c : Char) : Bool := c.isUpper || c.isDigit

def keyChar (c : Char) : Bool := c.isLower || c.isDigit || c == '_'

def groups (cs : List Char) (stop : Nat) : Nat → Nat × Nat → Nat × Nat
  | 0, state => state
  | fuel + 1, state =>
    if state.1 + 1 < stop && charAt cs state.1 == '-' && upperOrDigit (charAt cs (state.1 + 1)) then
      groups cs stop fuel (runWhile cs stop upperOrDigit (stop + 1) (state.1 + 1), state.2 + 1)
    else state

def referenceAt (cs : List Char) (start stop : Nat) : Option Nat :=
  let capitals := runWhile cs stop Char.isUpper (stop + 1) start
  if capitals - start < 2 then none
  else
    let grouped := groups cs stop (stop + 1) (capitals, 0)
    if grouped.2 == 0 then none
    else
      let past := grouped.1
      let ended :=
        if past + 1 < stop && charAt cs past == '.' && (charAt cs (past + 1)).isLower then
          runWhile cs stop keyChar (stop + 1) (past + 1)
        else past
      if ended < stop && (charAt cs ended).isAlphanum then none else some ended

def boundary (cs : List Char) (first place : Nat) : Bool :=
  place == first || !(charAt cs (place - 1)).isAlphanum

def namesFrom (cs : List Char) (first stop : Nat) : Nat → Nat → List (Nat × Nat)
  | 0, _ => []
  | fuel + 1, place =>
    if place < stop then
      match (if boundary cs first place then referenceAt cs place stop else none) with
      | some ended => (place, ended) :: namesFrom cs first stop fuel ended
      | none => namesFrom cs first stop fuel (place + 1)
    else []

/-- @models REQ-SHOW.references_are_links -/
def requirementNames (text : String) (start stop : Nat) : List (Nat × Nat) :=
  let cs := text.toList
  let last := min stop cs.length
  let first := min start last
  namesFrom cs first last (last + 1) first

theorem runWhile_ge (cs : List Char) (stop : Nat) (keep : Char → Bool) :
    ∀ (fuel i : Nat), i ≤ runWhile cs stop keep fuel i := by
  intro fuel
  induction fuel with
  | zero => intro i; exact Nat.le_refl _
  | succ fuel ih =>
    intro i
    unfold runWhile
    split
    · exact Nat.le_trans (Nat.le_succ i) (ih _)
    · exact Nat.le_refl _

theorem groups_ge (cs : List Char) (stop : Nat) :
    ∀ (fuel : Nat) (state : Nat × Nat), state.1 ≤ (groups cs stop fuel state).1 := by
  intro fuel
  induction fuel with
  | zero => intro state; exact Nat.le_refl _
  | succ fuel ih =>
    intro state
    unfold groups
    split
    · refine Nat.le_trans ?_ (ih _)
      exact Nat.le_trans (Nat.le_succ _) (runWhile_ge cs stop upperOrDigit _ _)
    · exact Nat.le_refl _

theorem referenceAt_ge {cs : List Char} {start stop e : Nat} (h : referenceAt cs start stop = some e) :
    start ≤ e := by
  unfold referenceAt at h
  simp only at h
  have h1 := runWhile_ge cs stop Char.isUpper (stop + 1) start
  have h2 := groups_ge cs stop (stop + 1) (runWhile cs stop Char.isUpper (stop + 1) start, 0)
  simp only at h2
  split at h
  · cases h
  · split at h
    · cases h
    · split at h
      · split at h
        · cases h
        · cases h
          exact Nat.le_trans h1 (Nat.le_trans h2
            (Nat.le_trans (Nat.le_succ _) (runWhile_ge cs stop keyChar _ _)))
      · split at h
        · cases h
        · cases h
          exact Nat.le_trans h1 h2

theorem namesFrom_sound (cs : List Char) (first stop : Nat) :
    ∀ (fuel place a b : Nat), (a, b) ∈ namesFrom cs first stop fuel place →
      place ≤ a ∧ a < stop ∧ boundary cs first a = true ∧ referenceAt cs a stop = some b := by
  intro fuel
  induction fuel with
  | zero => intro place a b h; simp [namesFrom] at h
  | succ fuel ih =>
    intro place a b h
    unfold namesFrom at h
    split at h
    · rename_i hp
      split at h
      · rename_i ended hr
        rcases List.mem_cons.mp h with h | h
        · simp only [Prod.mk.injEq] at h
          rw [h.1, h.2]
          by_cases hb : boundary cs first place = true
          · simp [hb] at hr
            exact ⟨Nat.le_refl _, hp, hb, hr⟩
          · simp [hb] at hr
        · have hge : place ≤ ended := by
            by_cases hb : boundary cs first place = true
            · simp [hb] at hr; exact referenceAt_ge hr
            · simp [hb] at hr
          obtain ⟨h1, h2, h3, h4⟩ := ih _ _ _ h
          exact ⟨Nat.le_trans hge h1, h2, h3, h4⟩
      · obtain ⟨h1, h2, h3, h4⟩ := ih _ _ _ h
        exact ⟨Nat.le_trans (Nat.le_succ _) h1, h2, h3, h4⟩
    · cases h

theorem namesFrom_complete (cs : List Char) (first stop p e : Nat) (hp : p < stop)
    (hb : boundary cs first p = true) (hr : referenceAt cs p stop = some e) :
    ∀ (fuel place : Nat), place ≤ p → p - place < fuel →
      (∀ q, place ≤ q → q < p → boundary cs first q = true → referenceAt cs q stop = none) →
      (p, e) ∈ namesFrom cs first stop fuel place := by
  intro fuel
  induction fuel with
  | zero => intro place _ hf _; omega
  | succ fuel ih =>
    intro place hle hf hnone
    unfold namesFrom
    have hps : place < stop := by omega
    simp only [hps, if_true]
    by_cases heq : place = p
    · subst heq
      simp [hb, hr]
    · have hnone' : (if boundary cs first place = true then referenceAt cs place stop else none)
          = none := by
        by_cases hbp : boundary cs first place = true
        · simp [hbp]; exact hnone place (Nat.le_refl _) (by omega) hbp
        · simp [hbp]
      simp only [hnone']
      exact ih (place + 1) (by omega) (by omega)
        (fun q h1 h2 h3 => hnone q (by omega) h2 h3)

/-- @proves REQ-SHOW.references_are_links -/
theorem requirement_names_are_exactly_the_references_marked (text : String) (start stop : Nat) :
    (∀ a b, (a, b) ∈ requirementNames text start stop →
      min start (min stop text.toList.length) ≤ a ∧ a < min stop text.toList.length ∧
      boundary text.toList (min start (min stop text.toList.length)) a = true ∧
      referenceAt text.toList a (min stop text.toList.length) = some b) ∧
    (∀ p e, min start (min stop text.toList.length) ≤ p → p < min stop text.toList.length →
      boundary text.toList (min start (min stop text.toList.length)) p = true →
      referenceAt text.toList p (min stop text.toList.length) = some e →
      (∀ q, min start (min stop text.toList.length) ≤ q → q < p →
        boundary text.toList (min start (min stop text.toList.length)) q = true →
        referenceAt text.toList q (min stop text.toList.length) = none) →
      (p, e) ∈ requirementNames text start stop) := by
  constructor
  · intro a b h
    exact namesFrom_sound _ _ _ _ _ a b h
  · intro p e h1 h2 h3 h4 h5
    exact namesFrom_complete _ _ _ p e h2 h3 h4 _ _ h1 (by omega) h5

end TraceLean.Highlight
