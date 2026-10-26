import TraceLean.Evidence

namespace TraceLean.Judge

open TraceLean.Evidence

open Lean (ToJson FromJson)

structure Key where
  reqId : String
  clause : Option String
  bond : Bond
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Detail where
  | judge (verdict judgedBy : String) (delegatedBy : Option String) (promptVersion : String)
      (note : Option String)
  | drt (seed cases : Nat) (op : String)
  | proof (theoremName toolchain : String)
  deriving Repr, Inhabited

def Detail.judgeFromJson (p : Lean.Json) : Except String Detail := do
  let verdict ← p.getObjValAs? String "verdict"
  let judgedBy ← p.getObjValAs? String "judgedBy"
  let delegatedBy ← p.getObjValAs? (Option String) "delegatedBy"
  let promptVersion ← p.getObjValAs? String "promptVersion"
  let note ← p.getObjValAs? (Option String) "note"
  pure (Detail.judge verdict judgedBy delegatedBy promptVersion note)

def Detail.drtFromJson (p : Lean.Json) : Except String Detail := do
  let seed ← p.getObjValAs? Nat "seed"
  let cases ← p.getObjValAs? Nat "cases"
  let op ← p.getObjValAs? String "op"
  pure (Detail.drt seed cases op)

def Detail.proofFromJson (p : Lean.Json) : Except String Detail := do
  let theoremName ← p.getObjValAs? String "theoremName"
  let toolchain ← p.getObjValAs? String "toolchain"
  pure (Detail.proof theoremName toolchain)

def Detail.fromJson? (j : Lean.Json) : Except String Detail :=
  match j.getObjVal? "judge", j.getObjVal? "drt", j.getObjVal? "proof" with
  | .ok p, _, _ => Detail.judgeFromJson p
  | _, .ok p, _ => Detail.drtFromJson p
  | _, _, .ok p => Detail.proofFromJson p
  | _, _, _ => .error "no inductive constructor matched"

instance : FromJson Detail := ⟨Detail.fromJson?⟩

def Detail.toJson : Detail → Lean.Json
  | .judge verdict judgedBy delegatedBy promptVersion note =>
    Lean.Json.mkObj [("judge", Lean.Json.mkObj
      ([("verdict", Lean.toJson verdict), ("judgedBy", Lean.toJson judgedBy)] ++
        Lean.Json.opt "delegatedBy" delegatedBy ++
        [("promptVersion", Lean.toJson promptVersion)] ++
        Lean.Json.opt "note" note))]
  | .drt seed cases op =>
    Lean.Json.mkObj [("drt", Lean.Json.mkObj
      [("seed", Lean.toJson seed), ("cases", Lean.toJson cases), ("op", Lean.toJson op)])]
  | .proof theoremName toolchain =>
    Lean.Json.mkObj [("proof", Lean.Json.mkObj
      [("theoremName", Lean.toJson theoremName), ("toolchain", Lean.toJson toolchain)])]

instance : ToJson Detail := ⟨Detail.toJson⟩

/-- @models REQ-EVID.judgement_caps -/
def Detail.ceiling : Detail → Level
  | .judge .. => .L2
  | .drt .. => .L3
  | .proof .. => .L4

structure EvidenceRecord where
  key : Key
  level : Level
  detail : Detail
  linkHash : String
  inputs : List (String × String)
  deriving Repr, Inhabited, ToJson, FromJson

inductive Verdict where
  | agrees
  | drift
  | unmodelable
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Material where
  reqId : String
  clause : Option String
  clauseText : String
  modelSource : String
  requirementHash : String
  modelHash : String
  divergence : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Judgement where
  verdict : Verdict
  judgedBy : String
  delegatedBy : Option String
  note : Option String
  requirementHash : String
  modelHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Person where
  name : String
  delegatesTo : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Judges where
  people : List Person
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Authority where
  | accepted (delegatedBy : Option String)
  | refused (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def isPerson (judges : Judges) (name : String) : Bool :=
  judges.people.any (fun p => p.name == name)

def delegatesTo (judges : Judges) (person delegate : String) : Bool :=
  judges.people.any (fun p => p.name == person && p.delegatesTo.any (fun d => d == delegate))

def authority (judges : Option Judges) (judgedBy : String) (delegatedBy : Option String) :
    Authority :=
  if judgedBy.all Char.isWhitespace then
    .refused "a judgement names who made it, and --by is empty"
  else
    match judges with
    | none => .accepted delegatedBy
    | some js =>
      if isPerson js judgedBy then .accepted none
      else
        match delegatedBy with
        | none =>
          .refused (judgedBy ++ " is not a person in .tracelean/judges.json; name the person who " ++
            "delegates to them with --delegated-by")
        | some person =>
          if delegatesTo js person judgedBy then .accepted (some person)
          else
            .refused (person ++ " does not delegate to " ++ judgedBy ++ "; add " ++ judgedBy ++
              " to the delegatesTo of " ++ person ++ " in .tracelean/judges.json to authorise it")

structure Proposal where
  reqId : String
  clause : Option String
  suggestion : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive JudgeOutcome where
  | recorded (evidence : EvidenceRecord)
  | drifted (evidence : EvidenceRecord)
  | proposed (evidence : EvidenceRecord) (proposal : Proposal)
  | refused (reason : String)
  deriving Repr, Inhabited, ToJson, FromJson

def JudgeOutcome.evidence? : JudgeOutcome → Option EvidenceRecord
  | .recorded e => some e
  | .drifted e => some e
  | .proposed e _ => some e
  | .refused _ => none

def promptVersion : String := "1"

def entry (material : Material) (judgement : Judgement) (delegatedBy : Option String)
    (verdict : String) (level : Level) (linkHash : String) : EvidenceRecord :=
  { key := { reqId := material.reqId, clause := material.clause, bond := .requirementModel },
    level := level,
    detail := .judge verdict judgement.judgedBy delegatedBy promptVersion judgement.note,
    linkHash := linkHash,
    inputs := [("requirement", judgement.requirementHash), ("model", judgement.modelHash)] }

/-- @models REQ-JUDGE.caps_at_judgement
@models REQ-JUDGE.proposal_not_mutation
@models REQ-JUDGE.human_decides
@models REQ-JUDGE.drift_recorded -/
def record (material : Material) (judgement : Judgement) (judges : Option Judges)
    (linkHash : String) : JudgeOutcome :=
  match authority judges judgement.judgedBy judgement.delegatedBy with
  | .refused reason => .refused reason
  | .accepted delegatedBy =>
    match judgement.verdict with
    | .agrees => .recorded (entry material judgement delegatedBy "agrees" .L2 linkHash)
    | .drift => .drifted (entry material judgement delegatedBy "drift" .L1 linkHash)
    | .unmodelable =>
      .proposed (entry material judgement delegatedBy "unmodelable" .L1 linkHash)
        { reqId := material.reqId,
          clause := material.clause,
          suggestion := judgement.note.getD "the clause cannot be modelled as written" }

/-- @models REQ-JUDGE.invalidated_by_change -/
def stillApplies (judgement : Judgement) (material : Material) : Bool :=
  judgement.requirementHash == material.requirementHash &&
  judgement.modelHash == material.modelHash

/-- @models REQ-JUDGE.structural_judged -/
def restsOn (models checks : List String) : Option String :=
  let found := ((if models.isEmpty then checks else models).mergeSort (fun a b => decide (a ≤ b))).eraseDups
  if found.isEmpty then none else some (String.intercalate "+" found)

/-- @proves REQ-JUDGE.caps_at_judgement -/
theorem judgement_never_exceeds_L2
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (e : EvidenceRecord) (h : (record material judgement judges linkHash).evidence? = some e) :
    (e.level = .L1 ∨ e.level = .L2) ∧ e.detail.ceiling = .L2 := by
  simp only [record] at h
  split at h
  all_goals try split at h
  all_goals simp [JudgeOutcome.evidence?] at h
  all_goals subst h
  all_goals simp [entry, Detail.ceiling]

/-- @proves REQ-JUDGE.drift_recorded
@proves REQ-JUDGE.proposal_not_mutation -/
theorem drift_is_recorded_at_L1
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (delegatedBy : Option String) (h : judgement.verdict = .drift)
    (ok : authority judges judgement.judgedBy judgement.delegatedBy = .accepted delegatedBy) :
    record material judgement judges linkHash =
      .drifted (entry material judgement delegatedBy "drift" .L1 linkHash) := by
  simp [record, h, ok]

/-- @proves REQ-JUDGE.human_decides -/
theorem a_blank_judge_records_nothing
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (h : judgement.judgedBy.all Char.isWhitespace = true) :
    (record material judgement judges linkHash).evidence? = none := by
  simp [record, authority, h, JudgeOutcome.evidence?]

structure Judged where
  verdict : String
  judgedBy : String
  delegatedBy : Option String
  note : Option String
  level : Level
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def levelName : Level → String
  | .L1 => "L1"
  | .L2 => "L2"
  | .L3 => "L3"
  | .L4 => "L4"

def delegatedText : Option String → String
  | some person => " (delegated by " ++ person ++ ")"
  | none => ""

/-- @models REQ-JUDGE.judgement_shown -/
def judgedText (judged : Judged) : String :=
  if judged.verdict == "agrees" then
    "agrees by " ++ judged.judgedBy ++ " — " ++ levelName judged.level ++
      delegatedText judged.delegatedBy
  else
    "judged: " ++ judged.verdict ++ " — " ++ judged.note.getD "no note" ++ "  by " ++
      judged.judgedBy ++ delegatedText judged.delegatedBy

/-- @models REQ-JUDGE.prompt_exported
@models REQ-JUDGE.divergence_presented
@models REQ-JUDGE.advice_is_not_evidence -/
def prompt (material : Material) : String :=
  let clause :=
    match material.clause with
    | some c => material.reqId ++ "." ++ c
    | none => material.reqId
  let divergence :=
    match material.divergence with
    | some text =>
      "\nDifferential testing found this disagreement between the model and the " ++
      "implementation:\n\n" ++ text ++ "\n"
    | none => ""
  "Does this formal model say what this requirement clause says?\n\n" ++
  "Requirement " ++ clause ++ ":\n\n" ++ material.clauseText.trim ++
  "\n\nModel:\n\n```lean\n" ++ material.modelSource.trim ++ "\n```\n" ++ divergence ++
  "\nAnswer with one of: agrees, drift, unmodelable — and, if drift, the concrete " ++
  "behaviour on which the two differ.\n\n" ++
  "(Prompt version " ++ promptVersion ++ ". This is advice. The decision is recorded as " ++
  "the judgement of the person who asked.)\n"

/-- @proves REQ-EVID.judgement_caps -/
theorem a_judgement_caps_at_the_second_level (verdict judgedBy promptVersion : String)
    (delegatedBy note : Option String) (seed cases : Nat) (op theoremName toolchain : String) :
    (Detail.judge verdict judgedBy delegatedBy promptVersion note).ceiling = Level.L2 ∧
    (Detail.drt seed cases op).ceiling = Level.L3 ∧
    (Detail.proof theoremName toolchain).ceiling = Level.L4 := by
  simp [Detail.ceiling]

theorem infixSelf (x : String) : ∃ p q, x = p ++ x ++ q := ⟨"", "", by simp⟩

theorem infixLeft (a : String) {s x : String} (h : ∃ p q, s = p ++ x ++ q) :
    ∃ p q, a ++ s = p ++ x ++ q := by
  obtain ⟨p, q, h⟩ := h
  exact ⟨a ++ p, q, by rw [h]; simp only [String.append_assoc]⟩

theorem infixRight (b : String) {s x : String} (h : ∃ p q, s = p ++ x ++ q) :
    ∃ p q, s ++ b = p ++ x ++ q := by
  obtain ⟨p, q, h⟩ := h
  exact ⟨p, q ++ b, by rw [h]; simp only [String.append_assoc]⟩

syntax "infix_tac" : tactic

macro_rules
  | `(tactic| infix_tac) =>
    `(tactic| first
      | with_reducible exact infixSelf _
      | (apply infixLeft; infix_tac)
      | (apply infixRight; infix_tac))

/-- @proves REQ-JUDGE.prompt_exported -/
theorem prompt_carries_clause_and_model (material : Material) :
    (∃ pre post, prompt material = pre ++ material.clauseText.trim ++ post) ∧
    (∃ pre post, prompt material = pre ++ material.modelSource.trim ++ post) := by
  unfold prompt
  exact ⟨by infix_tac, by infix_tac⟩

/-- @proves REQ-JUDGE.divergence_presented -/
theorem prompt_presents_divergence (material : Material) (text : String)
    (h : material.divergence = some text) :
    ∃ pre post, prompt material = pre ++ text ++ post := by
  unfold prompt
  rw [h]
  dsimp only
  infix_tac

/-- @proves REQ-JUDGE.advice_is_not_evidence -/
theorem advice_enters_only_as_the_human_decision (material : Material) (judgement : Judgement)
    (judges : Option Judges) (linkHash : String) :
    (∃ pre post, prompt material = pre ++ ". This is advice. The decision is recorded as " ++ post) ∧
    ∀ e, (record material judgement judges linkHash).evidence? = some e →
      ∃ verdict delegatedBy,
        e.detail = .judge verdict judgement.judgedBy delegatedBy promptVersion judgement.note := by
  refine ⟨?_, ?_⟩
  · unfold prompt
    infix_tac
  · intro e h
    simp only [record] at h
    split at h
    all_goals try split at h
    all_goals simp [JudgeOutcome.evidence?] at h
    all_goals subst h
    all_goals exact ⟨_, _, rfl⟩

/-- @proves REQ-JUDGE.judgement_shown -/
theorem judgement_is_shown_with_its_note_and_delegator (judged : Judged) :
    (∃ pre post, judgedText judged = pre ++ judged.judgedBy ++ post) ∧
    (∀ person, judged.delegatedBy = some person →
      ∃ pre post, judgedText judged = pre ++ (" (delegated by " ++ person ++ ")") ++ post) ∧
    (judged.verdict ≠ "agrees" →
      (∃ pre post, judgedText judged = pre ++ judged.verdict ++ post) ∧
      (∃ pre post, judgedText judged = pre ++ judged.note.getD "no note" ++ post)) := by
  refine ⟨?_, ?_, ?_⟩
  · unfold judgedText
    split <;> infix_tac
  · intro person hp
    have hd : ∃ pre post, delegatedText judged.delegatedBy =
        pre ++ (" (delegated by " ++ person ++ ")") ++ post := by
      rw [hp]
      exact ⟨"", "", by simp [delegatedText]⟩
    unfold judgedText
    split
    · exact infixLeft _ hd
    · exact infixLeft _ hd
  · intro hv
    unfold judgedText
    have : (judged.verdict == "agrees") = false := by simp [hv]
    rw [if_neg (by simp [this])]
    exact ⟨by infix_tac, by infix_tac⟩

theorem eraseDups_loop_length (as bs : List String) :
    bs.length ≤ (List.eraseDups.loop as bs).length := by
  induction as generalizing bs with
  | nil => simp [List.eraseDups.loop]
  | cons a rest ih =>
    simp only [List.eraseDups.loop]
    split
    · exact ih bs
    · exact Nat.le_trans (Nat.le_succ _) (ih (a :: bs))

theorem eraseDups_ne_nil (xs : List String) (h : xs ≠ []) : xs.eraseDups ≠ [] := by
  cases xs with
  | nil => exact absurd rfl h
  | cons a rest =>
    intro he
    have := eraseDups_loop_length rest [a]
    simp only [List.eraseDups, List.eraseDups.loop, List.elem_nil] at he
    rw [he] at this
    simp at this

/-- @proves REQ-JUDGE.structural_judged -/
theorem unmodelled_clause_rests_on_its_checks (models checks checks' : List String) :
    (models ≠ [] → restsOn models checks = restsOn models checks') ∧
    (restsOn [] checks = none ↔ checks = []) ∧
    (checks ≠ [] → restsOn [] checks =
      some (String.intercalate "+" ((checks.mergeSort (fun a b => decide (a ≤ b))).eraseDups))) := by
  refine ⟨?_, ?_, ?_⟩
  · intro hm
    have : models.isEmpty = false := by cases models <;> simp_all
    simp [restsOn, this]
  · constructor
    · intro h
      cases checks with
      | nil => rfl
      | cons c rest =>
        exfalso
        have hs : (c :: rest).mergeSort (fun a b => decide (a ≤ b)) ≠ [] := by
          intro he
          have := (List.mergeSort_perm (fun a b => decide (a ≤ b)) (c :: rest)).length_eq
          rw [he] at this
          simp at this
        have := eraseDups_ne_nil _ hs
        simp only [restsOn, List.isEmpty_nil, if_true] at h
        split at h
        · rename_i he
          exact this (List.isEmpty_iff.mp he)
        · cases h
    · intro h
      subst h
      simp [restsOn, List.mergeSort, List.eraseDups, List.eraseDups.loop]
  · intro hc
    have hs : checks.mergeSort (fun a b => decide (a ≤ b)) ≠ [] := by
      intro he
      have := (List.mergeSort_perm (fun a b => decide (a ≤ b)) checks).length_eq
      rw [he] at this
      cases checks <;> simp_all
    have := eraseDups_ne_nil _ hs
    have he : ((checks.mergeSort (fun a b => decide (a ≤ b))).eraseDups).isEmpty = false := by
      cases h : (checks.mergeSort (fun a b => decide (a ≤ b))).eraseDups <;> simp_all
    simp only [restsOn, List.isEmpty_nil, if_true]
    rw [if_neg (by simp [he])]

end TraceLean.Judge
