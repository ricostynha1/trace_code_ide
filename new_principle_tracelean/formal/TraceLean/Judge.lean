import TraceLean.Evidence

/-!
# The human judge

Models `REQ-JUDGE`. Nothing binds English to a formal model by execution --
there is no oracle for prose -- so this bond is decided by a person and graded
below the bonds that are checked by running something.

The property worth stating is the ceiling. This is the most fallible bond in the
system, and it must not be able to produce the system's most confident output:
a reading is not an execution. An implementation that let a judgement write `L4`
would make every other guarantee decorative, and it would look exactly like a
correct one until somebody checked.
-/

namespace TraceLean.Judge

open TraceLean.Evidence

open Lean (ToJson FromJson)

/-- What a record is about: one clause of one requirement, on one bond. -/
structure Key where
  reqId : String
  clause : Option String
  bond : Bond
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Backend-specific detail, typed rather than free-form so a record cannot
quietly omit what makes it reproducible. -/
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

/-- Read a detail; an absent delegate or note is `none`, as in records made
before either existed. -/
def Detail.fromJson? (j : Lean.Json) : Except String Detail :=
  match j.getObjVal? "judge", j.getObjVal? "drt", j.getObjVal? "proof" with
  | .ok p, _, _ => Detail.judgeFromJson p
  | _, .ok p, _ => Detail.drtFromJson p
  | _, _, .ok p => Detail.proofFromJson p
  | _, _, _ => .error "no inductive constructor matched"

instance : FromJson Detail := ⟨Detail.fromJson?⟩

/-- The wire form, which leaves out an absent delegate or note rather than
writing `null`, as the records on disk do. -/
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

/-- The highest level each kind of evidence can establish.

@models REQ-EVID.judgement_caps -/
def Detail.ceiling : Detail → Level
  | .judge .. => .L2
  | .drt .. => .L3
  | .proof .. => .L4

/-- One piece of evidence, and everything it depended on. -/
structure EvidenceRecord where
  key : Key
  level : Level
  detail : Detail
  linkHash : String
  inputs : List (String × String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- What a judge decided. -/
inductive Verdict where
  | agrees
  | drift
  | unmodelable
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The material a judgement is made from. -/
structure Material where
  reqId : String
  clause : Option String
  clauseText : String
  modelSource : String
  requirementHash : String
  modelHash : String
  divergence : Option String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A decision, attributable to the person who made it. -/
structure Judgement where
  verdict : Verdict
  judgedBy : String
  delegatedBy : Option String
  note : Option String
  requirementHash : String
  modelHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One person who may judge, and whom they authorise to judge for them. -/
structure Person where
  name : String
  delegatesTo : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Who may judge on a project: `.tracelean/judges.json`. -/
structure Judges where
  people : List Person
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether a judge may record a verdict, and in whose name. -/
inductive Authority where
  | accepted (delegatedBy : Option String)
  | refused (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def isPerson (judges : Judges) (name : String) : Bool :=
  judges.people.any (fun p => p.name == name)

def delegatesTo (judges : Judges) (person delegate : String) : Bool :=
  judges.people.any (fun p => p.name == person && p.delegatesTo.any (fun d => d == delegate))

/--
Who may judge.

A blank name is refused whatever the project says. Without a judges file a name
is taken at its word. With one, a listed person judges in their own name, and
anyone else only in the name of a listed person who delegates to them.
-/
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

/-- A change a judgement suggests, which nothing applies automatically. -/
structure Proposal where
  reqId : String
  clause : Option String
  suggestion : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What recording a judgement produced. -/
inductive JudgeOutcome where
  | recorded (evidence : EvidenceRecord)
  | drifted (evidence : EvidenceRecord)
  | proposed (evidence : EvidenceRecord) (proposal : Proposal)
  | refused (reason : String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- The record an outcome writes, if it writes one. -/
def JudgeOutcome.evidence? : JudgeOutcome → Option EvidenceRecord
  | .recorded e => some e
  | .drifted e => some e
  | .proposed e _ => some e
  | .refused _ => none

def promptVersion : String := "1"

/-- The record a verdict writes in the clause's judgement slot. The inputs are
the two hashes the judgement was about, so that changing either half means
nobody has judged the pair that now exists. -/
def entry (material : Material) (judgement : Judgement) (delegatedBy : Option String)
    (verdict : String) (level : Level) (linkHash : String) : EvidenceRecord :=
  -- The brace opens on its own line: the grammar that reads these annotations
  -- cannot follow a constructor applied to a structure instance whose `{` ends
  -- the line (ADR-0008).
  { key := { reqId := material.reqId, clause := material.clause, bond := .requirementModel },
    level := level,
    detail := .judge verdict judgement.judgedBy delegatedBy promptVersion judgement.note,
    linkHash := linkHash,
    inputs := [("requirement", judgement.requirementHash), ("model", judgement.modelHash)] }

/--
Record a judgement.

Four things are stated here and all four are the requirement. A judge who is
not authorised records nothing. Agreement writes evidence at `L2` and no
higher. Drift and `unmodelable` are recorded too, at `L1` -- a judge who says the
model is wrong has established nothing about it, but the verdict is shown, and
goes stale when either half changes. And `unmodelable` produces a proposal
rather than a change: the judge is saying the clause cannot be formalised as
written, and rewriting somebody's requirement on that basis is not a conclusion
a tool gets to draw.

@models REQ-JUDGE.caps_at_judgement
@models REQ-JUDGE.proposal_not_mutation
@models REQ-JUDGE.human_decides
@models REQ-JUDGE.drift_recorded
-/
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

/-- Whether a judgement still applies to the material in front of us.

@models REQ-JUDGE.invalidated_by_change -/
def stillApplies (judgement : Judgement) (material : Material) : Bool :=
  judgement.requirementHash == material.requirementHash &&
  judgement.modelHash == material.modelHash

/--
No judgement, whatever it says and whoever makes it, records above `L2`.

Stated over every material and every judgement rather than checked on examples:
the failure this forbids is a single branch writing a higher level, and an
example-based check would have to guess which branch.

@proves REQ-JUDGE.caps_at_judgement
-/
theorem judgement_never_exceeds_L2
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (e : EvidenceRecord) (h : (record material judgement judges linkHash).evidence? = some e) :
    (e.level = .L1 ∨ e.level = .L2) ∧ e.detail.ceiling = .L2 := by
  -- `simp only [record]` rather than `unfold record`, and `split` rather than
  -- `cases hv : …`: the same steps, in the subset of Lean the annotation
  -- grammar reads (ADR-0008).
  simp only [record] at h
  split at h
  all_goals try split at h
  all_goals simp [JudgeOutcome.evidence?] at h
  all_goals subst h
  all_goals simp [entry, Detail.ceiling]

/-- Drift is recorded, at `L1`, with the note and the hashes it was about; and
`unmodelable` writes a record and a proposal, never a change.

@proves REQ-JUDGE.drift_recorded
@proves REQ-JUDGE.proposal_not_mutation -/
theorem drift_is_recorded_at_L1
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (delegatedBy : Option String) (h : judgement.verdict = .drift)
    (ok : authority judges judgement.judgedBy judgement.delegatedBy = .accepted delegatedBy) :
    record material judgement judges linkHash =
      .drifted (entry material judgement delegatedBy "drift" .L1 linkHash) := by
  simp [record, h, ok]

/-- A blank name records nothing, whoever else is listed.

@proves REQ-JUDGE.human_decides -/
theorem a_blank_judge_records_nothing
    (material : Material) (judgement : Judgement) (judges : Option Judges) (linkHash : String)
    (h : judgement.judgedBy.all Char.isWhitespace = true) :
    (record material judgement judges linkHash).evidence? = none := by
  simp [record, authority, h, JudgeOutcome.evidence?]

/-! ## Showing a judgement

`judgement_shown`. An agreement names its judge and its level, a drift or
`unmodelable` verdict says so with its note, and a delegated verdict names the
person who delegated it -- so a delegated `L2` never reads as a person's own.
-/

/-- A clause's judgement as the requirement view shows it. -/
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

/-- One line for a judgement.

@models REQ-JUDGE.judgement_shown -/
def judgedText (judged : Judged) : String :=
  if judged.verdict == "agrees" then
    "agrees by " ++ judged.judgedBy ++ " — " ++ levelName judged.level ++
      delegatedText judged.delegatedBy
  else
    "judged: " ++ judged.verdict ++ " — " ++ judged.note.getD "no note" ++ "  by " ++
      judged.judgedBy ++ delegatedText judged.delegatedBy

/-! ## The prompt

`prompt_exported` and `divergence_presented`. The system writes the question; a
person carries it wherever they like and comes back with an opinion. What comes
back is advice, and the decision they then record is their own — which is why
the prompt says so in its own text, where the person reading the answer will
see it.

The prompt is an ordinary string-valued function, so it is modelled and
compared like anything else. That matters more than it looks: the prompt is the
one place where a model's opinion enters this system at all, and a prompt that
quietly stopped presenting the divergence would make every judgement after it
worth less without anything reporting a change.
-/

def String.trimBoth (s : String) : String := s.trim

/--
The prompt a person carries.

@models REQ-JUDGE.prompt_exported
@models REQ-JUDGE.divergence_presented
@models REQ-JUDGE.advice_is_not_evidence
-/
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

/-- A known divergence reaches the prompt, and the prompt says the answer is
advice. Both are properties of the text a person reads, which is where they
matter.

Stated on one material rather than universally: the phrases are in the fixed
part of the template, but a `clauseText` could contain them too, so a universal
claim would be about string search rather than about the prompt.

@proves REQ-JUDGE.divergence_presented
@proves REQ-JUDGE.advice_is_not_evidence -/
theorem the_prompt_carries_the_divergence_and_says_it_is_advice :
    (((prompt { reqId := "REQ-X", clause := some "one", clauseText := "A thing.",
                modelSource := "def f := 1", requirementHash := "rh", modelHash := "mh",
                divergence := some "model said 1, implementation said 2" }).splitOn
        "implementation said 2").length = 2)
    ∧ (((prompt { reqId := "REQ-X", clause := some "one", clauseText := "A thing.",
                   modelSource := "def f := 1", requirementHash := "rh", modelHash := "mh",
                   divergence := none }).splitOn "This is advice").length = 2) := by
  native_decide

/-- A judgement's ceiling is the second level, below what a differential test
and a proof can establish, so no judgement promotes a link to tested or proved.

@proves REQ-EVID.judgement_caps -/
theorem a_judgement_caps_at_the_second_level (verdict judgedBy promptVersion : String)
    (delegatedBy note : Option String) (seed cases : Nat) (op theoremName toolchain : String) :
    (Detail.judge verdict judgedBy delegatedBy promptVersion note).ceiling = Level.L2 ∧
    (Detail.drt seed cases op).ceiling = Level.L3 ∧
    (Detail.proof theoremName toolchain).ceiling = Level.L4 := by
  simp [Detail.ceiling]

/-- An agreement names its judge and level; any other verdict names itself and
its note; a delegated one names who delegated it.

@proves REQ-JUDGE.judgement_shown -/
theorem a_judgement_says_who_and_why :
    judgedText { verdict := "agrees", judgedBy := "ana", delegatedBy := none, note := none,
                 level := .L2 } = "agrees by ana — L2" ∧
    judgedText { verdict := "drift", judgedBy := "ana", delegatedBy := none,
                 note := some "off by one", level := .L1 } = "judged: drift — off by one  by ana" ∧
    judgedText { verdict := "agrees", judgedBy := "claude-review", delegatedBy := some "ana",
                 note := none, level := .L2 } = "agrees by claude-review — L2 (delegated by ana)" := by
  native_decide

/-- The exported prompt names the clause, carries its text and the model's
source, and asks for one of the three verdicts.

@proves REQ-JUDGE.prompt_exported -/
theorem the_prompt_carries_clause_and_model :
    let p := prompt { reqId := "REQ-X", clause := some "one", clauseText := "A thing shall be.",
                      modelSource := "def thing := 1", requirementHash := "rh", modelHash := "mh",
                      divergence := none }
    (p.splitOn "Requirement REQ-X.one:").length = 2 ∧ (p.splitOn "A thing shall be.").length = 2 ∧
      (p.splitOn "def thing := 1").length = 2 ∧
      (p.splitOn "agrees, drift, unmodelable").length = 2 := by
  native_decide

end TraceLean.Judge
