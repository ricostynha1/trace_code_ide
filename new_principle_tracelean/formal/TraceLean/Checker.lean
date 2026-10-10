import TraceLean.Annotation

/-!
# Findings

Models `REQ-CHECK`. Naming is the feature. "Traceability error" is not
actionable in a diff; *this clause has a model and an implementation and nothing
binds them* is a task.

Two decisions are stated here, and both are ones a reader has to be able to
trust without reading the scanner. **Exactly one** of unmodelled, unimplemented
and unbound applies to a clause, so a report cannot say the same thing twice
under two names. And a kind that describes **progress** must not be in the
default blocking set -- a tool that fails a build because somebody has not
written a model yet gets its checks disabled in week one, and then none of the
rest of this matters.
-/

namespace TraceLean.Checker

open TraceLean.Annotation

open Lean (ToJson FromJson)

inductive Severity where
  | info | warn | error
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The kinds of finding; `facts` is their model. -/
inductive Kind where
  /-- An annotation names a requirement or clause that does not exist. -/
  | dangling
  /-- `refines:` names a requirement that does not exist. -/
  | danglingRefines
  /-- The refinement graph contains a cycle. -/
  | refinesCycle
  /-- Two documents declare the same identifier. -/
  | duplicateId
  /-- A clause has no model and is not exempt. -/
  | unmodeled
  /-- Modelled, but nothing claims to implement it. -/
  | unimplemented
  /-- Model and implementation both exist, and nothing binds them. -/
  | unbound
  /-- Implemented, but no test. -/
  | untested
  /-- Two links exclusively claim the same clause. -/
  | contested
  /-- An exemption without a reason or an approver. -/
  | unsoundExemption
  /-- A partial or nondeterministic qualifier without a reason. -/
  | unsoundQualifier
  /-- Something wrong in a document or an annotation. -/
  | malformed
  /-- A source file did not parse cleanly, so its annotations anchor to the
  whole file and are capped at the lowest evidence level. Not an error. -/
  | imprecise
  /-- A clause with more than one `models` declaration (ADR-0014). -/
  | severalModels
  /-- A clause with more than one `specifies` declaration. -/
  | severalSpecs
  /-- A clause with more than one `pins` theorem. -/
  | severalPins
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether this names work not yet done, rather than something broken. -/
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

/-- Which kinds fail a build. Deliberately small: a requirement with no model
yet is where the work is, not a broken build. -/
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

/-- What is known about a kind, in one place. -/
structure KindFacts where
  severity : Severity
  progress : Bool
  blocksByDefault : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Severity, progress and the default gate for one kind.

Together because the requirement is that they agree.

@models REQ-CHECK.named_kinds
@models REQ-CHECK.progress_not_fault
@models REQ-CHECK.severity_policy -/
def facts (kind : Kind) : KindFacts :=
  { severity := kind.severity, progress := kind.progress,
    blocksByDefault := kind.blocksByDefault }

/--
What a clause's claimed roles imply.

Exactly one of unmodelled, unimplemented and unbound -- they are the three
points of a single chain, and reporting two of them for one clause would be
reporting the same gap twice. `untested` is separate because it is a different
gap.

An exempt clause yields nothing at all. That is what an exemption is, and why
one has to carry a reason and an approver.

@models REQ-CHECK.exactly_once
@models REQ-CHECK.unbound_reported
@models REQ-CHECK.structural_is_not_exempt
-/
def coverageKinds (roles : List Role) (exempt : Bool) (structural : Bool) : List Kind :=
  -- The four questions asked of a clause, named once before the branches: the
  -- grammar that reads these annotations cannot follow a run of `let` bindings
  -- inside an `else`, and this is the same function written the other way
  -- round (ADR-0008).
  let modeled := roles.contains .models
  let implemented := roles.contains .implements
  let bound := roles.contains .drt
  let tested := roles.contains .tests
  if exempt then []
  else
    -- A structural clause is a property of the tree, so there is no
    -- data-to-data function to model, no second implementation to compare
    -- against, and nothing to point an `implements` at -- the tree realises it
    -- by being the shape it is. What there is, and what is required, is a test
    -- that reads the repository. The clause stays in the denominator, which an
    -- exemption would not.
    if structural then
      if !tested then [Kind.untested] else []
    else
      let chain :=
        if !modeled then [Kind.unmodeled]
        else if !implemented then [Kind.unimplemented]
        else if !bound then [Kind.unbound]
        else []
      chain ++ (if implemented && !tested then [Kind.untested] else [])

/-- What a qualifier on a link implies about the link itself.

@models REQ-CHECK.qualifier_soundness -/
def qualifierKinds (qualifier : Option Qualifier) : List Kind :=
  match qualifier with
  | some (.exempt reason judgedBy _) =>
    if reason.isNone || judgedBy.isNone then [Kind.unsoundExemption] else []
  | some (.partial none) => [Kind.unsoundQualifier]
  | some (.nondeterministic none) => [Kind.unsoundQualifier]
  -- A structural clause that does not say why there is no law to state is
  -- indistinguishable from one nobody got round to modelling.
  | some (.structural none) => [Kind.unsoundQualifier]
  | _ => []

/-- Which roles a clause carries more than one of, as the kinds that report it:
one model, at most one specification, at most one pin (ADR-0014). `roles` holds
one entry per declaration claiming the clause.

@models REQ-CHECK.one_of_each_role -/
def crowdedKinds (roles : List Role) : List Kind :=
  [(Role.models, Kind.severalModels), (Role.specifies, Kind.severalSpecs),
    (Role.pins, Kind.severalPins)].filterMap (fun pair =>
      if (roles.filter (· == pair.1)).length > 1 then some pair.2 else none)

/-- One model is never reported.

@proves REQ-CHECK.one_of_each_role -/
theorem one_model_is_not_crowded :
    crowdedKinds [Role.models, Role.specifies, Role.pins, Role.tests, Role.tests] = [] := by
  decide

/-- Two models are.

@proves REQ-CHECK.one_of_each_role -/
theorem two_models_are_crowded :
    crowdedKinds [Role.models, Role.implements, Role.models] = [Kind.severalModels] := by
  decide

/-- At most one of the chain kinds is ever reported for a clause.

@proves REQ-CHECK.exactly_once -/
theorem the_chain_reports_at_most_one (roles : List Role) (exempt structural : Bool) :
    ((coverageKinds roles exempt structural).filter
      (fun k => k == .unmodeled || k == .unimplemented || k == .unbound)).length ≤ 1 := by
  -- `cases` on each Boolean rather than `by_cases h : …`: the same split into
  -- true and false, in the subset of Lean the annotation grammar reads
  -- (ADR-0008). Nothing here needs the hypothesis by name, only the two cases.
  unfold coverageKinds
  cases exempt <;>
    cases structural <;>
    cases roles.contains Role.models <;>
    cases roles.contains Role.implements <;>
    cases roles.contains Role.drt <;>
    cases roles.contains Role.tests <;>
    simp

/-- No kind that describes progress blocks a build.

@proves REQ-CHECK.progress_not_fault
@proves REQ-CHECK.severity_policy -/
theorem progress_never_blocks (kind : Kind) (h : kind.progress = true) :
    kind.blocksByDefault = false := by
  cases kind <;> simp_all [Kind.progress, Kind.blocksByDefault]

end TraceLean.Checker
