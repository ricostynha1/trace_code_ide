import TraceLean.Annotation

namespace TraceLean.Checker

open TraceLean.Annotation

open Lean (ToJson FromJson)

inductive Severity where
  | info | warn | error
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Kind where
  | dangling
  | danglingRefines
  | refinesCycle
  | duplicateId
  | unmodeled
  | unimplemented
  | unbound
  | untested
  | contested
  | unsoundExemption
  | unsoundQualifier
  | malformed
  | imprecise
  | severalModels
  | severalSpecs
  | severalPins
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Kind.progress : Kind → Bool
  | .unmodeled => true
  | .unimplemented => true
  | .untested => true
  | _ => false

def Kind.severity : Kind → Severity
  | .unmodeled => .info
  | .unimplemented => .info
  | .untested => .info
  | .unbound => .warn
  | .unsoundQualifier => .warn
  | .imprecise => .warn
  | _ => .error

def Kind.blocksByDefault : Kind → Bool
  | .dangling => true
  | .danglingRefines => true
  | .refinesCycle => true
  | .duplicateId => true
  | .contested => true
  | .unsoundExemption => true
  | .malformed => true
  | .severalModels => true
  | .severalSpecs => true
  | .severalPins => true
  | _ => false

structure KindFacts where
  severity : Severity
  progress : Bool
  blocksByDefault : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- @models REQ-CHECK.named_kinds
@models REQ-CHECK.progress_not_fault
@models REQ-CHECK.severity_policy -/
def facts (kind : Kind) : KindFacts :=
  { severity := kind.severity, progress := kind.progress,
    blocksByDefault := kind.blocksByDefault }

/-- @models REQ-CHECK.exactly_once
@models REQ-CHECK.unbound_reported
@models REQ-CHECK.structural_is_not_exempt -/
def coverageKinds (roles : List Role) (exempt : Bool) (structural : Bool) : List Kind :=
  let modeled := roles.contains .models
  let implemented := roles.contains .implements
  let bound := roles.contains .drt
  let tested := roles.contains .tests
  if exempt then []
  else
    if structural then
      if !tested then [Kind.untested] else []
    else
      let chain :=
        if !modeled then [Kind.unmodeled]
        else if !implemented then [Kind.unimplemented]
        else if !bound then [Kind.unbound]
        else []
      chain ++ (if implemented && !tested then [Kind.untested] else [])

def expired (expires today : Option String) : Bool :=
  match expires, today with
  | some e, some t => decide (e < t)
  | _, _ => false

/-- @models REQ-CHECK.qualifier_soundness -/
def qualifierKinds (qualifier : Option Qualifier) (today : Option String) : List Kind :=
  match qualifier with
  | some (.exempt reason judgedBy expires) =>
    if reason.isNone || judgedBy.isNone || expired expires today then [Kind.unsoundExemption] else []
  | some (.partial none) => [Kind.unsoundQualifier]
  | some (.nondeterministic none) => [Kind.unsoundQualifier]
  | some (.structural none) => [Kind.unsoundQualifier]
  | _ => []

/-- @models REQ-CHECK.one_of_each_role -/
def crowdedKinds (roles : List Role) : List Kind :=
  [(Role.models, Kind.severalModels), (Role.specifies, Kind.severalSpecs),
    (Role.pins, Kind.severalPins)].filterMap (fun pair =>
      if (roles.filter (· == pair.1)).length > 1 then some pair.2 else none)

/-- @proves REQ-CHECK.exactly_once -/
theorem the_chain_reports_at_most_one (roles : List Role) (exempt structural : Bool) :
    ((coverageKinds roles exempt structural).filter
      (fun k => k == .unmodeled || k == .unimplemented || k == .unbound)).length ≤ 1 := by
  unfold coverageKinds
  cases exempt <;>
    cases structural <;>
    cases roles.contains Role.models <;>
    cases roles.contains Role.implements <;>
    cases roles.contains Role.drt <;>
    cases roles.contains Role.tests <;>
    simp

/-- @proves REQ-CHECK.progress_not_fault
@proves REQ-CHECK.severity_policy -/
theorem progress_never_blocks (kind : Kind) (h : kind.progress = true) :
    kind.blocksByDefault = false := by
  cases kind <;> simp_all [Kind.progress, Kind.blocksByDefault]

/-- @proves REQ-CHECK.unbound_reported -/
theorem model_and_code_unbound_is_reported (roles : List Role)
    (modeled : roles.contains Role.models = true) (implemented : roles.contains Role.implements = true)
    (unbound : roles.contains Role.drt = false) :
    (coverageKinds roles false false).contains Kind.unbound = true := by
  unfold coverageKinds
  simp only [modeled, implemented, unbound]
  simp

/-- @proves REQ-CHECK.structural_is_not_exempt -/
theorem structural_asks_only_for_a_test (roles : List Role) :
    coverageKinds roles false true = (if roles.contains Role.tests then [] else [Kind.untested]) := by
  unfold coverageKinds
  cases roles.contains Role.tests <;> simp

/-- @proves REQ-CHECK.qualifier_soundness -/
theorem an_exemption_is_signed_and_in_date (reason judge due today : String) (expires : Option String)
    (date : Option String) :
    qualifierKinds (some (.exempt none (some judge) expires)) date = [Kind.unsoundExemption] ∧
    qualifierKinds (some (.exempt (some reason) none expires)) date = [Kind.unsoundExemption] ∧
    qualifierKinds (some (.exempt (some reason) (some judge) expires)) none = [] ∧
    (due < today →
      qualifierKinds (some (.exempt (some reason) (some judge) (some due))) (some today)
        = [Kind.unsoundExemption]) := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · simp [qualifierKinds]
  · simp [qualifierKinds]
  · simp [qualifierKinds, expired]
  · intro late
    simp [qualifierKinds, expired, late]

/-- @proves REQ-CHECK.named_kinds -/
theorem every_kind_is_reported_by_its_own_name (kind : Kind) :
    (Lean.fromJson? (Lean.toJson kind) : Except String Kind) = .ok kind ∧
    ((facts kind).severity = .error → (facts kind).progress = false) := by
  cases kind <;> exact ⟨rfl, by simp [facts, Kind.severity, Kind.progress]⟩

/-- @proves REQ-CHECK.one_of_each_role -/
theorem a_crowded_role_is_named (roles : List Role) :
    (Kind.severalModels ∈ crowdedKinds roles ↔ (roles.filter (· == Role.models)).length > 1) ∧
    (Kind.severalSpecs ∈ crowdedKinds roles ↔ (roles.filter (· == Role.specifies)).length > 1) ∧
    (Kind.severalPins ∈ crowdedKinds roles ↔ (roles.filter (· == Role.pins)).length > 1) := by
  unfold crowdedKinds
  by_cases a : (roles.filter (· == Role.models)).length > 1 <;>
    by_cases b : (roles.filter (· == Role.specifies)).length > 1 <;>
    by_cases c : (roles.filter (· == Role.pins)).length > 1 <;>
    simp [a, b, c]

end TraceLean.Checker
