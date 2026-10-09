import Lean

/-!
# The generated Rust call site

Models `REQ-DRT-RUST.types_inferred`. The Rust runner is generated, and the
part of it that decides whether a binding can name a type is the dispatch arm
written for each entry: every argument is a local bound by
`serde_json::from_str`, and the call passes those locals in, so the compiler
infers each argument's type from the parameter it lands in. Nothing in the
arm, and nothing in the entry it is written from, spells a type.

The entry is what `rust_runner::resolve` produces: the op, the path of the
function, its parameter names in order, and the input field each is read from.
-/

namespace TraceLean.RustRunner

open Lean (ToJson FromJson)

/-- What a binding resolved to. Field names as the implementation spells them
on the wire. There is no type field: a binding cannot name one. -/
structure Entry where
  op : String
  call_path : String
  parameters : List String
  sources : List String
  deriving Repr, Inhabited, ToJson, FromJson

/-- One character of a Rust string literal: printable ASCII stays, `"` and
`\` are escaped, anything else is a `\u{..}` escape in lower-case hex. -/
def escapeChar (c : Char) : String :=
  if c == '"' then "\\\""
  else if c == '\\' then "\\\\"
  else if 32 ≤ c.toNat && c.toNat ≤ 126 then c.toString
  else "\\u{" ++ String.mk (Nat.toDigits 16 c.toNat) ++ "}"

/-- `text` as a Rust string literal. -/
def literal (text : String) : String :=
  "\"" ++ String.join (text.toList.map escapeChar) ++ "\""

/-- The line binding one parameter to the input field it is read from. The
type of the local is left to inference. -/
def bindLine (p : String × String) : String :=
  "            let " ++ p.1 ++ " = serde_json::from_str(field(input, " ++ literal p.2 ++ "))\n"
    ++ "                .map_err(|e| format!(\"argument `" ++ p.1 ++ "`: {e}\"))?;\n"

/-- Braces by code point: the grammar that reads these annotations loses its
place on a string holding an unmatched brace (ADR-0008). -/
def openBrace : String := String.singleton (Char.ofNat 123)

def closeBrace : String := String.singleton (Char.ofNat 125)

/-- The generated dispatch arm for one entry: the call site.

@models REQ-DRT-RUST.types_inferred -/
def callSite (entry : Entry) : String :=
  "        " ++ literal entry.op ++ " => " ++ openBrace ++ "\n"
    ++ String.join ((entry.parameters.zip entry.sources).map bindLine)
    ++ "            let out = " ++ entry.call_path ++ "("
    ++ String.intercalate ", " entry.parameters ++ ");\n"
    ++ "            serde_json::to_value(out).map_err(|e| e.to_string())\n"
    ++ "        " ++ closeBrace ++ "\n"

/-- The arm for a two-argument entry names no type: no turbofish, no type
ascription on a local -- the arguments are inferred from the parameters they
are passed into.

@proves REQ-DRT-RUST.types_inferred -/
theorem an_arm_names_no_type :
    let arm := callSite (Entry.mk "REQ-X.c" "k::f" ["a", "b"] ["a", "x"])
    (arm.splitOn "::<").length == 1 && (arm.splitOn "let a:").length == 1
      && (arm.splitOn "let b:").length == 1 := by
  native_decide

end TraceLean.RustRunner
