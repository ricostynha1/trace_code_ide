import TraceLean.Annotation

namespace TraceLean.Chips

open Lean (ToJson FromJson)
open TraceLean.Annotation

structure Chip where
  line : Nat
  letter : String
  requirement : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def letter : Role → Option String
  | .models => some "M"
  | .specifies => some "M"
  | .implements => some "I"
  | .tests => some "T"
  | .drt => some "D"
  | .proves => some "P"
  | .pins => none

def textLines (text : String) : List String :=
  let pieces := text.splitOn "\n"
  let pieces := if pieces.getLast? == some "" then pieces.dropLast else pieces
  pieces.map fun line => if line.endsWith "\r" then line.dropRight 1 else line

def clauseKey (row : String) : Option String :=
  if row.startsWith "  " then
    let rest := row.drop 2
    if rest.startsWith " " || rest.startsWith "\t" then none
    else
      match rest.splitOn ":" with
      | key :: _ :: _ =>
        let key := key.trim
        if key.isEmpty || key.contains ' ' then none else some key
      | _ => none
  else none

def clausesFrom : List String → Nat → Nat → List (Nat × String)
  | [], _, _ => []
  | row :: rest, line, fences =>
    if row.trim == "---" then
      if fences + 1 == 2 then [] else clausesFrom rest (line + 1) (fences + 1)
    else
      match clauseKey row with
      | some key => (line, key) :: clausesFrom rest (line + 1) fences
      | none => clausesFrom rest (line + 1) fences

def clauseLines (text : String) : List (Nat × String) :=
  clausesFrom (textLines text) 0 0

/-- @models REQ-SHOW.claims_beside_code -/
def clauseChips (text id : String) (claims : List (Option String × Role)) : List Chip :=
  (clauseLines text).bind fun (pair : Nat × String) =>
    let letters := (claims.filter (·.1 == some pair.2)).filterMap (letter ·.2)
    (["M", "I", "T", "D", "P"].filter letters.contains).map fun l =>
      { line := pair.1, letter := l, requirement := id ++ "." ++ pair.2 }

theorem letter_is_shown (r : Role) (l : String) (h : letter r = some l) :
    l ∈ ["M", "I", "T", "D", "P"] := by
  cases r <;> simp [letter] at h <;> subst h <;> simp

/-- @proves REQ-SHOW.claims_beside_code -/
theorem every_claim_is_marked_beside_its_clause (text id : String)
    (claims : List (Option String × Role)) :
    (∀ chip ∈ clauseChips text id claims, ∃ key, (chip.line, key) ∈ clauseLines text ∧
      chip.requirement = id ++ "." ++ key ∧
      ∃ claim ∈ claims, claim.1 = some key ∧ letter claim.2 = some chip.letter) ∧
    (∀ line key, (line, key) ∈ clauseLines text → ∀ claim ∈ claims, claim.1 = some key →
      ∀ l, letter claim.2 = some l →
        ({ line := line, letter := l, requirement := id ++ "." ++ key } : Chip) ∈
          clauseChips text id claims) := by
  constructor
  · intro chip h
    simp only [clauseChips, List.mem_bind, List.mem_map, List.mem_filter] at h
    obtain ⟨⟨line, key⟩, inLines, l, ⟨_, has⟩, rfl⟩ := h
    simp only [List.elem_eq_mem, decide_eq_true_eq, List.mem_filterMap, List.mem_filter,
      beq_iff_eq] at has
    obtain ⟨claim, ⟨inClaims, same⟩, lt⟩ := has
    exact ⟨key, inLines, rfl, claim, inClaims, same, lt⟩
  · intro line key inLines claim inClaims same l lt
    simp only [clauseChips, List.mem_bind, List.mem_map, List.mem_filter]
    refine ⟨(line, key), inLines, l, ⟨letter_is_shown claim.2 l lt, ?_⟩, rfl⟩
    simp only [List.elem_eq_mem, decide_eq_true_eq, List.mem_filterMap, List.mem_filter,
      beq_iff_eq]
    exact ⟨claim, ⟨inClaims, same⟩, lt⟩

end TraceLean.Chips
