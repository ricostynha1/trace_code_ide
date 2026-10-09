import TraceLean.Evidence
import Lean

/-!
# Evidence records

The shape a record has on the wire and in the lockfile, and what makes one
reproducible. Separate from `Evidence` because that module is the algebra --
what a claim is worth and how worths combine -- while this is the record itself:
who established it, by what method, and depending on what.
-/

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

/-- Read a detail; an absent delegate or note is `none`, as in records made
before either existed. -/
def Verdict.fromJson? (j : Lean.Json) : Except String Verdict :=
  match j.getObjVal? "judge", j.getObjVal? "drt", j.getObjVal? "proof" with
  | .ok p, _, _ => Verdict.judgeFromJson p
  | _, .ok p, _ => Verdict.drtFromJson p
  | _, _, .ok p => Verdict.proofFromJson p
  | _, _, _ => .error "no inductive constructor matched"

instance : FromJson Verdict := ⟨Verdict.fromJson?⟩

/-- The wire form, which leaves out an absent delegate or note rather than
writing `null`, as the records on disk do (ADR-0015). -/
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


/-! ## Reproducibility

`REQ-EVID.record_reproducible` and `REQ-STALE.inputs_identified`.

A record carries what is needed to reproduce it, and cannot be expressed without
it. The type does most of the work -- a differential record has nowhere to put a
seed except the `seed` field -- but a type cannot stop an empty string, and an
empty theorem name is a record nobody can check.

The inputs are the other half. A record that did not name what it depended on
could never be invalidated, so it would be believed forever.
-/

/-- What is wrong with a record, if anything. -/
inductive Reproducibility where
  | reproducible
  /-- A field the method needs is empty. -/
  | incomplete (field : String)
  /-- An input the method depends on is not named. -/
  | missingInput (name : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The inputs a record of this kind must name.

A judgement depends on the requirement text and the model; a differential run on
the model and the implementation; a proof on the model and the toolchain that
checked it. Each is something that can change underneath the record, and
something that changes and is not named is a record that stays valid through a
change it should not have survived. -/
def requiredInputs : Verdict → List String
  | .judge .. => ["requirement", "model"]
  | .drt _ _ _ => ["model", "implementation"]
  | .proof _ _ => ["model", "toolchain"]

/-- Whether a record carries what is needed to reproduce and to invalidate it.

The first fault is the one reported, checking the method's own fields before its
inputs: a record whose method is not even complete is not worth asking about
dependencies.

@models REQ-EVID.record_reproducible
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
  -- The field check is a definition rather than a `let` holding a `match` over
  -- several lines, which the grammar that reads these annotations cannot read
  -- (ADR-0008).
  match missingField record.detail with
  | some field => .incomplete field
  | none =>
    if record.linkHash.isEmpty then .incomplete "linkHash"
    else
      match (requiredInputs record.detail).find?
        (fun name => !(record.inputs.any (·.1 == name))) with
      | some name => .missingInput name
      | none => .reproducible

/-- A differential record with a given number of cases and no inputs recorded.

Built by a definition rather than written inline in the theorems below: the
grammar that reads these annotations cannot follow a structure instance given
as an argument across several lines (ADR-0008). -/
private def drtSample (cases : Nat) : Evidence :=
  { key := { reqId := "REQ-X", clause := none, bond := .modelImpl },
    level := .L3,
    detail := .drt 7 cases "REQ-X",
    linkHash := "l1",
    inputs := [] }

/-- A differential record with no cases behind it is not a record of anything.

@proves REQ-EVID.record_reproducible -/
theorem no_cases_is_not_reproducible :
    reproducibility (drtSample 0) = Reproducibility.incomplete "cases" := by
  native_decide

/-- And one that named no inputs could never be invalidated.

@proves REQ-STALE.inputs_identified -/
theorem unnamed_inputs_are_reported :
    reproducibility (drtSample 2000) = Reproducibility.missingInput "model" := by
  native_decide

end TraceLean.Record
