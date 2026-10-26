import Lean
import TraceLean.Transcript

namespace TraceLean.Claude

open Lean (Json)
open TraceLean.Transcript

def shown : Nat := 240

def isSpace (c : Char) : Bool :=
  let n := c.toNat
  (0x09 ≤ n && n ≤ 0x0D) || n == 0x20 || n == 0x85 || n == 0xA0 || n == 0x1680 ||
    (0x2000 ≤ n && n ≤ 0x200A) || n == 0x2028 || n == 0x2029 || n == 0x202F || n == 0x205F || n == 0x3000

def shorten (text : String) : String :=
  let one := " ".intercalate ((text.split isSpace).filter (!·.isEmpty))
  if one.length ≤ shown then one else String.mk (one.toList.take shown) ++ "…"

def str? (j : Json) (key : String) : Option String :=
  match j.getObjVal? key with
  | .ok (.str s) => some s
  | _ => none

def bool? (j : Json) (key : String) : Option Bool :=
  match j.getObjVal? key with
  | .ok (.bool b) => some b
  | _ => none

def call (name : String) (input : Json) : String :=
  let keys := ["file_path", "path", "command", "pattern", "url", "query", "description", "prompt"]
  match keys.findSome? (str? input) with
  | some arg => name ++ "(" ++ shorten arg ++ ")"
  | none => name ++ "()"

def resultText : Json → String
  | .str text => text
  | .arr blocks => " ".intercalate (blocks.toList.filterMap (str? · "text"))
  | _ => ""

def isMachinery (text : String) : Bool :=
  let t := String.mk (text.toList.dropWhile isSpace)
  t.isEmpty || t.startsWith "<command-" || t.startsWith "<local-command" || t.startsWith "<system-reminder>"

def row (kind text : String) : Event := { kind := kind, text := text }

def block (kind : String) (b : Json) : List Event :=
  match str? b "type" with
  | some "text" =>
    let text := (str? b "text").getD ""
    if isMachinery text then [] else [row (if kind == "user" then "you" else "agent") (shorten text)]
  | some "thinking" =>
    let text := (str? b "thinking").getD ""
    if text.isEmpty then [] else [row "thinking" (shorten text)]
  | some "tool_use" =>
    let input :=
      match b.getObjVal? "input" with
      | .ok j => j
      | .error _ => Json.null
    [row "tool" (call ((str? b "name").getD "tool") input)]
  | some "tool_result" =>
    let failed := bool? b "is_error" == some true
    let content :=
      match b.getObjVal? "content" with
      | .ok j => j
      | .error _ => Json.null
    [row (if failed then "failed" else "result") (shorten (resultText content))]
  | _ => []

def record (value : Json) : List Event :=
  match str? value "type", bool? value "isMeta", value.getObjVal? "message" with
  | some kind, meta, .ok message =>
    if meta == some true then []
    else
      match message.getObjVal? "content" with
      | .ok (.str text) =>
        if kind == "user" && !isMachinery text then [row "you" (shorten text)] else []
      | .ok (.arr blocks) =>
        if kind == "user" || kind == "assistant" then blocks.toList.bind (block kind) else []
      | _ => []
  | _, _, _ => []

def completeLines (text : String) : List String :=
  let pieces := text.splitOn "\n"
  pieces.dropLast.map fun line => if line.endsWith "\r" then line.dropRight 1 else line

/-- @models REQ-TRANSCRIPT.tool_format_read -/
def events (text : String) : List Event :=
  (completeLines text).bind fun line =>
    match Json.parse line with
    | .ok value => record value
    | .error _ => []

def recordLine (kind : String) (blocks : List Json) : String :=
  (Json.mkObj [("type", Json.str kind),
    ("message", Json.mkObj [("content", Json.arr blocks.toArray)])]).compress

def turn : String :=
  recordLine "assistant"
    [Json.mkObj [("type", Json.str "thinking"), ("thinking", Json.str "look")],
     Json.mkObj [("type", Json.str "text"), ("text", Json.str "Reading it.")],
     Json.mkObj [("type", Json.str "tool_use"), ("name", Json.str "Read"),
       ("input", Json.mkObj [("file_path", Json.str "a.rs")])]] ++ "\n" ++
  recordLine "user"
    [Json.mkObj [("type", Json.str "tool_result"), ("content", Json.str "fn main")]] ++ "\n"

def rowKinds : List String := ["you", "agent", "thinking", "tool", "result", "failed"]

theorem block_kinds (kind : String) (b : Json) : ∀ e ∈ block kind b, e.kind ∈ rowKinds := by
  intro e h
  unfold block at h
  split at h <;> (try simp only [] at h) <;> (repeat' split at h) <;> simp_all [row, rowKinds]

theorem record_kinds (value : Json) : ∀ e ∈ record value, e.kind ∈ rowKinds := by
  intro e h
  unfold record at h
  split at h
  · split at h
    · simp at h
    · split at h
      · split at h <;> simp_all [row, rowKinds]
      · split at h
        · simp only [List.mem_bind] at h
          obtain ⟨b, _, hb⟩ := h
          exact block_kinds _ b e hb
        · simp at h
      · simp at h
  · simp at h

/-- @proves REQ-TRANSCRIPT.tool_format_read -/
theorem every_row_is_said_thought_called_or_returned (text : String) :
    ∀ e ∈ events text, e.kind ∈ rowKinds := by
  intro e h
  simp only [events, List.mem_bind] at h
  obtain ⟨line, _, hl⟩ := h
  split at hl
  · exact record_kinds _ e hl
  · simp at hl

end TraceLean.Claude
