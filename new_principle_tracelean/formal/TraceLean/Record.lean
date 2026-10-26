import TraceLean.Evidence
import Lean

namespace TraceLean.Record

open TraceLean.Evidence

open Lean (ToJson FromJson)

inductive Verdict where
  | judge (verdict judgedBy : String) (delegatedBy : Option String) (promptVersion : String)
      (note : Option String)
  | drt (seed cases : Nat) (op : String)
  | proof (theoremName toolchain : String)
  deriving Repr, DecidableEq, Inhabited

def Verdict.judgeFromJson (p : Lean.Json) : Except String Verdict := do
  let verdict ← p.getObjValAs? String "verdict"
  let judgedBy ← p.getObjValAs? String "judgedBy"
  let delegatedBy ← p.getObjValAs? (Option String) "delegatedBy"
  let promptVersion ← p.getObjValAs? String "promptVersion"
  let note ← p.getObjValAs? (Option String) "note"
  pure (Verdict.judge verdict judgedBy delegatedBy promptVersion note)

def Verdict.drtFromJson (p : Lean.Json) : Except String Verdict := do
  let seed ← p.getObjValAs? Nat "seed"
  let cases ← p.getObjValAs? Nat "cases"
  let op ← p.getObjValAs? String "op"
  pure (Verdict.drt seed cases op)

def Verdict.proofFromJson (p : Lean.Json) : Except String Verdict := do
  let theoremName ← p.getObjValAs? String "theoremName"
  let toolchain ← p.getObjValAs? String "toolchain"
  pure (Verdict.proof theoremName toolchain)

def Verdict.fromJson? (j : Lean.Json) : Except String Verdict :=
  match j.getObjVal? "judge", j.getObjVal? "drt", j.getObjVal? "proof" with
  | .ok p, _, _ => Verdict.judgeFromJson p
  | _, .ok p, _ => Verdict.drtFromJson p
  | _, _, .ok p => Verdict.proofFromJson p
  | _, _, _ => .error "no inductive constructor matched"

instance : FromJson Verdict := ⟨Verdict.fromJson?⟩

def Verdict.toJson : Verdict → Lean.Json
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

instance : ToJson Verdict := ⟨Verdict.toJson⟩

structure EvidenceKey where
  reqId : String
  clause : Option String
  bond : Bond
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Evidence where
  key : EvidenceKey
  level : Level
  detail : Verdict
  linkHash : String
  inputs : List (String × String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Reproducibility where
  | reproducible
  | incomplete (field : String)
  | missingInput (name : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def requiredInputs : Verdict → List String
  | .judge .. => ["requirement", "model"]
  | .drt _ _ _ => ["model", "implementation"]
  | .proof _ _ => ["model", "toolchain"]

/-- @models REQ-EVID.record_reproducible
@models REQ-STALE.inputs_identified -/
private def missingField (detail : Verdict) : Option String :=
  match detail with
  | Verdict.judge verdict judgedBy _ promptVersion _ =>
    if verdict.isEmpty then some "verdict"
    else if judgedBy.isEmpty then some "judgedBy"
    else if promptVersion.isEmpty then some "promptVersion"
    else none
  | Verdict.drt _ cases op =>
    if cases == 0 then some "cases" else if op.isEmpty then some "op" else none
  | Verdict.proof theoremName toolchain =>
    if theoremName.isEmpty then some "theoremName"
    else if toolchain.isEmpty then some "toolchain"
    else none

def reproducibility (record : Evidence) : Reproducibility :=
  match missingField record.detail with
  | some field => .incomplete field
  | none =>
    if record.linkHash.isEmpty then .incomplete "linkHash"
    else
      match (requiredInputs record.detail).find?
        (fun name => !(record.inputs.any (·.1 == name))) with
      | some name => .missingInput name
      | none => .reproducible


def Complete : Verdict → Prop
  | .judge verdict judgedBy _ promptVersion _ =>
    verdict.isEmpty = false ∧ judgedBy.isEmpty = false ∧ promptVersion.isEmpty = false
  | .drt _ cases op => cases ≠ 0 ∧ op.isEmpty = false
  | .proof theoremName toolchain => theoremName.isEmpty = false ∧ toolchain.isEmpty = false

private theorem missingField_none (detail : Verdict) : missingField detail = none ↔ Complete detail := by
  cases detail with
  | judge v j d p n =>
    simp only [missingField, Complete]
    cases v.isEmpty <;> cases j.isEmpty <;> cases p.isEmpty <;> simp
  | drt s c o =>
    simp only [missingField, Complete]
    by_cases hc : c = 0
    · simp [hc]
    · cases ho : o.isEmpty <;> simp [hc, ho]
  | proof t tc =>
    simp only [missingField, Complete]
    cases t.isEmpty <;> cases tc.isEmpty <;> simp

/-- @proves REQ-EVID.record_reproducible -/
theorem a_record_is_reproducible_only_when_complete (record : Evidence) :
    reproducibility record = .reproducible ↔
      Complete record.detail ∧ record.linkHash.isEmpty = false ∧
      ∀ name ∈ requiredInputs record.detail, ∃ i ∈ record.inputs, i.1 = name := by
  rw [← missingField_none]
  unfold reproducibility
  cases _hm : missingField record.detail with
  | some field => simp
  | none =>
    cases _hl : record.linkHash.isEmpty with
    | true => simp
    | false =>
      rw [if_neg (by decide)]
      simp only [true_and]
      cases hf : (requiredInputs record.detail).find? (fun name => !(record.inputs.any (·.1 == name))) with
      | some name =>
        have hmem := List.mem_of_find?_eq_some hf
        have hp := List.find?_some hf
        constructor
        · intro h; cases h
        · intro hall
          obtain ⟨i, hi, e⟩ := hall name hmem
          have : record.inputs.any (fun x => x.1 == name) = true :=
            List.any_eq_true.mpr ⟨i, hi, by simp [e]⟩
          simp [this] at hp
      | none =>
        constructor
        · intro _ name hname
          rw [List.find?_eq_none] at hf
          have := hf name hname
          simp only [Bool.not_eq_true', Bool.not_eq_false, List.any_eq_true, beq_iff_eq] at this
          exact this
        · intro _; rfl

/-- @proves REQ-STALE.inputs_identified -/
theorem a_record_names_every_input (record : Evidence) :
    (reproducibility record = .reproducible →
      ∀ name ∈ requiredInputs record.detail, ∃ hash, (name, hash) ∈ record.inputs) ∧
    "model" ∈ requiredInputs record.detail ∧
    (∀ name, reproducibility record = .missingInput name →
      name ∈ requiredInputs record.detail ∧ ∀ i ∈ record.inputs, i.1 ≠ name) := by
  refine ⟨?_, ?_, ?_⟩
  · intro h name hname
    obtain ⟨_, _, hall⟩ := (a_record_is_reproducible_only_when_complete record).mp h
    obtain ⟨⟨n, hash⟩, hi, e⟩ := hall name hname
    exact ⟨hash, by simp only at e; rw [← e]; exact hi⟩
  · cases record.detail <;> simp [requiredInputs]
  · intro name h
    unfold reproducibility at h
    split at h
    · cases h
    · split at h
      · cases h
      · split at h
        · rename_i hf
          cases h
          have hp := List.find?_some hf
          refine ⟨List.mem_of_find?_eq_some hf, fun i hi e => ?_⟩
          have : record.inputs.any (fun x => x.1 == name) = true :=
            List.any_eq_true.mpr ⟨i, hi, by simp [e]⟩
          simp [this] at hp
        · cases h

end TraceLean.Record
