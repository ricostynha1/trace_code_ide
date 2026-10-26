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

private theorem join_data (l : List String) (acc : String) :
    (l.foldl (fun r s => r ++ s) acc).data = acc.data ++ (l.map String.data).join := by
  induction l generalizing acc with
  | nil => simp
  | cons s rest ih => simp [List.foldl_cons, ih, String.data_append]

private theorem callSite_data (entry : Entry) :
    (callSite entry).data =
      ("        " ++ literal entry.op ++ " => " ++ openBrace ++ "\n").data
        ++ (((entry.parameters.zip entry.sources).map bindLine).map String.data).join
        ++ ("            let out = " ++ entry.call_path ++ "("
          ++ String.intercalate ", " entry.parameters ++ ");\n"
          ++ "            serde_json::to_value(out).map_err(|e| e.to_string())\n"
          ++ "        " ++ closeBrace ++ "\n").data := by
  unfold callSite String.join
  simp only [String.data_append, join_data, List.append_assoc]
  rfl

/-- @proves REQ-DRT-RUST.types_inferred -/
theorem a_call_site_leaves_types_to_inference (entry : Entry) :
    (∀ p ∈ entry.parameters.zip entry.sources,
      ("            let " ++ p.1 ++ " = serde_json::from_str(field(input, " ++ literal p.2 ++ "))").data
        <:+: (callSite entry).data) ∧
    ("            let out = " ++ entry.call_path ++ "(" ++ String.intercalate ", " entry.parameters
      ++ ");\n").data <:+: (callSite entry).data := by
  rw [callSite_data]
  constructor
  · intro p hp
    obtain ⟨s, t, hst⟩ := List.append_of_mem hp
    rw [hst]
    refine ⟨("        " ++ literal entry.op ++ " => " ++ openBrace ++ "\n").data
        ++ ((s.map bindLine).map String.data).join,
      ("\n" ++ "                .map_err(|e| format!(\"argument `" ++ p.1 ++ "`: {e}\"))?;\n").data
        ++ ((t.map bindLine).map String.data).join
        ++ ("            let out = " ++ entry.call_path ++ "("
          ++ String.intercalate ", " entry.parameters ++ ");\n"
          ++ "            serde_json::to_value(out).map_err(|e| e.to_string())\n"
          ++ "        " ++ closeBrace ++ "\n").data, ?_⟩
    simp only [List.map_append, List.map_cons, List.join_append, List.join_cons, bindLine,
      String.data_append, List.append_assoc, List.cons_append, List.nil_append]
  · refine ⟨("        " ++ literal entry.op ++ " => " ++ openBrace ++ "\n").data
        ++ (((entry.parameters.zip entry.sources).map bindLine).map String.data).join,
      ("            serde_json::to_value(out).map_err(|e| e.to_string())\n"
          ++ "        " ++ closeBrace ++ "\n").data, ?_⟩
    simp only [String.data_append, List.append_assoc]

end TraceLean.RustRunner
