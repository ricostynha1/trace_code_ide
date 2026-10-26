import Lean

namespace TraceLean.Signature

open Lean (ToJson FromJson)

def isAlphaNum (c : Char) : Bool := c.isAlphanum
def isIdentStart (c : Char) : Bool := c.isAlpha || c == '_'

def isIdentifier (s : String) : Bool :=
  match s.toList with
  | [] => false
  | c :: rest => isIdentStart c && rest.all (fun ch => isAlphaNum ch || ch == '_')

private def matchingGo (cs : List Char) (open_ close : Char)
    : Nat → Nat → Int → Option Nat
  | 0, _, _ => none
  | fuel + 1, i, depth =>
    if i ≥ cs.length then none
    else
      let ch := cs.getD i ' '
      if ch == close && close == '>' && i > 0 && cs.getD (i - 1) ' ' == '-' then
        matchingGo cs open_ close fuel (i + 1) depth
      else if ch == open_ then matchingGo cs open_ close fuel (i + 1) (depth + 1)
      else if ch == close then
        if depth - 1 == 0 then some i
        else matchingGo cs open_ close fuel (i + 1) (depth - 1)
      else matchingGo cs open_ close fuel (i + 1) depth

def matching (cs : List Char) (start : Nat) (open_ close : Char) : Option Nat :=
  matchingGo cs open_ close (cs.length + 1) start 0

inductive Mode where
  | code
  | line
  | block
  | quoted (q : Char)
  deriving Repr, DecidableEq, Inhabited

def keepNewline (c : Char) : Char := if c == '\n' then '\n' else ' '

def codeChar (quotes : List Char) (charLits : Bool) (c : Char) (rest : List Char)
    : List Char × Nat × Mode :=
  match charLits && c == '\'', rest with
  | true, '\\' :: e :: '\'' :: _ => (['\'', '\\', e, '\''], 4, Mode.code)
  | true, e :: '\'' :: _ => (['\'', e, '\''], 3, Mode.code)
  | _, _ =>
    match quotes.contains c with
    | true => ([' '], 1, Mode.quoted c)
    | false => ([c], 1, Mode.code)

def blankStep (quotes : List Char) (charLits : Bool) : Mode → List Char → List Char × Nat × Mode
  | Mode.code, '/' :: '/' :: _ => ([' ', ' '], 2, Mode.line)
  | Mode.code, '/' :: '*' :: _ => ([' ', ' '], 2, Mode.block)
  | Mode.code, c :: rest => codeChar quotes charLits c rest
  | Mode.line, '\n' :: _ => (['\n'], 1, Mode.code)
  | Mode.line, _ => ([' '], 1, Mode.line)
  | Mode.block, '*' :: '/' :: _ => ([' ', ' '], 2, Mode.code)
  | Mode.block, c :: _ => ([keepNewline c], 1, Mode.block)
  | Mode.quoted q, '\\' :: _ :: _ => ([' ', ' '], 2, Mode.quoted q)
  | Mode.quoted q, c :: _ =>
    match c == q with
    | true => ([' '], 1, Mode.code)
    | false => ([keepNewline c], 1, Mode.quoted q)
  | m, [] => ([], 1, m)

private def blankGo (quotes : List Char) (charLits : Bool) : Nat → Mode → List Char → List Char
  | 0, _, _ => []
  | _, _, [] => []
  | fuel + 1, mode, cs =>
    let s := blankStep quotes charLits mode cs
    s.1 ++ blankGo quotes charLits fuel s.2.2 (cs.drop s.2.1)

def blanked (cs : List Char) (quotes : List Char) (charLits : Bool) : List Char :=
  blankGo quotes charLits (cs.length + 1) Mode.code cs

private def skipSpaceGo (cs : List Char) : Nat → Nat → Nat
  | 0, i => i
  | fuel + 1, i =>
    if i < cs.length && (cs.getD i 'x').isWhitespace then skipSpaceGo cs fuel (i + 1) else i

private def openParenGo (cs : List Char) (target : List Char) : Nat → Nat → Option Nat
  | 0, _ => none
  | fuel + 1, from_ =>
    match decide (from_ + 3 > cs.length) with
    | true => none
    | false =>
      match (cs.drop from_).take 3 != ['f', 'n', ' '] with
      | true => openParenGo cs target fuel (from_ + 1)
      | false =>
        let at_ := from_ + 3
        let nameLen := ((cs.drop at_).takeWhile (fun c => isAlphaNum c || c == '_')).length
        match (cs.drop at_).take nameLen != target with
        | true => openParenGo cs target fuel (from_ + 1)
        | false =>
          let afterName := skipSpaceGo cs (cs.length + 1) (at_ + nameLen)
          let cursor :=
            match afterName < cs.length && cs.getD afterName ' ' == '<' with
            | true =>
              match matching cs afterName '<' '>' with
              | none => cs.length
              | some e => skipSpaceGo cs (cs.length + 1) (e + 1)
            | false => afterName
          match cursor < cs.length && cs.getD cursor ' ' == '(' with
          | true => some cursor
          | false => openParenGo cs target fuel (from_ + 1)

def signatureOpenParen (cs : List Char) (symbol : String) : Option Nat :=
  openParenGo cs symbol.toList (cs.length + 1) 0

def countFrom (find : Nat → Option Nat) : Nat → Nat → Nat
  | 0, _ => 0
  | fuel + 1, from_ =>
    match find from_ with
    | none => 0
    | some at_ => 1 + countFrom find fuel (at_ + 1)

def declarations (cs : List Char) (symbol : String) : Nat :=
  countFrom (fun from_ => openParenGo cs symbol.toList (cs.length + 1) from_) (cs.length + 1) 0

private def splitTopLevelGo (cs : List Char) (sep : Char)
    : Nat → Nat → Nat → Int → List (List Char) → List (List Char)
  | 0, _, _, _, acc => acc
  | fuel + 1, i, start, depth, acc =>
    match decide (i ≥ cs.length) with
    | true => acc ++ [(cs.drop start).take (cs.length - start)]
    | false =>
      let ch := cs.getD i ' '
      let nested :=
        if ch == '<' || ch == '(' || ch == '[' then depth + 1
        else if ch == '>' || ch == ')' || ch == ']' then depth - 1 else depth
      let level := if ch == '>' && i > 0 && cs.getD (i - 1) ' ' == '-' then nested + 1 else nested
      match ch == sep && level == 0 with
      | true =>
        splitTopLevelGo cs sep fuel (i + 1) (i + 1) level (acc ++ [(cs.drop start).take (i - start)])
      | false => splitTopLevelGo cs sep fuel (i + 1) start level acc

def splitTopLevel (cs : List Char) (sep : Char) : List (List Char) :=
  splitTopLevelGo cs sep (cs.length + 1) 0 0 0 []

def trimChars (cs : List Char) : List Char :=
  (((cs.dropWhile (·.isWhitespace)).reverse).dropWhile (·.isWhitespace)).reverse

def dropPrefix (cs : List Char) (p : List Char) : List Char :=
  if cs.take p.length == p then cs.drop p.length else cs

private def parameterStep (acc : Option (List String)) (raw : List Char) : Option (List String) :=
  match acc with
  | none => none
  | some names =>
    let part := trimChars raw
    match part.isEmpty with
    | true => some names
    | false =>
      let borrowed := trimChars (trimChars (part.dropWhile (· == '&')))
      let head := trimChars (dropPrefix borrowed "mut ".toList)
      match head == "self".toList || (head.take 5 == "self:".toList) with
      | true => some names
      | false =>
        let named :=
          match head.findIdx? (· == ':') with
          | some _ => trimChars (part.takeWhile (· != ':'))
          | none => part
        let name := trimChars (dropPrefix (trimChars named) "mut ".toList)
        let nameStr := String.mk name
        match name.isEmpty || !isIdentifier nameStr with
        | true => none
        | false => some (names ++ [nameStr])

/-- @models REQ-DRT-RUST.params_from_source -/
def parameters (source : String) (symbol : String) : Option (List String) :=
  let cs := blanked source.toList ['"'] true
  match decide (declarations cs symbol > 1), signatureOpenParen cs symbol with
  | true, _ => none
  | false, none => none
  | false, some openAt =>
    match matching cs openAt '(' ')' with
    | none => none
    | some closeAt =>
      let params := (cs.drop (openAt + 1)).take (closeAt - openAt - 1)
      (splitTopLevel params ',').foldl parameterStep (some [])

private def openBrace : String := String.mk [Char.ofNat 123]

private def closeBrace : String := String.mk [Char.ofNat 125]

private def tsOpenParenGo (cs : List Char) (target : List Char) : Nat → Nat → Option Nat
  | 0, _ => none
  | fuel + 1, from_ =>
    match decide (from_ + 9 > cs.length) with
    | true => none
    | false =>
      match (cs.drop from_).take 9 != "function ".toList with
      | true => tsOpenParenGo cs target fuel (from_ + 1)
      | false =>
        let at_ := from_ + 9
        let nameLen := ((cs.drop at_).takeWhile (fun c => isAlphaNum c || c == '_')).length
        match (cs.drop at_).take nameLen != target with
        | true => tsOpenParenGo cs target fuel (from_ + 1)
        | false =>
          let afterName := skipSpaceGo cs (cs.length + 1) (at_ + nameLen)
          let cursor :=
            match afterName < cs.length && cs.getD afterName ' ' == '<' with
            | true =>
              match matching cs afterName '<' '>' with
              | none => cs.length
              | some e => skipSpaceGo cs (cs.length + 1) (e + 1)
            | false => afterName
          match cursor < cs.length && cs.getD cursor ' ' == '(' with
          | true => some cursor
          | false => tsOpenParenGo cs target fuel (from_ + 1)

def tsSignatureOpenParen (cs : List Char) (symbol : String) : Option Nat :=
  tsOpenParenGo cs symbol.toList (cs.length + 1) 0

private def tsParameterStep (acc : Option (List String)) (raw : List Char) : Option (List String) :=
  match acc with
  | none => none
  | some names =>
    let part := trimChars raw
    match part.isEmpty with
    | true => some names
    | false =>
      let named := trimChars (part.takeWhile (fun c => c != ':' && c != '='))
      let nameStr := String.mk named
      match named.isEmpty || !isIdentifier nameStr, nameStr == "this" with
      | true, _ => none
      | false, true => some names
      | false, false => some (names ++ [nameStr])

def tsDeclarations (cs : List Char) (symbol : String) : Nat :=
  countFrom (fun from_ => tsOpenParenGo cs symbol.toList (cs.length + 1) from_) (cs.length + 1) 0

def tsQuotes : List Char := ['"', '\'', '`']

/-- @models REQ-DRT-TS.params_from_source -/
def tsParameters (source : String) (symbol : String) : Option (List String) :=
  let cs := blanked source.toList tsQuotes false
  match decide (tsDeclarations cs symbol > 1), tsSignatureOpenParen cs symbol with
  | true, _ => none
  | false, none => none
  | false, some openAt =>
    match matching cs openAt '(' ')' with
    | none => none
    | some closeAt =>
      let params := (cs.drop (openAt + 1)).take (closeAt - openAt - 1)
      (splitTopLevel params ',').foldl tsParameterStep (some [])

private theorem trimChars_infix (cs : List Char) : trimChars cs <:+: cs := by
  unfold trimChars
  have h1 : cs.dropWhile (·.isWhitespace) <:+: cs := (List.dropWhile_suffix _).isInfix
  have h2 : ((cs.dropWhile (·.isWhitespace)).reverse.dropWhile (·.isWhitespace)) <:+:
      (cs.dropWhile (·.isWhitespace)).reverse := (List.dropWhile_suffix _).isInfix
  have h3 := List.reverse_infix.mpr h2
  rw [List.reverse_reverse] at h3
  exact h3.trans h1

private theorem dropPrefix_infix (cs p : List Char) : dropPrefix cs p <:+: cs := by
  unfold dropPrefix
  split
  · exact (List.drop_suffix _ _).isInfix
  · exact List.infix_refl _

private theorem pieces_infix (cs : List Char) (sep : Char) (fuel i start : Nat) (depth : Int)
    (acc : List (List Char)) (piece : List Char)
    (h : piece ∈ splitTopLevelGo cs sep fuel i start depth acc) : piece ∈ acc ∨ piece <:+: cs := by
  induction fuel generalizing i start depth acc with
  | zero => exact Or.inl (by simpa [splitTopLevelGo] using h)
  | succ f ih =>
    simp only [splitTopLevelGo] at h
    split at h
    · rcases List.mem_append.mp h with h | h
      · exact Or.inl h
      · rw [List.mem_singleton] at h
        subst h
        exact Or.inr ((List.take_prefix _ _).isInfix.trans (List.drop_suffix _ _).isInfix)
    · split at h
      · rcases ih _ _ _ _ h with h | h
        · rcases List.mem_append.mp h with h | h
          · exact Or.inl h
          · rw [List.mem_singleton] at h
            subst h
            exact Or.inr ((List.take_prefix _ _).isInfix.trans (List.drop_suffix _ _).isInfix)
        · exact Or.inr h
      · exact ih _ _ _ _ h

private theorem split_pieces_infix (cs : List Char) (sep : Char) (piece : List Char)
    (h : piece ∈ splitTopLevel cs sep) : piece <:+: cs := by
  rcases pieces_infix cs sep _ _ _ _ [] piece h with h | h
  · simp at h
  · exact h

private def parameterName (raw : List Char) : Option String :=
  let part := trimChars raw
  match part.isEmpty with
  | true => none
  | false =>
    let borrowed := trimChars (trimChars (part.dropWhile (· == '&')))
    let head := trimChars (dropPrefix borrowed "mut ".toList)
    match head == "self".toList || (head.take 5 == "self:".toList) with
    | true => none
    | false =>
      let named :=
        match head.findIdx? (· == ':') with
        | some _ => trimChars (part.takeWhile (· != ':'))
        | none => part
      some (String.mk (trimChars (dropPrefix (trimChars named) "mut ".toList)))

private theorem parameterName_infix (raw : List Char) (n : String)
    (h : parameterName raw = some n) : n.data <:+: raw := by
  unfold parameterName at h
  simp only [] at h
  split at h
  · cases h
  · split at h
    · cases h
    · injection h with h'
      rw [← h']
      have hpart := trimChars_infix raw
      apply List.IsInfix.trans _ hpart
      apply ((trimChars_infix _).trans (dropPrefix_infix _ _)).trans
      apply (trimChars_infix _).trans
      split
      · exact (trimChars_infix _).trans (List.takeWhile_prefix _).isInfix
      · exact List.infix_refl _

private theorem parameterStep_some (names : List String) (raw : List Char) (r : List String)
    (h : parameterStep (some names) raw = some r) :
    r = names ++ (parameterName raw).toList ∧
      ∀ n, parameterName raw = some n → isIdentifier n = true := by
  unfold parameterStep at h
  unfold parameterName
  dsimp only at h ⊢
  generalize trimChars raw = part at h ⊢
  cases hE : part.isEmpty
  · simp only [hE] at h ⊢
    generalize trimChars (dropPrefix (trimChars (trimChars (part.dropWhile (· == '&')))) "mut ".toList)
      = head at h ⊢
    cases hS : (head == "self".toList || (head.take 5 == "self:".toList))
    · simp only [hS] at h ⊢
      generalize (match head.findIdx? (· == ':') with
        | some _ => trimChars (part.takeWhile (· != ':'))
        | none => part) = named at h ⊢
      cases hI : ((trimChars (dropPrefix (trimChars named) "mut ".toList)).isEmpty ||
          !isIdentifier (String.mk (trimChars (dropPrefix (trimChars named) "mut ".toList))))
      · simp only [hI] at h
        injection h with h
        subst h
        simp only [Bool.or_eq_false_iff, Bool.not_eq_false'] at hI
        refine ⟨rfl, ?_⟩
        intro n hn
        injection hn with hn
        subst hn
        exact hI.2
      · simp only [hI] at h
        cases h
    · simp only [hS] at h ⊢
      injection h with h
      subst h
      simp
  · simp only [hE] at h ⊢
    injection h with h
    subst h
    simp

private theorem parameters_fold (parts : List (List Char)) (acc r : List String)
    (h : parts.foldl parameterStep (some acc) = some r) :
    r = acc ++ parts.filterMap parameterName ∧
      ∀ raw ∈ parts, ∀ n, parameterName raw = some n → isIdentifier n = true := by
  induction parts generalizing acc with
  | nil => simp at h; simp [h]
  | cons raw rest ih =>
    rw [List.foldl_cons] at h
    cases hs : parameterStep (some acc) raw with
    | none =>
      rw [hs] at h
      have : ∀ l : List (List Char), l.foldl parameterStep none = none := by
        intro l
        induction l with
        | nil => rfl
        | cons x xs ihx => simpa [parameterStep] using ihx
      rw [this] at h
      cases h
    | some acc' =>
      rw [hs] at h
      obtain ⟨hacc, hid⟩ := parameterStep_some acc raw acc' hs
      obtain ⟨hr, hrest⟩ := ih acc' h
      refine ⟨?_, ?_⟩
      · rw [hr, hacc, List.filterMap_cons]
        cases parameterName raw <;> simp
      · intro x hx n hn
        rcases List.mem_cons.mp hx with hx | hx
        · subst hx; exact hid n hn
        · exact hrest x hx n hn

/-- @proves REQ-DRT-RUST.params_from_source -/
theorem parameters_follow_the_signature (source symbol : String) (names : List String)
    (h : parameters source symbol = some names) :
    declarations (blanked source.toList ['"'] true) symbol ≤ 1 ∧
    ∃ openAt closeAt,
      signatureOpenParen (blanked source.toList ['"'] true) symbol = some openAt ∧
      matching (blanked source.toList ['"'] true) openAt '(' ')' = some closeAt ∧
      names = (splitTopLevel (((blanked source.toList ['"'] true).drop (openAt + 1)).take
        (closeAt - openAt - 1)) ',').filterMap parameterName ∧
      ∀ n ∈ names, isIdentifier n = true ∧ n.data <:+: blanked source.toList ['"'] true := by
  unfold parameters at h
  dsimp only at h
  split at h
  · cases h
  · cases h
  · rename_i openAt hdecl hopen
    split at h
    · cases h
    · rename_i closeAt hclose
      obtain ⟨hnames, hid⟩ := parameters_fold _ [] names h
      simp only [List.nil_append] at hnames
      refine ⟨by simpa using hdecl, openAt, closeAt, hopen, hclose, hnames, ?_⟩
      intro n hn
      rw [hnames, List.mem_filterMap] at hn
      obtain ⟨raw, hraw, hname⟩ := hn
      refine ⟨hid raw hraw n hname, ?_⟩
      exact (parameterName_infix raw n hname).trans ((split_pieces_infix _ _ raw hraw).trans
        ((List.take_prefix _ _).isInfix.trans (List.drop_suffix _ _).isInfix))

private def tsParameterName (raw : List Char) : Option String :=
  let part := trimChars raw
  match part.isEmpty with
  | true => none
  | false =>
    let nameStr := String.mk (trimChars (part.takeWhile (fun c => c != ':' && c != '=')))
    match nameStr == "this" with
    | true => none
    | false => some nameStr

private theorem tsParameterName_infix (raw : List Char) (n : String)
    (h : tsParameterName raw = some n) : n.data <:+: raw := by
  unfold tsParameterName at h
  dsimp only at h
  split at h
  · cases h
  · split at h
    · cases h
    · injection h with h'
      rw [← h']
      exact ((trimChars_infix _).trans (List.takeWhile_prefix _).isInfix).trans (trimChars_infix raw)

private theorem tsParameterStep_some (names : List String) (raw : List Char) (r : List String)
    (h : tsParameterStep (some names) raw = some r) :
    r = names ++ (tsParameterName raw).toList ∧
      ∀ n, tsParameterName raw = some n → isIdentifier n = true := by
  unfold tsParameterStep at h
  unfold tsParameterName
  dsimp only at h ⊢
  generalize trimChars raw = part at h ⊢
  cases hE : part.isEmpty
  · simp only [hE] at h ⊢
    generalize trimChars (part.takeWhile (fun c => c != ':' && c != '=')) = named at h ⊢
    cases hI : (named.isEmpty || !isIdentifier (String.mk named))
    · simp only [Bool.or_eq_false_iff, Bool.not_eq_false'] at hI
      cases hT : (String.mk named == "this")
      · simp only [hI, hT, Bool.not_true, Bool.or_false] at h
        injection h with h
        subst h
        refine ⟨by simp, ?_⟩
        intro n hn
        injection hn with hn
        subst hn
        exact hI.2
      · simp only [hI, hT, Bool.not_true, Bool.or_false] at h
        injection h with h
        subst h
        simp
    · simp only [hI] at h
      cases h
  · simp only [hE] at h ⊢
    injection h with h
    subst h
    simp

private theorem tsParameters_fold (parts : List (List Char)) (acc r : List String)
    (h : parts.foldl tsParameterStep (some acc) = some r) :
    r = acc ++ parts.filterMap tsParameterName ∧
      ∀ raw ∈ parts, ∀ n, tsParameterName raw = some n → isIdentifier n = true := by
  induction parts generalizing acc with
  | nil => simp at h; simp [h]
  | cons raw rest ih =>
    rw [List.foldl_cons] at h
    cases hs : tsParameterStep (some acc) raw with
    | none =>
      rw [hs] at h
      have : ∀ l : List (List Char), l.foldl tsParameterStep none = none := by
        intro l
        induction l with
        | nil => rfl
        | cons x xs ihx => simpa [tsParameterStep] using ihx
      rw [this] at h
      cases h
    | some acc' =>
      rw [hs] at h
      obtain ⟨hacc, hid⟩ := tsParameterStep_some acc raw acc' hs
      obtain ⟨hr, hrest⟩ := ih acc' h
      refine ⟨?_, ?_⟩
      · rw [hr, hacc, List.filterMap_cons]
        cases tsParameterName raw <;> simp
      · intro x hx n hn
        rcases List.mem_cons.mp hx with hx | hx
        · subst hx; exact hid n hn
        · exact hrest x hx n hn

/-- @proves REQ-DRT-TS.params_from_source -/
theorem ts_parameters_follow_the_signature (source symbol : String) (names : List String)
    (h : tsParameters source symbol = some names) :
    tsDeclarations (blanked source.toList tsQuotes false) symbol ≤ 1 ∧
    ∃ openAt closeAt,
      tsSignatureOpenParen (blanked source.toList tsQuotes false) symbol = some openAt ∧
      matching (blanked source.toList tsQuotes false) openAt '(' ')' = some closeAt ∧
      names = (splitTopLevel (((blanked source.toList tsQuotes false).drop (openAt + 1)).take
        (closeAt - openAt - 1)) ',').filterMap tsParameterName ∧
      ∀ n ∈ names, isIdentifier n = true ∧ n.data <:+: blanked source.toList tsQuotes false := by
  unfold tsParameters at h
  dsimp only at h
  split at h
  · cases h
  · cases h
  · rename_i openAt hdecl hopen
    split at h
    · cases h
    · rename_i closeAt hclose
      obtain ⟨hnames, hid⟩ := tsParameters_fold _ [] names h
      simp only [List.nil_append] at hnames
      refine ⟨by simpa using hdecl, openAt, closeAt, hopen, hclose, hnames, ?_⟩
      intro n hn
      rw [hnames, List.mem_filterMap] at hn
      obtain ⟨raw, hraw, hname⟩ := hn
      refine ⟨hid raw hraw n hname, ?_⟩
      exact (tsParameterName_infix raw n hname).trans ((split_pieces_infix _ _ raw hraw).trans
        ((List.take_prefix _ _).isInfix.trans (List.drop_suffix _ _).isInfix))

end TraceLean.Signature
