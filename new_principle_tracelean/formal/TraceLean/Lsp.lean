import Lean
import TraceLean.Command

namespace TraceLean.Lsp

open TraceLean.Command

open Lean (ToJson FromJson)

inductive Encoding where
  | utf8
  | utf16
  | utf32
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def width (encoding : Encoding) (c : Char) : Nat :=
  match encoding with
  | .utf8 => String.utf8ByteSize (String.mk [c])
  | .utf16 => if c.val < 0x10000 then 1 else 2
  | .utf32 => 1

/-- @models REQ-LSP.column_to_offset -/
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

/-- @models REQ-LSP.offset_to_column -/
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

structure Edit where
  start : Nat
  «end» : Nat
  text : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive LowerError where
  | overlap (first second : Nat)
  | outOfRange (start «end» : Nat)
  | inverted (start «end» : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Lowered where
  | commands (commands : List Command)
  | refused (error : LowerError)
  deriving Repr, Inhabited, ToJson, FromJson

def isBoundary (s : String) (i : Nat) : Bool :=
  let b := s.toUTF8
  if i > b.size then false
  else if i == b.size then true
  else (b[i]! &&& 0xC0) != 0x80

def sliceBytes (s : String) (start stop : Nat) : String :=
  String.fromUTF8! (s.toUTF8.extract start stop)

def firstBadRange (content : String) : List Edit → Option LowerError
  | [] => none
  | e :: rest =>
    if e.start > e.end then some (.inverted e.start e.end)
    else if e.end > content.utf8ByteSize || !isBoundary content e.start || !isBoundary content e.end then
      some (.outOfRange e.start e.end)
    else firstBadRange content rest

def ordered (edits : List Edit) : List Edit :=
  edits.mergeSort (fun a b => a.start < b.start || (a.start == b.start && a.end ≤ b.end))

def firstOverlap : List Edit → Option LowerError
  | a :: b :: rest =>
    if b.start < a.end then some (.overlap a.start b.start) else firstOverlap (b :: rest)
  | _ => none

/-- @models REQ-LSP.edits_become_commands
@models REQ-LSP.edits_ordered
@models REQ-LSP.overlap_refused -/
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

/-- @proves REQ-LSP.edits_ordered -/
theorem no_edits_lower_to_nothing (file content : String) :
    lower file content [] = .commands [] := by
  simp [lower, firstBadRange, ordered, firstOverlap]

inductive ServerState where
  | notStarted
  | starting
  | running (encoding : Encoding)
  | failed (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive DisplayText where
  | result (value : String)
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive DisplayEncoding where
  | result (value : Encoding)
  | unavailable (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def unavailableBecause : ServerState → String
  | .running _ => "the server returned nothing"
  | .notStarted => "no server is running for this language"
  | .starting => "the server is starting"
  | .failed reason => "the server failed: " ++ reason

/-- @models REQ-LSP.absent_server_named -/
def presentText (state : ServerState) (answer : Option String) : DisplayText :=
  match state, answer with
  | .running _, some value => .result value
  | _, _ => .unavailable (unavailableBecause state)

/-- @models REQ-LSP.registry_per_language
@models REQ-LSP.encoding_declared -/
def encodingFor (registry : List (String × ServerState)) (language : String) :
    DisplayEncoding :=
  match registry.find? (·.1 == language) with
  | none => .unavailable ("no server is registered for " ++ language)
  | some (_, .running encoding) => .result encoding
  | some (_, state) => .unavailable (unavailableBecause state)

/-- @proves REQ-LSP.absent_server_named
@proves REQ-LSP.encoding_declared -/
theorem an_unregistered_language_gets_no_default :
    encodingFor [] "rust" = DisplayEncoding.unavailable "no server is registered for rust" := by
  native_decide

/-- @proves REQ-LSP.encoding_declared -/
theorem a_declared_encoding_is_the_answer :
    encodingFor [("rust", .running .utf8)] "rust" = DisplayEncoding.result .utf8 := by
  native_decide

/-- @proves REQ-LSP.registry_per_language -/
theorem each_language_has_its_own_server :
    let registry := [("rust", ServerState.running .utf8), ("lean", ServerState.running .utf16)]
    encodingFor registry "rust" = .result .utf8 ∧ encodingFor registry "lean" = .result .utf16 ∧
      encodingFor registry "python" = .unavailable "no server is registered for python" := by
  native_decide

def loweredText (lowered : Lowered) : String := (Lean.toJson lowered).compress

def isRefused : Lowered → Bool
  | .refused _ => true
  | .commands _ => false

/-- @proves REQ-LSP.edits_become_commands -/
theorem an_edit_lowers_to_delete_and_insert :
    loweredText (lower "a.rs" "hello" [⟨0, 1, "J"⟩])
      = loweredText (.commands [Command.delete "a.rs" 0 "h", Command.insert "a.rs" 0 "J"]) := by
  native_decide

/-- @proves REQ-LSP.overlap_refused -/
theorem overlapping_edits_are_refused :
    loweredText (lower "a.rs" "hello" [⟨0, 3, "x"⟩, ⟨2, 4, "y"⟩])
      = loweredText (.refused (.overlap 0 2)) ∧
    isRefused (lower "a.rs" "hello" [⟨0, 2, "x"⟩, ⟨2, 4, "y"⟩]) = false := by
  native_decide

end TraceLean.Lsp
