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

theorem foldl_collects {α X : Type} (f : α → String → α) (g : α → List X) (P : String → X → Prop)
    (keeps : ∀ acc line x, x ∈ g acc → x ∈ g (f acc line))
    (adds : ∀ acc line x, P line x → x ∈ g (f acc line)) :
    ∀ (lines : List String) (acc : α) (x : X),
      (x ∈ g acc ∨ ∃ line ∈ lines, P line x) → x ∈ g (lines.foldl f acc)
  | [], acc, x, h => by
    rcases h with h | ⟨_, hl, _⟩
    · exact h
    · simp at hl
  | line :: rest, acc, x, h => by
    apply foldl_collects f g P keeps adds rest (f acc line) x
    rcases h with h | ⟨l, hl, hp⟩
    · exact Or.inl (keeps acc line x h)
    · simp only [List.mem_cons] at hl
      rcases hl with hl | hl
      · subst hl; exact Or.inl (adds acc l x hp)
      · exact Or.inr ⟨l, hl, hp⟩

def completeLines (text : String) : List String :=
  (((text.splitOn "\n").dropLast).map stripCR).filter (fun l => l.trim != "")

/-- @proves REQ-TRANSCRIPT.unknown_preserved -/
theorem unknown_reported_and_read_continues (text line : String)
    (hl : line ∈ completeLines text) :
    ((classify line).event = none →
      ((classify line).claimed = true ∨ (classify line).usage = none) →
      line ∈ (readTranscript text).unrecognised) ∧
    (∀ e, (classify line).event = some e → e ∈ (readTranscript text).events) ∧
    (∀ u, (classify line).usage = some u → u ∈ (readTranscript text).usage) := by
  unfold readTranscript completeLines at *
  simp only
  refine ⟨?_, ?_, ?_⟩
  · intro hev hk
    refine foldl_collects _ (fun (acc : List Event × List String × List Usage) => acc.2.1)
      (fun l x => x = l ∧ (classify l).event = none ∧
        ((classify l).claimed = true ∨ (classify l).usage = none)) ?_ ?_ _ _ _ ?_
    · intro acc l x hx
      simp only
      split <;> simp_all
    · intro acc l x ⟨hx, he, hc⟩
      subst hx
      simp only [he, Option.isNone_none, Bool.true_and]
      rcases hc with hc | hc <;> simp [hc]
    · exact Or.inr ⟨line, hl, rfl, hev, hk⟩
  · intro e he
    refine foldl_collects _ (fun (acc : List Event × List String × List Usage) => acc.1) (fun l x => (classify l).event = some x)
      ?_ ?_ _ _ _ ?_
    · intro acc l x hx
      simp only
      split <;> simp_all
    · intro acc l x hx
      simp [hx]
    · exact Or.inr ⟨line, hl, he⟩
  · intro u hu
    refine foldl_collects _ (fun (acc : List Event × List String × List Usage) => acc.2.2) (fun l x => (classify l).usage = some x)
      ?_ ?_ _ _ _ ?_
    · intro acc l x hx
      simp only
      split <;> simp_all
    · intro acc l x hx
      simp [hx]
    · exact Or.inr ⟨line, hl, hu⟩

/-- @proves REQ-TRANSCRIPT.absent_is_fine -/
theorem no_records_reads_empty (text : String)
    (h : ∀ l ∈ (text.splitOn "\n").dropLast, (stripCR l).trim = "") :
    readTranscript text =
      { events := [], unrecognised := [], held := (text.splitOn "\n").getLast!, usage := [] } := by
  have hnil : (((text.splitOn "\n").dropLast).map stripCR).filter (fun l => l.trim != "") = [] := by
    rw [List.filter_eq_nil]
    intro a ha
    obtain ⟨l, hl, rfl⟩ := List.mem_map.mp ha
    simp [h l hl]
  unfold readTranscript
  simp only [hnil, List.foldl_nil]

end TraceLean.Transcript
