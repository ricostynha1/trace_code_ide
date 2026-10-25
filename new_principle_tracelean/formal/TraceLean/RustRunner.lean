import Lean

namespace TraceLean.RustRunner

open Lean (ToJson FromJson)

structure Entry where
  op : String
  call_path : String
  parameters : List String
  sources : List String
  deriving Repr, Inhabited, ToJson, FromJson

def escapeChar (c : Char) : String :=
  if c == '"' then "\\\""
  else if c == '\\' then "\\\\"
  else if 32 ≤ c.toNat && c.toNat ≤ 126 then c.toString
  else "\\u{" ++ String.mk (Nat.toDigits 16 c.toNat) ++ "}"

def literal (text : String) : String :=
  "\"" ++ String.join (text.toList.map escapeChar) ++ "\""

def bindLine (p : String × String) : String :=
  "            let " ++ p.1 ++ " = serde_json::from_str(field(input, " ++ literal p.2 ++ "))\n"
    ++ "                .map_err(|e| format!(\"argument `" ++ p.1 ++ "`: {e}\"))?;\n"

def openBrace : String := String.singleton (Char.ofNat 123)

def closeBrace : String := String.singleton (Char.ofNat 125)

/-- @models REQ-DRT-RUST.types_inferred -/
def callSite (entry : Entry) : String :=
  "        " ++ literal entry.op ++ " => " ++ openBrace ++ "\n"
    ++ String.join ((entry.parameters.zip entry.sources).map bindLine)
    ++ "            let out = " ++ entry.call_path ++ "("
    ++ String.intercalate ", " entry.parameters ++ ");\n"
    ++ "            serde_json::to_value(out).map_err(|e| e.to_string())\n"
    ++ "        " ++ closeBrace ++ "\n"

end TraceLean.RustRunner
