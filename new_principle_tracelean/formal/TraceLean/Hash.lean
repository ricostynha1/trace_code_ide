import Lean

namespace TraceLean.Hash

open Lean (ToJson FromJson)

def rangeAt (ranges : List (Nat × Nat)) (i : Nat) : Option (Nat × Nat) :=
  ranges.find? (fun r => r.1 ≤ i && i < r.2)

def isSpace' (c : Char) : Bool :=
  c == ' ' || c == '\t' || c == '\n' || c == '\r'
    || c == Char.ofNat 12 || c == Char.ofNat 11

private def normalizeGo (cs : List Char) (n : Nat) (removed protected_ : List (Nat × Nat))
    (fuel i : Nat) (lastWasSpace : Bool) (acc : List Char) : List Char :=
  match fuel with
  | 0 => acc
  | fuel + 1 =>
    if i ≥ n then acc
    else
      match rangeAt removed i with
      | some (_, e) =>
        if lastWasSpace then normalizeGo cs n removed protected_ fuel e true acc
        else normalizeGo cs n removed protected_ fuel e true (acc ++ [' '])
      | none =>
        match rangeAt protected_ i with
        | some (s, e) =>
          normalizeGo cs n removed protected_ fuel e false (acc ++ ((cs.drop s).take (e - s)))
        | none =>
          let c := cs.getD i ' '
          if isSpace' c then
            if lastWasSpace then normalizeGo cs n removed protected_ fuel (i + 1) true acc
            else normalizeGo cs n removed protected_ fuel (i + 1) true (acc ++ [' '])
          else normalizeGo cs n removed protected_ fuel (i + 1) false (acc ++ [c])

/-- @models REQ-ANCHOR.whitespace_normalised
@models REQ-ANCHOR.comments_excluded -/
def normalize (source : String) (removed protected_ : List (Nat × Nat)) : String :=
  let cs := source.toList
  let n := cs.length
  let out := normalizeGo cs n removed protected_ (n + 1) 0 true []
  String.mk (out.reverse.dropWhile (· == ' ')).reverse

def fnvPrime : UInt64 := 0x00000100000001B3
def fnvOffset : UInt64 := 0xcbf29ce484222325

def hexDigit (n : UInt64) : Char :=
  let d := (UInt64.land n 0xf).toNat
  if d < 10 then Char.ofNat ('0'.toNat + d) else Char.ofNat ('a'.toNat + d - 10)

def toHex16 (h : UInt64) : String :=
  String.mk ((List.range 16).map
    (fun i => hexDigit (UInt64.shiftRight h (UInt64.ofNat ((15 - i) * 4)))))

def digest (s : String) : String :=
  toHex16 (s.toUTF8.toList.foldl
    (fun h b => (UInt64.xor h (UInt64.ofNat b.toNat)) * fnvPrime) fnvOffset)

/-- @models REQ-ANCHOR.hash_tracks_body -/
def bodyHash (source : String) (removed protected_ : List (Nat × Nat)) : String :=
  digest (normalize source removed protected_)

def keyedBuffer (entries : List (String × String)) : String :=
  let sorted := (entries.foldl
    (fun acc kv =>
      if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
      else acc ++ [kv]) []).mergeSort (fun a b => decide (a.1 ≤ b.1))
  sorted.foldl
    (fun acc kv =>
      acc ++ kv.1 ++ String.mk [Char.ofNat 1] ++ kv.2.trim ++ String.mk [Char.ofNat 2]) ""

def rustInput (path : String) : Bool :=
  path.endsWith ".rs" || path.endsWith "Cargo.toml" || path.endsWith "Cargo.lock"

def sourcesHash (files : List (String × String)) : String :=
  let nul := String.mk [Char.ofNat 0]
  let kept := (files.foldl
    (fun acc kv =>
      if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
      else acc ++ [kv]) []).mergeSort (fun a b => decide (a.1 ≤ b.1))
  digest ((kept.filter (fun kv => rustInput kv.1)).foldl
    (fun acc kv => acc ++ kv.1 ++ nul ++ digest kv.2 ++ nul) "")

/-- @models REQ-STALE.measured_not_retaken -/
def retake (measured : Bool) (held : String) (files : List (String × String)) (again : Bool) : Bool :=
  again || !measured || held != sourcesHash files

/-- @models REQ-REQDOC.clause_addressable
@models REQ-STALE.requirement_reopens_all -/
def clauseHash (key : Option String) (text : String) (narrowings : List (String × String)) :
    String :=
  match key with
  | none => digest (normalize text [] [])
  | some k =>
    digest (k ++ String.mk [Char.ofNat 1] ++ text.trim ++ String.mk [Char.ofNat 2]
      ++ String.mk [Char.ofNat 3] ++ keyedBuffer narrowings)

/-- @proves REQ-STALE.measured_not_retaken -/
theorem measured_again_only_when_its_sources_moved (measured : Bool) (held : String)
    (files : List (String × String)) (again : Bool) :
    retake true (sourcesHash files) files false = false ∧
    retake measured held files true = true ∧
    retake false held files again = true ∧
    (held ≠ sourcesHash files → retake measured held files again = true) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · simp [retake]
  · simp [retake]
  · simp [retake]
  · intro moved
    simp [retake, moved]

theorem slice_eq (cs cs' : List Char) (s e : Nat) (hlen : cs.length = cs'.length)
    (hagree : ∀ j, s ≤ j → j < e → cs.getD j ' ' = cs'.getD j ' ') :
    (cs.drop s).take (e - s) = (cs'.drop s).take (e - s) := by
  apply List.ext_getElem
  · simp [hlen]
  · intro i h1 h2
    simp only [List.length_take, List.length_drop] at h1 h2
    rw [List.getElem_take', List.getElem_take', List.getElem_drop, List.getElem_drop]
    have ha := hagree (s + i) (by omega) (by omega)
    rw [List.getD_eq_getElem?_getD, List.getD_eq_getElem?_getD,
      List.getElem?_eq_getElem (by omega), List.getElem?_eq_getElem (by omega)] at ha
    simpa using ha


theorem normalizeGo_congr (cs cs' : List Char) (n : Nat) (removed protected_ : List (Nat × Nat))
    (hlen : cs.length = cs'.length)
    (hagree : ∀ j, cs.getD j ' ' = cs'.getD j ' ' ∨
      (((rangeAt removed j).isSome ∨ (isSpace' (cs.getD j ' ') ∧ isSpace' (cs'.getD j ' '))) ∧
        ∀ r ∈ protected_, ¬(r.1 ≤ j ∧ j < r.2))) :
    ∀ fuel i lastWasSpace acc,
      normalizeGo cs n removed protected_ fuel i lastWasSpace acc =
        normalizeGo cs' n removed protected_ fuel i lastWasSpace acc := by
  intro fuel
  induction fuel with
  | zero => intro _ _ _; simp [normalizeGo]
  | succ fuel ih =>
    intro i l acc
    simp only [normalizeGo]
    split
    · rfl
    · split
      · split <;> exact ih _ _ _
      · rename_i hrem
        split
        · rename_i s e hp
          have hmem := List.mem_of_find?_eq_some hp
          rw [slice_eq cs cs' s e hlen (fun j h1 h2 => by
            rcases hagree j with h | ⟨_, hn⟩
            · exact h
            · exact absurd ⟨h1, h2⟩ (hn _ hmem))]
          exact ih _ _ _
        · rcases hagree i with h | ⟨h | ⟨h1, h2⟩, _⟩
          · rw [h]
            split
            · split <;> exact ih _ _ _
            · exact ih _ _ _
          · simp [hrem] at h
          · simp only [h1, h2, if_true]
            split <;> exact ih _ _ _

/-- @proves REQ-ANCHOR.comments_excluded -/
theorem comment_text_does_not_reach_the_hash (source source' : String)
    (removed protected_ : List (Nat × Nat)) (hlen : source.length = source'.length)
    (hagree : ∀ j, source.toList.getD j ' ' = source'.toList.getD j ' ' ∨
      ((rangeAt removed j).isSome ∧ ∀ r ∈ protected_, ¬(r.1 ≤ j ∧ j < r.2))) :
    normalize source removed protected_ = normalize source' removed protected_ := by
  have hl : source.toList.length = source'.toList.length := by
    simpa [String.length] using hlen
  unfold normalize
  dsimp only
  rw [hl, normalizeGo_congr source.toList source'.toList _ removed protected_ hl
    (fun j => (hagree j).imp_right (fun ⟨h1, h2⟩ => ⟨Or.inl h1, h2⟩))]

/-- @proves REQ-ANCHOR.hash_tracks_body -/
theorem body_hash_moves_only_with_the_normalised_body (source source' : String)
    (removed removed' protected_ protected' : List (Nat × Nat)) :
    normalize source removed protected_ = normalize source' removed' protected' →
      bodyHash source removed protected_ = bodyHash source' removed' protected' := by
  intro h
  unfold bodyHash
  rw [h]

theorem keep_trailing (cs : List Char) (h : cs.getLast? ≠ some ' ') :
    (cs.reverse.dropWhile (· == ' ')).reverse = cs := by
  rw [List.getLast?_eq_head?_reverse] at h
  cases hr : cs.reverse with
  | nil =>
    simp only [List.dropWhile_nil, List.reverse_nil]
    exact (List.reverse_eq_nil_iff.mp hr).symm
  | cons c rest =>
    rw [hr] at h
    have hc : (c == ' ') = false := by simpa using h
    simp only [List.dropWhile_cons, hc, Bool.false_eq_true, if_false]
    rw [← hr, List.reverse_reverse]

theorem literal_pass (cs : List Char) :
    normalizeGo cs cs.length [] [(0, cs.length)] (cs.length + 1) 0 true [] = cs := by
  cases cs with
  | nil => simp [normalizeGo]
  | cons c rest => simp [normalizeGo, rangeAt]

/-- @proves REQ-ANCHOR.whitespace_normalised -/
theorem whitespace_is_normalised_outside_literals_only (source source' : String)
    (removed protected_ : List (Nat × Nat)) (hlen : source.length = source'.length)
    (hagree : ∀ j, source.toList.getD j ' ' = source'.toList.getD j ' ' ∨
      ((isSpace' (source.toList.getD j ' ') ∧ isSpace' (source'.toList.getD j ' ')) ∧
        ∀ r ∈ protected_, ¬(r.1 ≤ j ∧ j < r.2)))
    (literal : String) (hlast : literal.toList.getLast? ≠ some ' ') :
    normalize source removed protected_ = normalize source' removed protected_ ∧
    normalize literal [] [(0, literal.length)] = literal := by
  constructor
  · have hl : source.toList.length = source'.toList.length := by
      simpa [String.length] using hlen
    unfold normalize
    dsimp only
    rw [hl, normalizeGo_congr source.toList source'.toList _ removed protected_ hl
      (fun j => (hagree j).imp_right (fun ⟨h1, h2⟩ => ⟨Or.inr h1, h2⟩))]
  · obtain ⟨cs⟩ := literal
    unfold normalize
    show String.mk ((normalizeGo cs cs.length [] [(0, cs.length)] (cs.length + 1) 0 true []).reverse.dropWhile
      (· == ' ')).reverse = String.mk cs
    rw [literal_pass, keep_trailing cs hlast]

end TraceLean.Hash
