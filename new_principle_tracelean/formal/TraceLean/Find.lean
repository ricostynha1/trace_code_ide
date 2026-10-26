import Lean

namespace TraceLean.Find

def fold (c : Char) : Char := c.toLower

def dedupStars (xs : List Char) : List Char :=
  (xs.foldl (fun (acc : List Char) (c : Char) =>
    if c == '*' && acc.head? == some '*' then acc else c :: acc) []).reverse

def shortest : Nat → List Char → List Char → Option Nat
  | 0, _, _ => none
  | fuel + 1, [], _ => some 0
  | fuel + 1, p :: rest, text =>
    if p == '*' then
      match shortest fuel rest text with
      | some n => some n
      | none =>
        match text with
        | [] => none
        | t :: tail => (shortest fuel (p :: rest) tail).map (fun n => n + 1)
    else
      match text with
      | [] => none
      | t :: tail =>
        if p == '?' || p == t then (shortest fuel rest tail).map (fun n => n + 1) else none

def scanLine (pattern : List Char) (lineStart : Nat) : Nat → Nat → List Char → List (Nat × Nat)
  | 0, _, _ => []
  | fuel + 1, at_, [] => []
  | fuel + 1, at_, text =>
    match shortest (pattern.length + text.length + 1) pattern text with
    | some length =>
      if length > 0 then
        (lineStart + at_, lineStart + at_ + length) :: scanLine pattern lineStart fuel (at_ + length) (text.drop length)
      else scanLine pattern lineStart fuel (at_ + 1) text.tail
    | none => scanLine pattern lineStart fuel (at_ + 1) text.tail

/-- @models REQ-NAV.find_by_glob -/
def globMatches (text pattern : String) : List (Nat × Nat) :=
  let wanted := dedupStars (pattern.toList.map fold)
  if wanted.isEmpty then []
  else
    let lines := text.splitOn "\n"
    (lines.foldl (fun (acc : List (Nat × Nat) × Nat) (line : String) =>
      let chars := line.toList.map fold
      (acc.1 ++ scanLine wanted acc.2 chars.length 0 chars, acc.2 + chars.length + 1)) ([], 0)).1

/-- @models REQ-NAV.find_lists_lines -/
def matchingLines (text pattern : String) : List (Nat × String) :=
  ((text.splitOn "\n").enum.filter (fun (p : Nat × String) => !(globMatches p.2 pattern).isEmpty)).map
    (fun (p : Nat × String) => (p.1 + 1, p.2))

inductive GlobMatch : List Char → List Char → Prop where
  | done : GlobMatch [] []
  | starEmpty {rest text : List Char} : GlobMatch rest text → GlobMatch ('*' :: rest) text
  | starMore {rest text : List Char} (c : Char) :
      GlobMatch ('*' :: rest) text → GlobMatch ('*' :: rest) (c :: text)
  | one {p c : Char} {rest text : List Char} : p ≠ '*' → (p = '?' ∨ p = c) →
      GlobMatch rest text → GlobMatch (p :: rest) (c :: text)

theorem shortest_sound :
    ∀ (fuel : Nat) (pattern text : List Char) (n : Nat), shortest fuel pattern text = some n →
      n ≤ text.length ∧ GlobMatch pattern (text.take n)
  | 0, _, _, _, h => by simp [shortest] at h
  | _ + 1, [], text, n, h => by
    simp only [shortest, Option.some.injEq] at h
    subst h
    exact ⟨Nat.zero_le _, by simpa using GlobMatch.done⟩
  | fuel + 1, p :: rest, text, n, h => by
    simp only [shortest] at h
    split at h
    · rename_i hp
      have hp : p = '*' := by simpa using hp
      subst hp
      cases hm : shortest fuel rest text with
      | some m =>
        rw [hm] at h
        have ⟨hl, hg⟩ := shortest_sound fuel rest text m hm
        cases h
        exact ⟨hl, GlobMatch.starEmpty hg⟩
      | none =>
        rw [hm] at h
        cases text with
        | nil => cases h
        | cons t tail =>
          cases hm' : shortest fuel ('*' :: rest) tail with
          | none => simp [hm'] at h
          | some m =>
            simp only [hm', Option.map_some', Option.some.injEq] at h
            subst h
            have ⟨hl, hg⟩ := shortest_sound fuel ('*' :: rest) tail m hm'
            exact ⟨by simp; omega, by simpa using GlobMatch.starMore t hg⟩
    · rename_i hp
      have hp : p ≠ '*' := by simpa using hp
      cases text with
      | nil => cases h
      | cons t tail =>
        simp only at h
        split at h
        · rename_i hc
          cases hm : shortest fuel rest tail with
          | none => simp [hm] at h
          | some m =>
            simp only [hm, Option.map_some', Option.some.injEq] at h
            subst h
            have ⟨hl, hg⟩ := shortest_sound fuel rest tail m hm
            have hc' : p = '?' ∨ p = t := by simpa using hc
            exact ⟨by simp; omega, by simpa using GlobMatch.one hp hc' hg⟩
        · cases h

theorem scanLine_sound (pattern : List Char) (lineStart : Nat) :
    ∀ (fuel at_ : Nat) (text : List Char) (m : Nat × Nat), m ∈ scanLine pattern lineStart fuel at_ text →
      lineStart + at_ ≤ m.1 ∧ m.1 < m.2 ∧
        GlobMatch pattern ((text.drop (m.1 - lineStart - at_)).take (m.2 - m.1))
  | 0, _, _, m, h => by simp [scanLine] at h
  | _ + 1, _, [], m, h => by simp [scanLine] at h
  | fuel + 1, at_, c :: cs, m, h => by
    simp only [scanLine] at h
    split at h
    · rename_i len hlen
      split at h
      · rename_i hpos
        rcases List.mem_cons.mp h with rfl | h
        · refine ⟨Nat.le_refl _, by simp; omega, ?_⟩
          have := (shortest_sound _ _ _ _ hlen).2
          have e1 : lineStart + at_ + len - (lineStart + at_) = len := by omega
          have e2 : lineStart + at_ - lineStart - at_ = 0 := by omega
          simp only [e1, e2, List.drop_zero]
          exact this
        · have ⟨h1, h2, h3⟩ := scanLine_sound pattern lineStart fuel (at_ + len) _ m h
          refine ⟨by omega, h2, ?_⟩
          rw [List.drop_drop] at h3
          have : m.1 - lineStart - (at_ + len) + len = m.1 - lineStart - at_ := by omega
          rw [this] at h3
          exact h3
      · have ⟨h1, h2, h3⟩ := scanLine_sound pattern lineStart fuel (at_ + 1) _ m h
        refine ⟨by omega, h2, ?_⟩
        have : m.1 - lineStart - at_ = (m.1 - lineStart - (at_ + 1)) + 1 := by omega
        rw [this]
        simpa using h3
    · have ⟨h1, h2, h3⟩ := scanLine_sound pattern lineStart fuel (at_ + 1) _ m h
      refine ⟨by omega, h2, ?_⟩
      have : m.1 - lineStart - at_ = (m.1 - lineStart - (at_ + 1)) + 1 := by omega
      rw [this]
      simpa using h3

theorem foldlInv {α β : Type} (P : β → Prop) (f : β → α → β) :
    ∀ (xs : List α) (init : β), P init → (∀ b, ∀ x ∈ xs, P b → P (f b x)) → P (xs.foldl f init)
  | [], _, h, _ => h
  | x :: xs, init, h, step =>
    foldlInv P f xs (f init x) (step init x (by simp) h)
      (fun b y hy hb => step b y (List.mem_cons_of_mem _ hy) hb)

/-- @proves REQ-NAV.find_by_glob -/
theorem every_match_is_a_nonempty_glob_match (text pattern : String) (m : Nat × Nat)
    (h : m ∈ globMatches text pattern) :
    m.1 < m.2 ∧ ∃ line ∈ text.splitOn "\n", ∃ start,
      GlobMatch (dedupStars (pattern.toList.map fold))
        (((line.toList.map fold).drop start).take (m.2 - m.1)) := by
  unfold globMatches at h
  dsimp only at h
  split at h
  · simp at h
  · revert m
    refine foldlInv (fun (acc : List (Nat × Nat) × Nat) => ∀ m ∈ acc.1, m.1 < m.2 ∧
        ∃ line ∈ text.splitOn "\n", ∃ start,
          GlobMatch (dedupStars (pattern.toList.map fold))
            (((line.toList.map fold).drop start).take (m.2 - m.1))) _ _ _ (by simp) ?_
    intro acc line hline hacc m hm
    rcases List.mem_append.mp hm with hm | hm
    · exact hacc m hm
    · have ⟨_, h2, h3⟩ := scanLine_sound _ _ _ _ _ m hm
      exact ⟨h2, line, hline, _, h3⟩

theorem enumFrom_ordered {α : Type} :
    ∀ (j : Nat) (xs : List α), (xs.enumFrom j).Pairwise (fun a b => a.1 < b.1)
  | _, [] => List.Pairwise.nil
  | j, x :: xs => by
    rw [List.enumFrom_cons]
    refine List.Pairwise.cons ?_ (enumFrom_ordered (j + 1) xs)
    intro b hb
    have := (List.mem_enumFrom (x := b.2) (i := b.1) hb).1
    omega

/-- @proves REQ-NAV.find_lists_lines -/
theorem matching_lines_are_exactly_the_lines_with_a_match (text pattern : String)
    (number : Nat) (line : String) :
    ((number, line) ∈ matchingLines text pattern ↔
      0 < number ∧ (text.splitOn "\n")[number - 1]? = some line ∧
        globMatches line pattern ≠ []) ∧
    (matchingLines text pattern).Pairwise (fun a b => a.1 < b.1) := by
  constructor
  · unfold matchingLines
    simp only [List.mem_map, List.mem_filter, Prod.mk.injEq, Bool.not_eq_true',
      List.isEmpty_eq_false, ne_eq]
    constructor
    · rintro ⟨⟨i, l⟩, ⟨hmem, hne⟩, rfl, rfl⟩
      refine ⟨by omega, ?_, ?_⟩
      · simpa using List.mem_enum_iff_getElem?.mp hmem
      · intro he; simp [he] at hne
    · rintro ⟨hpos, hget, hne⟩
      refine ⟨(number - 1, line), ⟨List.mem_enum_iff_getElem?.mpr hget, ?_⟩, by omega, rfl⟩
      cases hg : globMatches line pattern with
      | nil => exact absurd hg hne
      | cons _ _ => simp
  · unfold matchingLines
    rw [List.pairwise_map]
    refine List.Pairwise.filter _ ?_
    have := enumFrom_ordered 0 (text.splitOn "\n")
    refine this.imp ?_
    intro a b hab
    omega

end TraceLean.Find
