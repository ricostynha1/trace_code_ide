import Lean

namespace TraceLean.Transcript

open Lean (ToJson FromJson Json)

structure Event where
  kind : String
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Usage where
  model : String
  input : Nat
  cached : Nat
  cacheWrite : Nat
  output : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure TranscriptRead where
  events : List Event
  unrecognised : List String
  held : String
  usage : List Usage
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def stripCR (line : String) : String :=
  if line.endsWith "\r" then line.dropRight 1 else line

def strField (j : Json) (key : String) : Option String :=
  match j.getObjVal? key with
  | .ok v => match v.getStr? with
    | .ok s => some s
    | .error _ => none
  | .error _ => none

def natField (j : Json) (key : String) : Nat :=
  match j.getObjVal? key with
  | .ok v => match v.getNat? with
    | .ok n => n
    | .error _ => 0
  | .error _ => 0

def usageIn (j : Json) : Option Usage :=
  let holder := match j.getObjVal? "message" with
    | .ok inner => inner
    | .error _ => j
  match holder.getObjVal? "usage" with
  | .error _ => none
  | .ok counts =>
  match counts.getObj? with
  | .error _ => none
  | .ok _ =>
    let model := match strField holder "model" with
      | some name => name
      | none => match strField j "model" with
        | some name => name
        | none => "unknown"
    some { model := model,
           input := natField counts "input_tokens",
           cached := natField counts "cache_read_input_tokens",
           cacheWrite := natField counts "cache_creation_input_tokens",
           output := natField counts "output_tokens" }

structure Line where
  event : Option Event
  usage : Option Usage
  claimed : Bool
  deriving Repr, DecidableEq, Inhabited

def classify (line : String) : Line :=
  match Json.parse line with
  | .error _ => { event := none, usage := none, claimed := false }
  | .ok j =>
    let event := match strField j "type", (strField j "text" <|> strField j "content") with
      | some kind, some text => some ({ kind := kind, text := text } : Event)
      | _, _ => none
    { event := event, usage := usageIn j, claimed := (j.getObjVal? "type").toOption.isSome }

/-- @models REQ-TRANSCRIPT.partial_line_held
@models REQ-TRANSCRIPT.unknown_preserved
@models REQ-TRANSCRIPT.absent_is_fine -/
def readTranscript (text : String) : TranscriptRead :=
  let parts := text.splitOn "\n"
  let held := parts.getLast!
  let complete := parts.dropLast
  let lines := (complete.map stripCR).filter (fun l => l.trim != "")
  let step := fun (acc : List Event × List String × List Usage) (line : String) =>
    let read := classify line
    let events := match read.event with
      | some e => acc.1 ++ [e]
      | none => acc.1
    let usage := match read.usage with
      | some u => acc.2.2 ++ [u]
      | none => acc.2.2
    let unknown :=
      if read.event.isNone && (read.claimed || read.usage.isNone) then acc.2.1 ++ [line]
      else acc.2.1
    (events, unknown, usage)
  let result := lines.foldl step ([], [], [])
  { events := result.1, unrecognised := result.2.1, held := held, usage := result.2.2 }

def readChunks (chunks : List String) : TranscriptRead :=
  readTranscript (String.intercalate "\n" chunks)

/-- @proves REQ-TRANSCRIPT.partial_line_held -/
theorem no_newline_is_all_held (text : String) (h : text.splitOn "\n" = [text]) :
    readTranscript text = { events := [], unrecognised := [], held := text, usage := [] } := by
  simp [readTranscript, h, List.getLast!]

def said : String := (Json.mkObj [("type", Json.str "user"), ("text", Json.str "hi")]).compress

end TraceLean.Transcript
