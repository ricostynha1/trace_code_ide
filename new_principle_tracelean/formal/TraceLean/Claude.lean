import Lean
import TraceLean.Transcript

/-!
# Claude Code's transcript

Models `REQ-TRANSCRIPT.tool_format_read`: a transcript in Claude Code's own
format -- JSON lines whose `message.content` is a string or a list of content
blocks -- read into the conversation it records, a row for what was said,
thought, called and returned; a last line without its newline is still being
written and is left for the next read.
-/

namespace TraceLean.Claude

open Lean (Json)
open TraceLean.Transcript

/-- How much of one block a row shows. -/
def shown : Nat := 240

/-- Unicode's white space, as Rust's `char::is_whitespace` has it. -/
def isSpace (c : Char) : Bool :=
  let n := c.toNat
  (0x09 ≤ n && n ≤ 0x0D) || n == 0x20 || n == 0x85 || n == 0xA0 || n == 0x1680 ||
    (0x2000 ≤ n && n ≤ 0x200A) || n == 0x2028 || n == 0x2029 || n == 0x202F || n == 0x205F || n == 0x3000

/-- The text on one line, its runs of white space each one space, cut at
`shown` characters with `…` after a cut. -/
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

/-- What a tool was called with, briefly: the argument a person recognises. -/
def call (name : String) (input : Json) : String :=
  let keys := ["file_path", "path", "command", "pattern", "url", "query", "description", "prompt"]
  match keys.findSome? (str? input) with
  | some arg => name ++ "(" ++ shorten arg ++ ")"
  | none => name ++ "()"

/-- A tool result's text: a string, or a list of blocks' texts joined. -/
def resultText : Json → String
  | .str text => text
  | .arr blocks => " ".intercalate (blocks.toList.filterMap (str? · "text"))
  | _ => ""

/-- Text a harness wrapped around a message rather than something said. -/
def isMachinery (text : String) : Bool :=
  let t := String.mk (text.toList.dropWhile isSpace)
  t.isEmpty || t.startsWith "<command-" || t.startsWith "<local-command" || t.startsWith "<system-reminder>"

def row (kind text : String) : Event := { kind := kind, text := text }

/-- The events of one content block of a record of kind `kind`. -/
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

/-- The events of one record, in the order its blocks appear. -/
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

/-- The lines up to the last newline, as Rust's `lines` reads them. -/
def completeLines (text : String) : List String :=
  let pieces := text.splitOn "\n"
  pieces.dropLast.map fun line => if line.endsWith "\r" then line.dropRight 1 else line

/-- The conversation in a Claude Code transcript, as far as it is written.

@models REQ-TRANSCRIPT.tool_format_read -/
def events (text : String) : List Event :=
  (completeLines text).bind fun line =>
    match Json.parse line with
    | .ok value => record value
    | .error _ => []

/-- A record of Claude Code's: its kind, and its message's content blocks. -/
def recordLine (kind : String) (blocks : List Json) : String :=
  (Json.mkObj [("type", Json.str kind),
    ("message", Json.mkObj [("content", Json.arr blocks.toArray)])]).compress

/-- An assistant turn that thought, spoke and called a tool, and the result
coming back: each block a row, in order. -/
def turn : String :=
  recordLine "assistant"
    [Json.mkObj [("type", Json.str "thinking"), ("thinking", Json.str "look")],
     Json.mkObj [("type", Json.str "text"), ("text", Json.str "Reading it.")],
     Json.mkObj [("type", Json.str "tool_use"), ("name", Json.str "Read"),
       ("input", Json.mkObj [("file_path", Json.str "a.rs")])]] ++ "\n" ++
  recordLine "user"
    [Json.mkObj [("type", Json.str "tool_result"), ("content", Json.str "fn main")]] ++ "\n"

/-- Claude Code's content blocks are read into the conversation, a row for what
was said, thought, called and returned; a last record without its newline is
left for the next read.

@proves REQ-TRANSCRIPT.tool_format_read -/
theorem a_turn_reads_as_its_rows :
    (events turn).map (fun e => (e.kind, e.text))
      = [("thinking", "look"), ("agent", "Reading it."), ("tool", "Read(a.rs)"), ("result", "fn main")] ∧
    events (turn ++ "partial record") = events turn := by
  native_decide

end TraceLean.Claude
