import TraceLean.Lsp

/-!
# What a column is, in each encoding

`REQ-LSP.encoding_round_trip` asks that converting a position between the
editor's encoding and the protocol's round-trips exactly. That pins nothing by
itself; what makes the conversion exact is what a column *means*. A column in an
encoding is the number of that encoding's units before the position, so column
`n` is the byte offset where a prefix of the line spanning exactly `n` units
ends -- and there is no such offset when `n` falls inside a character or past
the end of the line.
-/

namespace TraceLean.SpecLsp

open TraceLean.Lsp

/-- How many units of `encoding` one character takes: its UTF-8 bytes, its
UTF-16 code units (two for a character beyond the basic multilingual plane),
or one code point. -/
def unitsOf (encoding : Encoding) (c : Char) : Nat :=
  match encoding with
  | .utf8 => c.utf8Size
  | .utf16 => if c.toNat ≤ 0xFFFF then 1 else 2
  | .utf32 => 1

/-- The columns a run of characters spans. -/
def columnsOf (encoding : Encoding) (cs : List Char) : Nat :=
  (cs.map (unitsOf encoding)).foldr (· + ·) 0

/-- The bytes a run of characters takes in UTF-8. -/
def bytesOf (cs : List Char) : Nat :=
  (cs.map Char.utf8Size).foldr (· + ·) 0

/-- Column `character`, counted in `encoding`, is byte `b` of the line exactly
when some prefix of the line spans that many columns and that many bytes; a
column no prefix ends at -- inside a character, or past the end -- has no byte
offset.

@specifies REQ-LSP.encoding_round_trip -/
def ByteOffsetAt (lineText : String) (character : Nat) (encoding : Encoding) (y : Option Nat) : Prop :=
  ∀ b, y = some b ↔
    ∃ pre post, lineText.toList = pre ++ post ∧ columnsOf encoding pre = character ∧ bytesOf pre = b

theorem width_eq (encoding : Encoding) (c : Char) : width encoding c = unitsOf encoding c := by
  cases encoding
  · show String.utf8ByteSize (String.mk [c]) = c.utf8Size
    simp [String.utf8ByteSize, String.utf8ByteSize.go]
  · show (if c.val < 0x10000 then 1 else 2) = (if c.toNat ≤ 0xFFFF then 1 else 2)
    refine (Classical.em (c.toNat ≤ 0xFFFF)).elim ?_ ?_
    · intro h
      have h2 : c.val < 0x10000 := by
        show c.toNat < 65536
        omega
      rw [if_pos h, if_pos h2]
    · intro h
      have h2 : ¬ c.val < 0x10000 := by
        intro h3
        have h4 : c.toNat < 65536 := h3
        omega
      rw [if_neg h, if_neg h2]
  · rfl

theorem units_pos (encoding : Encoding) (c : Char) : 0 < unitsOf encoding c := by
  cases encoding
  · exact Char.utf8Size_pos c
  · show 0 < (if c.toNat ≤ 0xFFFF then 1 else 2)
    split
    · omega
    · omega
  · show 0 < 1
    omega

theorem byte_size_one (c : Char) : String.utf8ByteSize (String.mk [c]) = c.utf8Size := by
  simp [String.utf8ByteSize, String.utf8ByteSize.go]

theorem columns_nil (encoding : Encoding) : columnsOf encoding [] = 0 := rfl

theorem columns_cons (encoding : Encoding) (c : Char) (cs : List Char) :
    columnsOf encoding (c :: cs) = unitsOf encoding c + columnsOf encoding cs := rfl

theorem bytes_nil : bytesOf [] = 0 := rfl

theorem bytes_cons (c : Char) (cs : List Char) : bytesOf (c :: cs) = c.utf8Size + bytesOf cs := rfl

theorem columns_zero (encoding : Encoding) (pre : List Char) (h : columnsOf encoding pre = 0) :
    pre = [] := by
  cases pre
  case nil => rfl
  case cons c rest =>
    rw [columns_cons] at h
    have hp := units_pos encoding c
    omega

/-- The model's walk finds the offset exactly where a prefix spans the column. -/
theorem go_spec (character : Nat) (encoding : Encoding) (cs : List Char) :
    ∀ counted offset b, toByteOffset.go character encoding cs counted offset = some b ↔
      ∃ pre post, cs = pre ++ post ∧ counted + columnsOf encoding pre = character ∧
        offset + bytesOf pre = b := by
  induction cs
  case nil =>
    intro counted offset b
    simp only [toByteOffset.go]
    refine (Classical.em (counted = character)).elim ?_ ?_
    · intro e
      have hc : (counted == character) = true := by
        simp [e]
      rw [if_pos hc]
      constructor
      · intro h
        have hb : offset = b := Option.some.inj h
        refine ⟨[], [], rfl, ?_, ?_⟩
        · rw [columns_nil]
          omega
        · rw [bytes_nil]
          omega
      · intro h
        refine h.elim ?_
        intro pre h
        refine h.elim ?_
        intro post h
        have hpre : pre = [] := by
          apply columns_zero encoding pre
          omega
        have h3 := h.2.2
        rw [hpre, bytes_nil] at h3
        rw [← h3]
        rfl
    · intro e
      have hc : ¬ (counted == character) = true := by
        simp [e]
      rw [if_neg hc]
      constructor
      · intro h
        cases h
      · intro h
        refine h.elim ?_
        intro pre h
        refine h.elim ?_
        intro post h
        have hpre : pre = [] := by
          have h4 := congrArg List.length h.1
          simp at h4
          have hl : pre.length = 0 := by
            omega
          exact List.eq_nil_of_length_eq_zero hl
        have h5 := h.2.1
        rw [hpre, columns_nil] at h5
        exact absurd h5 (by omega)
  case cons c rest ih =>
    intro counted offset b
    simp only [toByteOffset.go]
    refine (Classical.em (counted = character)).elim ?_ ?_
    · intro e
      have hc : (counted == character) = true := by
        simp [e]
      rw [if_pos hc]
      constructor
      · intro h
        have hb : offset = b := Option.some.inj h
        refine ⟨[], c :: rest, rfl, ?_, ?_⟩
        · rw [columns_nil]
          omega
        · rw [bytes_nil]
          omega
      · intro h
        refine h.elim ?_
        intro pre h
        refine h.elim ?_
        intro post h
        have hpre : pre = [] := by
          apply columns_zero encoding pre
          omega
        have h3 := h.2.2
        rw [hpre, bytes_nil] at h3
        rw [← h3]
        rfl
    · intro e
      have hc : ¬ (counted == character) = true := by
        simp [e]
      rw [if_neg hc]
      simp only [width_eq, byte_size_one]
      refine (Classical.em (counted + unitsOf encoding c > character)).elim ?_ ?_
      · intro hgt
        rw [if_pos hgt]
        constructor
        · intro h
          cases h
        · intro h
          refine h.elim ?_
          intro pre h0
          refine h0.elim ?_
          intro post h
          clear h0
          cases pre
          case nil =>
            have h5 := h.2.1
            rw [columns_nil] at h5
            exact absurd h5 (by omega)
          case cons d pre =>
            have h6 := h.1
            simp at h6
            have h5 := h.2.1
            rw [columns_cons, ← h6.1] at h5
            exact absurd h5 (by omega)
      · intro hgt
        rw [if_neg hgt]
        rw [ih]
        constructor
        · intro h
          refine h.elim ?_
          intro pre h
          refine h.elim ?_
          intro post h
          refine ⟨c :: pre, post, ?_, ?_, ?_⟩
          · rw [h.1]
            rfl
          · rw [columns_cons]
            omega
          · rw [bytes_cons]
            omega
        · intro h
          refine h.elim ?_
          intro pre h0
          refine h0.elim ?_
          intro post h
          clear h0
          cases pre
          case nil =>
            have h5 := h.2.1
            rw [columns_nil] at h5
            exact absurd h5 e
          case cons d pre =>
            have h6 := h.1
            simp at h6
            have h5 := h.2.1
            have h7 := h.2.2
            rw [columns_cons, ← h6.1] at h5
            rw [bytes_cons, ← h6.1] at h7
            refine ⟨pre, post, h6.2, ?_, ?_⟩
            · omega
            · omega

/-- @pins REQ-LSP.encoding_round_trip -/
theorem encoding_round_trip_pinned :
    (∀ x1 x2 x3, ByteOffsetAt x1 x2 x3 (toByteOffset x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, ByteOffsetAt x1 x2 x3 y1 → ByteOffsetAt x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3 b
    unfold toByteOffset
    have k := go_spec x2 x3 x1.toList 0 0 b
    rw [k]
    constructor
    · intro h
      refine h.elim ?_
      intro pre h
      refine h.elim ?_
      intro post h
      refine ⟨pre, post, h.1, ?_, ?_⟩
      · omega
      · omega
    · intro h
      refine h.elim ?_
      intro pre h
      refine h.elim ?_
      intro post h
      refine ⟨pre, post, h.1, ?_, ?_⟩
      · omega
      · omega
  · intro x1 x2 x3 y1 y2 h1 h2
    cases y1
    case none =>
      cases y2
      case none => rfl
      case some b =>
        have k := (h1 b).2 ((h2 b).1 rfl)
        cases k
    case some b =>
      exact ((h2 b).2 ((h1 b).1 rfl)).symm

end TraceLean.SpecLsp
