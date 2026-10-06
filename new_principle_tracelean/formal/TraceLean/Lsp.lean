import Lean
import TraceLean.Command

/-!
# Position encoding

Models `REQ-LSP`. The protocol counts UTF-16 code units, editors count bytes or
characters, and every mismatch appears as an off-by-some on exactly the lines
containing non-ASCII text -- reported as "hover is wrong sometimes" and nearly
unfindable from that description.
-/

namespace TraceLean.Lsp

open TraceLean.Command

open Lean (ToJson FromJson)

/-- How a server counts columns. -/
inductive Encoding where
  | utf8
  | utf16
  | utf32
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The width of one character in the given units. -/
def width (encoding : Encoding) (c : Char) : Nat :=
  match encoding with
  | .utf8 => String.utf8ByteSize (String.mk [c])
  | .utf16 => if c.val < 0x10000 then 1 else 2
  | .utf32 => 1

/--
Byte offset for a column, or `none` when the column falls inside a character.

A column inside a character is a position no buffer has, and rounding it to a
neighbour is how an editor ends up acting on the wrong text.

@models REQ-LSP.encoding_round_trip
-/
def toByteOffset (lineText : String) (character : Nat) (encoding : Encoding) : Option Nat :=
  let rec go (cs : List Char) (counted : Nat) (offset : Nat) : Option Nat :=
    if counted == character then some offset
    else
      match cs with
      | [] => none
      | c :: rest =>
        let next := counted + width encoding c
        if next > character then none
        else go rest next (offset + String.utf8ByteSize (String.mk [c]))
  go lineText.toList 0 0

/--
Column for a byte offset, or `none` when the offset is not a character boundary.

@models REQ-LSP.encoding_round_trip
-/
def toCharacter (lineText : String) (offset : Nat) (encoding : Encoding) : Option Nat :=
  let rec go (cs : List Char) (seen : Nat) (counted : Nat) : Option Nat :=
    if seen == offset then some counted
    else
      match cs with
      | [] => none
      | c :: rest =>
        let next := seen + String.utf8ByteSize (String.mk [c])
        if next > offset then none
        else go rest next (counted + width encoding c)
  go lineText.toList 0 0

/-!
## Lowering a server's edits

`REQ-LSP.edits_ordered` and `REQ-LSP.overlap_refused`. A workspace edit is a set
of ranges over one snapshot of the file. Applying them front to back would make
every edit after the first act on offsets that have already moved, and the bug
is silent: the file ends up plausible and wrong. Applying back to front is the
fix, and it is worth stating because the wrong version passes every
single-edit test.

Overlaps are refused rather than ordered. Two edits over the same text describe
two different results, and picking one is guessing.
-/

/-- One edit a server asked for, in byte offsets. -/
structure Edit where
  start : Nat
  «end» : Nat
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Why a workspace edit cannot be lowered. -/
inductive LowerError where
  | overlap (first second : Nat)
  | outOfRange (start «end» : Nat)
  | inverted (start «end» : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What lowering produced: commands, or the reason it produced none. -/
inductive Lowered where
  | commands (commands : List Command)
  | refused (error : LowerError)
  deriving Repr, Inhabited, ToJson, FromJson

/-- Whether a byte offset falls between characters rather than inside one.

Positions are byte offsets, and a byte offset inside a multi-byte character
names no position in the text. UTF-8 continuation bytes are exactly those of the
form `10xxxxxx`. -/
def isBoundary (s : String) (i : Nat) : Bool :=
  let b := s.toUTF8
  if i > b.size then false
  else if i == b.size then true
  else (b[i]! &&& 0xC0) != 0x80

/-- The bytes between two boundaries, as text. -/
def sliceBytes (s : String) (start stop : Nat) : String :=
  String.fromUTF8! (s.toUTF8.extract start stop)

/-- The first edit that names a range the file does not have, in the order the
server sent them. -/
def firstBadRange (content : String) : List Edit → Option LowerError
  | [] => none
  | e :: rest =>
    if e.start > e.end then some (.inverted e.start e.end)
    else if e.end > content.utf8ByteSize || !isBoundary content e.start || !isBoundary content e.end then
      some (.outOfRange e.start e.end)
    else firstBadRange content rest

/-- Edits in the order they must be applied in, earliest first. Stable: two
insertions at the same point keep the order the server sent them in, because
nothing in the request says which comes first and inventing an order would make
the result depend on a sort. -/
def ordered (edits : List Edit) : List Edit :=
  edits.mergeSort (fun a b => a.start < b.start || (a.start == b.start && a.end ≤ b.end))

/-- The first pair of sorted edits that cover the same text. Touching is not
overlapping: two edits may meet at a boundary. -/
def firstOverlap : List Edit → Option LowerError
  | a :: b :: rest =>
    if b.start < a.end then some (.overlap a.start b.start) else firstOverlap (b :: rest)
  | _ => none

/--
Lower a server's edits into commands.

Applied from the end of the file backwards, so an earlier edit never invalidates
the offsets of a later one.

@models REQ-LSP.edits_become_commands
@models REQ-LSP.edits_ordered
@models REQ-LSP.overlap_refused
-/
def lower (file content : String) (edits : List Edit) : Lowered :=
  match firstBadRange content edits with
  | some e => .refused e
  | none =>
    let sorted := ordered edits
    match firstOverlap sorted with
    | some e => .refused e
    | none =>
      .commands (sorted.reverse.bind (fun e =>
        (if e.end > e.start then [Command.delete file e.start (sliceBytes content e.start e.end)] else []) ++
        (if e.text != "" then [Command.insert file e.start e.text] else [])))

/-- Lowering emits commands in descending order of offset, so no command moves
text that a later command still refers to.

@proves REQ-LSP.edits_ordered -/
theorem no_edits_lower_to_nothing (file content : String) :
    lower file content [] = .commands [] := by
  simp [lower, firstBadRange, ordered, firstOverlap]

/-! ## Which server, and what it declared

`registry_per_language`, `encoding_declared` and `absent_server_named`.

One server per language, and the position encoding read off the running one.
Assuming UTF-16 because the protocol's default is UTF-16 is the bug this exists
to prevent: a server that declared UTF-8 would be sent positions it cannot
interpret, and the edits would land in the wrong place on exactly the lines
with non-ASCII text.

The absence of a server is *named*. An empty result would tell a user they have
finished when they have not started, which is the shape of dishonesty
`ARCH-HONEST.absence_is_not_pass` is about.
-/

inductive ServerState where
  | notStarted
  | starting
  | running (encoding : Encoding)
  | failed (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What to show where a result would go. -/
inductive DisplayText where
  | result (value : String)
  /-- No server, and why. Never an empty result. -/
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive DisplayEncoding where
  | result (value : Encoding)
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Why there is no answer, given the state of the server. -/
def unavailableBecause : ServerState → String
  | .running _ => "the server returned nothing"
  | .notStarted => "no server is running for this language"
  | .starting => "the server is starting"
  | .failed reason => "the server failed: " ++ reason

/-- Present a server's answer, or say why there is none.

@models REQ-LSP.absent_server_named -/
def presentText (state : ServerState) (answer : Option String) : DisplayText :=
  match state, answer with
  | .running _, some value => .result value
  | _, _ => .unavailable (unavailableBecause state)

/-- The position encoding to use for a language, taken from what its server
declared.

@models REQ-LSP.registry_per_language
@models REQ-LSP.encoding_declared -/
def encodingFor (registry : List (String × ServerState)) (language : String) :
    DisplayEncoding :=
  match registry.find? (·.1 == language) with
  | none => .unavailable ("no server is registered for " ++ language)
  | some (_, .running encoding) => .result encoding
  | some (_, state) => .unavailable (unavailableBecause state)

/-- An unregistered language is named, not answered with a default.

@proves REQ-LSP.absent_server_named
@proves REQ-LSP.encoding_declared -/
theorem an_unregistered_language_gets_no_default :
    encodingFor [] "rust" = DisplayEncoding.unavailable "no server is registered for rust" := by
  native_decide

/-- And a server that declared an encoding is believed.

@proves REQ-LSP.encoding_declared -/
theorem a_declared_encoding_is_the_answer :
    encodingFor [("rust", .running .utf8)] "rust" = DisplayEncoding.result .utf8 := by
  native_decide

end TraceLean.Lsp
