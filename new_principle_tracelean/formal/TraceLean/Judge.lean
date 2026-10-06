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
  | judge (verdict judgedBy promptVersion : String)
  | drt (seed cases : Nat) (op : String)
  | proof (theoremName toolchain : String)
  deriving Repr, Inhabited, ToJson, FromJson

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
  note : Option String
  requirementHash : String
  modelHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- A change a judgement suggests, which nothing applies automatically. -/
structure Proposal where
  reqId : String
  clause : Option String
  suggestion : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What recording a judgement produced. -/
inductive JudgeOutcome where
  | recorded (evidence : EvidenceRecord)
  | drifted
  | proposed (proposal : Proposal)
  deriving Repr, Inhabited, ToJson, FromJson

def promptVersion : String := "1"

/--
Record a judgement.

Three things are stated here and all three are the requirement. Agreement writes
evidence at `L2` and no higher. Drift writes nothing -- a judge who says the
model is wrong has not established anything about it. And `unmodelable`
produces a proposal rather than a change: the judge is saying the clause cannot
be formalised as written, and rewriting somebody's requirement on that basis is
not a conclusion a tool gets to draw.

The inputs recorded are the two hashes the judgement was about, so that changing
either half means nobody has judged the pair that now exists.

@models REQ-JUDGE.caps_at_judgement
@models REQ-JUDGE.proposal_not_mutation
@models REQ-JUDGE.human_decides
-/
def record (material : Material) (judgement : Judgement) (linkHash : String) : JudgeOutcome :=
  match judgement.verdict with
  | .agrees =>
    -- The brace opens on its own line: the grammar that reads these annotations
    -- cannot follow a constructor applied to a structure instance whose `{`
    -- ends the line (ADR-0008).
    .recorded
      { key := { reqId := material.reqId, clause := material.clause, bond := .requirementModel },
        level := .L2,
        detail := .judge "agrees" judgement.judgedBy promptVersion,
        linkHash := linkHash,
        inputs := [("requirement", judgement.requirementHash), ("model", judgement.modelHash)] }
  | .drift => .drifted
  | .unmodelable =>
    .proposed
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
    (material : Material) (judgement : Judgement) (linkHash : String) (e : EvidenceRecord)
    (h : record material judgement linkHash = .recorded e) :
    e.level = .L2 ∧ e.detail.ceiling = .L2 := by
  -- `simp only [record]` rather than `unfold record`, and `split` rather than
  -- `cases hv : …`: the same steps, in the subset of Lean the annotation
  -- grammar reads (ADR-0008).
  simp only [record] at h
  split at h
  all_goals simp at h
  all_goals subst h
  all_goals exact ⟨rfl, rfl⟩

/-- Drift establishes nothing, and `unmodelable` changes nothing. The other two
branches of the same requirement.

@proves REQ-JUDGE.proposal_not_mutation -/
theorem drift_records_nothing
    (material : Material) (judgement : Judgement) (linkHash : String)
    (h : judgement.verdict = .drift) :
    record material judgement linkHash = .drifted := by
  simp [record, h]

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

end TraceLean.Judge
