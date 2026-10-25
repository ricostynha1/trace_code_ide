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

/-- @proves REQ-DRT-RUST.params_from_source -/
theorem absent_is_not_empty :
    parameters ("pub fn f() " ++ openBrace ++ closeBrace) "f" = some []
    ∧ parameters ("pub fn f() " ++ openBrace ++ closeBrace) "g" = none := by
  native_decide

/-- @proves REQ-DRT-RUST.params_from_source -/
theorem an_arrow_in_a_bound_is_not_a_closing_bracket :
    parameters
      ("fn apply<F: Fn(u8) -> u8>(f: F, x: u8) -> u8 " ++ openBrace ++ " f(x) " ++ closeBrace)
      "apply" = some ["f", "x"] := by
  native_decide

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

/-- @proves REQ-DRT-TS.params_from_source -/
theorem an_absent_typescript_function_is_not_an_empty_one :
    tsParameters ("export function f() " ++ openBrace ++ closeBrace) "f" = some []
    ∧ tsParameters ("export function f() " ++ openBrace ++ closeBrace) "g" = none := by
  native_decide

/-- @proves REQ-DRT-TS.params_from_source -/
theorem a_comma_inside_a_type_is_not_a_parameter :
    tsParameters
      ("export function draw(b: Map<string, number>, at: number): string " ++ openBrace ++
        closeBrace)
      "draw" = some ["b", "at"] := by
  native_decide

/-- @proves REQ-DRT-TS.params_from_source -/
theorem a_pattern_parameter_is_refused :
    tsParameters
      ("export function f(a: number, b?: number) " ++ openBrace ++ closeBrace) "f" = none := by
  native_decide

/-- @proves REQ-DRT-TS.params_from_source -/
theorem a_comment_this_and_a_second_declaration_do_not_reorder :
    tsParameters "// was: function f(b, a)\nexport function f(a: number, b: number)" "f"
      = some ["a", "b"]
    ∧ tsParameters "/* function f(b, a) */ export function f(this: Window, a, b)" "f"
      = some ["a", "b"]
    ∧ tsParameters "function f(a)\nexport function f(a, b)" "f" = none := by
  native_decide

/-- @proves REQ-DRT-RUST.params_from_source -/
theorem a_commented_rust_signature_does_not_win :
    parameters "// fn f(b: u8, a: u8)\nfn f(a: u8, b: u8)" "f" = some ["a", "b"]
    ∧ parameters "fn f(a: u8)\nfn f(a: u8, b: u8)" "f" = none := by
  native_decide

end TraceLean.Signature
