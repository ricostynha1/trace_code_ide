import Lean

/-!
# Reading a Rust function's parameter names

Models `REQ-DRT-RUST.params_from_source`. Rust has no runtime reflection, so a
generated runner cannot bind arguments by name the way a dynamic language can.
Parameter *order* is therefore taken from the declaration and types are left to
inference at the call site.

That makes this small text parser load-bearing in an uncomfortable way: get the
order wrong and the generated runner still compiles, still runs, and compares
the model against the implementation called with its arguments swapped. The
divergence that follows looks like a bug in the code under test.

The two places it can go wrong are both about angle brackets. `fn f<T: Fn(u8) ->
u8>(x: T)` contains a parenthesis inside its generic list and a `>` that is half
of an arrow, and reading either as a delimiter loses the parameter list
entirely.

Modelled over characters. Sources are ASCII, which is what the differential run
generates; the implementation's byte offsets and these character indices
coincide there.
-/

namespace TraceLean.Signature

open Lean (ToJson FromJson)

def isAlphaNum (c : Char) : Bool := c.isAlphanum
def isIdentStart (c : Char) : Bool := c.isAlpha || c == '_'

/-- Whether the whole string is a single identifier. -/
def isIdentifier (s : String) : Bool :=
  match s.toList with
  | [] => false
  | c :: rest => isIdentStart c && rest.all (fun ch => isAlphaNum ch || ch == '_')

/--
Index of the delimiter closing the one at `start`.

A `>` preceded by `-` is the tail of an arrow and not a closing bracket.
-/
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

/-- Index of the `(` opening the parameter list of `fn {symbol}`.

Generic parameters are skipped by balance rather than by searching for the next
`(`, because a generic bound may contain one. -/
private def skipSpaceGo (cs : List Char) : Nat → Nat → Nat
  | 0, i => i
  | fuel + 1, i =>
    if i < cs.length && (cs.getD i 'x').isWhitespace then skipSpaceGo cs fuel (i + 1) else i

private def openParenGo (cs : List Char) (target : List Char) : Nat → Nat → Option Nat
  | 0, _ => none
  | fuel + 1, from_ =>
    -- Written as nested `match`es rather than an `if` chain: the grammar that
    -- reads these annotations cannot follow a run of `let` bindings inside an
    -- `else` (ADR-0008). The conditions and their order are unchanged.
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

/-- Split on `sep` at nesting depth zero, so a comma inside `Vec<A, B>`,
`(A, B)` or `[A; 2]` does not start a new parameter. -/
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
      -- `->` is not a closing angle bracket, so the decrement above is undone.
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

/--
Parameter names of `fn {symbol}` in `source`, in declaration order.

`self` is not a parameter a case can supply, so it is dropped. A pattern
parameter has no single name to bind and is reported as absent rather than
guessed at -- `none` here means "cannot be bound", which is a different answer
from `some []`, a function that takes nothing.

@models REQ-DRT-RUST.params_from_source
@models REQ-DRT-RUST.types_inferred
-/
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

def parameters (source : String) (symbol : String) : Option (List String) :=
  let cs := source.toList
  match signatureOpenParen cs symbol with
  | none => none
  | some openAt =>
    match matching cs openAt '(' ')' with
    | none => none
    | some closeAt =>
      let params := (cs.drop (openAt + 1)).take (closeAt - openAt - 1)
      (splitTopLevel params ',').foldl parameterStep (some [])

/-- Braces by code point: the grammar that reads these annotations treats `{` in
a string as the start of an interpolation and stops there, so the Rust samples
below are built rather than written out (ADR-0008). -/
private def openBrace : String := String.mk [Char.ofNat 123]

private def closeBrace : String := String.mk [Char.ofNat 125]

/-- A function with no parameters answers `some []`, and one that does not exist
answers `none`. The distinction is the point: a missing function must not look
like one that takes nothing.

@proves REQ-DRT-RUST.params_from_source -/
theorem absent_is_not_empty :
    parameters ("pub fn f() " ++ openBrace ++ closeBrace) "f" = some []
    ∧ parameters ("pub fn f() " ++ openBrace ++ closeBrace) "g" = none := by
  native_decide

/-- The arrow inside a generic bound does not close the bound.

@proves REQ-DRT-RUST.params_from_source -/
theorem an_arrow_in_a_bound_is_not_a_closing_bracket :
    parameters
      ("fn apply<F: Fn(u8) -> u8>(f: F, x: u8) -> u8 " ++ openBrace ++ " f(x) " ++ closeBrace)
      "apply" = some ["f", "x"] := by
  native_decide

/-! ## TypeScript

A second implementation language needs the same thing for the same reason: the
order arguments are passed in comes from the implementation's own signature, so
a binding cannot silently reorder them.

The two parsers are separate rather than one parser with a keyword argument.
They read different languages: Rust has `&`, `mut` and `self` and TypeScript has
`?` and defaults, and the shapes only look alike until one of them has to change.
-/

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

/-- One TypeScript parameter's name.

A name, then optionally a type after `:` or a default after `=`. Anything else
-- a destructuring pattern, an optional `name?` -- answers `none`, because a
parameter this cannot name is one the runner cannot supply, and refusing beats
supplying the wrong argument.

@models REQ-DRT-TS.params_from_source -/
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
      match named.isEmpty || !isIdentifier nameStr with
      | true => none
      | false => some (names ++ [nameStr])

/-- The parameters of a TypeScript function, in the order it declares them.

@models REQ-DRT-TS.params_from_source -/
def tsParameters (source : String) (symbol : String) : Option (List String) :=
  let cs := source.toList
  match tsSignatureOpenParen cs symbol with
  | none => none
  | some openAt =>
    match matching cs openAt '(' ')' with
    | none => none
    | some closeAt =>
      let params := (cs.drop (openAt + 1)).take (closeAt - openAt - 1)
      (splitTopLevel params ',').foldl tsParameterStep (some [])

/-- A function with no parameters answers `some []` and an absent one answers
`none`, exactly as on the Rust side: a missing function must not look like one
that takes nothing.

@proves REQ-DRT-TS.params_from_source -/
theorem an_absent_typescript_function_is_not_an_empty_one :
    tsParameters ("export function f() " ++ openBrace ++ closeBrace) "f" = some []
    ∧ tsParameters ("export function f() " ++ openBrace ++ closeBrace) "g" = none := by
  native_decide

/-- Types are read past, and a comma inside one does not start a parameter.

@proves REQ-DRT-TS.params_from_source -/
theorem a_comma_inside_a_type_is_not_a_parameter :
    tsParameters
      ("export function draw(b: Map<string, number>, at: number): string " ++ openBrace ++
        closeBrace)
      "draw" = some ["b", "at"] := by
  native_decide

/-- A parameter this cannot name is refused rather than guessed at.

@proves REQ-DRT-TS.params_from_source -/
theorem a_pattern_parameter_is_refused :
    tsParameters
      ("export function f(a: number, b?: number) " ++ openBrace ++ closeBrace) "f" = none := by
  native_decide

end TraceLean.Signature
