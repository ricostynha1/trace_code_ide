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

theorem firstBadRange_none_ordered (content : String) :
    ∀ (edits : List Edit), firstBadRange content edits = none → ∀ e ∈ edits, e.start ≤ e.end
  | [], _, e, he => by simp at he
  | x :: rest, h, e, he => by
    simp only [firstBadRange] at h
    split at h
    · simp at h
    · split at h
      · simp at h
      · rcases List.mem_cons.mp he with rfl | he
        · omega
        · exact firstBadRange_none_ordered content rest h e he

theorem firstOverlap_none_disjoint :
    ∀ (l : List Edit), (∀ e ∈ l, e.start ≤ e.end) → firstOverlap l = none →
      l.Pairwise (fun a b => a.end ≤ b.start)
  | [], _, _ => List.Pairwise.nil
  | [a], _, _ => by simp
  | a :: b :: rest, hle, h => by
    simp only [firstOverlap] at h
    split at h
    · simp at h
    · have hab : a.end ≤ b.start := by omega
      have tail := firstOverlap_none_disjoint (b :: rest)
        (fun e he => hle e (List.mem_cons_of_mem _ he)) h
      refine List.Pairwise.cons ?_ tail
      intro c hc
      rcases List.mem_cons.mp hc with rfl | hc
      · exact hab
      · have hb := hle b (by simp)
        have hbc := (List.pairwise_cons.mp tail).1 c hc
        omega

/-- @proves REQ-LSP.overlap_refused -/
theorem lowered_edits_never_overlap (file content : String) (edits : List Edit)
    (commands : List Command) (h : lower file content edits = .commands commands) :
    edits.Pairwise (fun a b => a.end ≤ b.start ∨ b.end ≤ a.start) := by
  simp only [lower] at h
  split at h
  · cases h
  · rename_i hbad
    split at h
    · cases h
    · rename_i hov
      have hle := firstBadRange_none_ordered content edits hbad
      have perm := List.mergeSort_perm
        (fun a b => decide (a.start < b.start) || (a.start == b.start && decide (a.end ≤ b.end))) edits
      have hle' : ∀ e ∈ ordered edits, e.start ≤ e.end :=
        fun e he => hle e (perm.mem_iff.mp he)
      have disj := firstOverlap_none_disjoint (ordered edits) hle' hov
      have disj' : (ordered edits).Pairwise
          (fun a b => a.end ≤ b.start ∨ b.end ≤ a.start) :=
        disj.imp (fun h => Or.inl h)
      exact disj'.perm perm (fun h => h.symm)

/-- @proves REQ-LSP.edits_become_commands -/
theorem valid_edits_lower_to_commands (file content : String) (edits : List Edit)
    (hbad : firstBadRange content edits = none) (hov : firstOverlap (ordered edits) = none) :
    ∃ commands, lower file content edits = .commands commands ∧
      ∀ e ∈ edits, e.text ≠ "" → Command.insert file e.start e.text ∈ commands := by
  refine ⟨(ordered edits).reverse.bind (fun e =>
        (if e.end > e.start then [Command.delete file e.start (sliceBytes content e.start e.end)] else []) ++
        (if e.text != "" then [Command.insert file e.start e.text] else [])), by simp [lower, hbad, hov], ?_⟩
  intro e he hne
  have perm := List.mergeSort_perm
    (fun a b => decide (a.start < b.start) || (a.start == b.start && decide (a.end ≤ b.end))) edits
  have he' : e ∈ (ordered edits).reverse := List.mem_reverse.mpr (perm.mem_iff.mpr he)
  refine List.mem_bind.mpr ⟨e, he', ?_⟩
  simp [hne]

/-- @proves REQ-LSP.absent_server_named -/
theorem absent_server_is_named (state : ServerState) (answer : Option String)
    (h : ∀ encoding, state ≠ .running encoding) :
    presentText state answer = .unavailable (unavailableBecause state) ∧
      (state = .notStarted →
        presentText state answer = .unavailable "no server is running for this language") := by
  have : presentText state answer = .unavailable (unavailableBecause state) := by
    cases state with
    | running e => exact absurd rfl (h e)
    | _ => rfl
  refine ⟨this, ?_⟩
  intro hs
  subst hs
  rfl

/-- @proves REQ-LSP.registry_per_language -/
theorem each_language_has_its_own_server (registry : List (String × ServerState))
    (language other : String) (state : ServerState) (encoding : Encoding) :
    (other ≠ language → encodingFor ((other, state) :: registry) language = encodingFor registry language) ∧
    encodingFor ((language, .running encoding) :: registry) language = .result encoding := by
  constructor
  · intro hne
    have : (other == language) = false := by simp [hne]
    simp [encodingFor, List.find?, this]
  · simp [encodingFor, List.find?]

/-- @proves REQ-LSP.encoding_declared -/
theorem encoding_is_the_declared_one (registry : List (String × ServerState))
    (language : String) (encoding : Encoding) :
    encodingFor registry language = .result encoding ↔
      ∃ name, registry.find? (·.1 == language) = some (name, .running encoding) := by
  unfold encodingFor
  split
  · rename_i hf
    simp [hf]
  · rename_i name e hf
    simp [hf]
  · rename_i name st hst hf
    simp only [hf, Option.some.injEq, Prod.mk.injEq]
    constructor
    · intro h; cases h
    · rintro ⟨_, _, hs⟩
      exact absurd hs (hst _)

end TraceLean.Lsp
