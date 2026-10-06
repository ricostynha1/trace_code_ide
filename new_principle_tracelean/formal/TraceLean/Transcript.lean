import Lean

/-!
# Reading an external tool's own session record

Models `REQ-TRANSCRIPT`. A transcript is appended to while it is being read, so
the last record is routinely half-written. Parsing it would produce an error
exactly when the tool is most active; discarding it would lose the record. It is
held instead, and the next read completes it.

The other half is that an unrecognised record is kept rather than dropped. A
reader that discarded what it did not understand would silently lose everything
about a tool it had not been taught, and would look like it was working.
-/

namespace TraceLean.Transcript

open Lean (ToJson FromJson Json)

/-- One thing the tool reported doing. -/
structure Event where
  kind : String
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What one exchange used, as the tool reported it.

Here rather than with the pricing, because it is a thing a transcript says. What
it costs is a separate question answered against a table (`REQ-COST`). -/
structure Usage where
  model : String
  input : Nat
  cached : Nat
  cacheWrite : Nat
  output : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What a read of a transcript yielded. -/
structure TranscriptRead where
  events : List Event
  /-- Records whose shape was not recognised, kept rather than discarded. -/
  unrecognised : List String
  /-- Bytes not yet consumed, because the last record is still being written. -/
  held : String
  /-- What the tool said it used, where it said so. -/
  usage : List Usage
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A line as a line-oriented reader sees it: without the carriage return that a
Windows-written file leaves before the newline. -/
def stripCR (line : String) : String :=
  if line.endsWith "\r" then line.dropRight 1 else line

/-- A string field, or nothing if the key is absent or holds something else. -/
def strField (j : Json) (key : String) : Option String :=
  match j.getObjVal? key with
  | .ok v => match v.getStr? with
    | .ok s => some s
    | .error _ => none
  | .error _ => none

/-- A natural field, or zero. A count a tool did not report is a count of none,
and refusing the whole record over one absent field would lose the rest of it. -/
def natField (j : Json) (key : String) : Nat :=
  match j.getObjVal? key with
  | .ok v => match v.getNat? with
    | .ok n => n
    | .error _ => 0
  | .error _ => 0

/-- The usage a record reports, if it reports any.

Two shapes, because tools write both: the counts at the top of the record, or
one level down under `message`, which is where a tool that wraps an API response
puts them. The names are the ones the API itself uses; renaming them here would
be this project inventing a vocabulary and then failing to recognise the real
one. -/
def usageIn (j : Json) : Option Usage :=
  let holder := match j.getObjVal? "message" with
    | .ok inner => inner
    | .error _ => j
  match holder.getObjVal? "usage" with
  | .error _ => none
  | .ok counts =>
  -- The key existing is not enough: `{"usage": 123}` is not a usage record, and
  -- reading it as one invented a model named `unknown` that the estimate then
  -- reported as unpriced. A count nobody spent, announced as a gap in the price
  -- table.
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

/-- What one complete line yielded: an event, some usage, both, or neither --
and whether the record claimed to be saying something in the first place. -/
structure Line where
  event : Option Event
  usage : Option Usage
  /-- The record carries a `type`, so it claims to be a thing with a kind.

  A record that claims that and yields no event is one whose shape this reader
  does not know -- the common case being text written as an array of content
  blocks rather than as a string. Without this flag such a record was silently
  dropped whenever it also carried usage, which is `unknown_preserved` failing
  in the one place it most matters. -/
  claimed : Bool
  deriving Repr, DecidableEq, Inhabited

/-- Classify one complete line.

A record that reports usage is recognised even when it carries no text, which is
the common case: a tool's accounting records say what was spent and nothing a
person reads. Before this they went to `unrecognised`, which was true when
nothing here knew what usage was and is misleading now. -/
def classify (line : String) : Line :=
  match Json.parse line with
  | .error _ => { event := none, usage := none, claimed := false }
  | .ok j =>
    let event := match strField j "type", (strField j "text" <|> strField j "content") with
      | some kind, some text => some ({ kind := kind, text := text } : Event)
      | _, _ => none
    { event := event, usage := usageIn j, claimed := (j.getObjVal? "type").toOption.isSome }

/--
Parse as much of a transcript as is complete.

Only what precedes the final newline is complete. Everything after it is held,
*whether or not it happens to parse*: a record can be valid JSON and still be
half of what the tool intends to write, and a reader that took the parse as
proof of completeness would consume the first half of a record and then the
second half as a separate one.

@models REQ-TRANSCRIPT.partial_line_held
@models REQ-TRANSCRIPT.unknown_preserved
@models REQ-TRANSCRIPT.absent_is_fine
-/
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
    -- Unrecognised when nothing was got out of it, *or* when it said it had a
    -- kind and the reader could not read what it said. A record can be
    -- recognised in one part and not in another, and reporting only the wholly
    -- unrecognised ones loses exactly the records a reader has not been taught.
    let unknown :=
      if read.event.isNone && (read.claimed || read.usage.isNone) then acc.2.1 ++ [line]
      else acc.2.1
    (events, unknown, usage)
  let result := lines.foldl step ([], [], [])
  { events := result.1, unrecognised := result.2.1, held := held, usage := result.2.2 }

/-- `readTranscript`, over the fragments a transcript was written in.

Exists so that a generator can produce the shape that matters -- a record cut
off mid-write -- rather than only flat strings.

@models REQ-TRANSCRIPT.partial_line_held -/
def readChunks (chunks : List String) : TranscriptRead :=
  readTranscript (String.intercalate "\n" chunks)

/-- A transcript with no newline in it is entirely held: nothing is complete.

@proves REQ-TRANSCRIPT.partial_line_held -/
theorem no_newline_is_all_held (text : String) (h : text.splitOn "\n" = [text]) :
    readTranscript text = { events := [], unrecognised := [], held := text, usage := [] } := by
  simp [readTranscript, h, List.getLast!]

/-- Nothing at all is a valid transcript.

@proves REQ-TRANSCRIPT.absent_is_fine -/
theorem empty_is_empty :
    readTranscript "" = { events := [], unrecognised := [], held := "", usage := [] } := by
  native_decide

end TraceLean.Transcript
