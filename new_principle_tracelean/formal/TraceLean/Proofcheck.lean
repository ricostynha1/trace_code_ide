import Lean
import TraceLean.Annotation
import TraceLean.Strength

namespace TraceLean.Proofcheck

open Lean (Json ToJson FromJson toJson)
open TraceLean.Annotation (Role)
open TraceLean.Strength (Strength)

inductive Refusal where
  | «sorry»
  | «axiom» (name : String)
  | silent
  | «example»
  | indirect (via : String)
  | vacuous
  | type
  | missing
  | statement
  | unchecked
  deriving Repr, DecidableEq, Inhabited

instance : ToJson Refusal where
  toJson
    | .«sorry» => Json.mkObj [("reason", "sorry")]
    | .«axiom» name => Json.mkObj [("reason", "axiom"), ("via", toJson name)]
    | .silent => Json.mkObj [("reason", "silent")]
    | .«example» => Json.mkObj [("reason", "example")]
    | .indirect via => Json.mkObj [("reason", "indirect"), ("via", toJson via)]
    | .vacuous => Json.mkObj [("reason", "vacuous")]
    | .type => Json.mkObj [("reason", "type")]
    | .missing => Json.mkObj [("reason", "missing")]
    | .statement => Json.mkObj [("reason", "statement")]
    | .unchecked => Json.mkObj [("reason", "unchecked")]

inductive Verdict where
  | accepted (assumed : List String)
  | refused (refusal : Refusal)
  deriving Repr, DecidableEq, Inhabited

instance : ToJson Verdict where
  toJson
    | .accepted assumed => Json.mkObj [("verdict", "accepted"), ("assumed", toJson assumed)]
    | .refused refusal => Json.mkObj [("verdict", "refused"), ("refusal", toJson refusal)]

def Verdict.isAccepted : Verdict → Bool
  | .accepted _ => true
  | .refused _ => false

structure ProofFacts where
  found : Bool
  isType : Bool
  mentions : Bool
  general : Bool
  leak : Option String
  ignores : Bool
  closes : Bool
  axioms : List (String × Bool)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure PinFacts where
  found : Bool
  statement : Bool
  axioms : List (String × Bool)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def leanOwn : List String := ["propext", "Classical.choice", "Quot.sound"]

def axiomStep (acc : Except Refusal (List String)) (a : String × Bool) : Except Refusal (List String) :=
  match acc with
  | .error e => .error e
  | .ok kept =>
    if a.1 == "sorryAx" then .error .«sorry»
    else if leanOwn.contains a.1 then .ok kept
    else if !a.2 then .error (.«axiom» a.1)
    else .ok (kept ++ [a.1])

def axiomVerdict (axioms : List (String × Bool)) : Except Refusal (List String) :=
  match axioms.foldl axiomStep (.ok []) with
  | .error e => .error e
  | .ok kept => .ok (kept.mergeSort (fun a b => decide (a ≤ b)))

/-- @models REQ-STRENGTH.proof_states_the_model
@models REQ-STRENGTH.proof_is_general
@models REQ-STRENGTH.proof_is_direct
@models REQ-STRENGTH.proof_not_vacuous
@models REQ-STRENGTH.model_is_a_function
@models REQ-STRENGTH.axioms_declared
@models REQ-STRENGTH.assumptions_shown -/
def verdict (facts : ProofFacts) : Verdict :=
  if !facts.found then .refused .missing
  else if facts.isType then .refused .type
  else
    match axiomVerdict facts.axioms with
    | .error r => .refused r
    | .ok assumed =>
      if !facts.mentions then .refused .silent
      else if !facts.general then .refused .«example»
      else
        match facts.leak with
        | some via => .refused (.indirect via)
        | none => if facts.ignores || facts.closes then .refused .vacuous else .accepted assumed

/-- @models REQ-STRENGTH.pinned_by_theorem -/
def pinVerdict (facts : PinFacts) : Verdict :=
  if !facts.found then .refused .missing
  else
    match axiomVerdict facts.axioms with
    | .error r => .refused r
    | .ok assumed => if !facts.statement then .refused .statement else .accepted assumed

def pinnedNow (held : Option (String × List String)) (keyNow : String) (pinsNow : List String) :
    Option String :=
  match held with
  | some (key, accepted) => if key = keyNow then accepted.find? (fun p => decide (p ∈ pinsNow)) else none
  | none => none

/-- @models REQ-STRENGTH.open_is_the_default
@models REQ-STRENGTH.attempted_distinguished
@models REQ-STRENGTH.nondeterministic_declared
@models REQ-STRENGTH.verdict_kept
@models REQ-STRENGTH.qualifies_proof -/
def standing (held : Option (String × List String)) (keyNow : String) (pinsNow : List String)
    (nondeterministic : Option String) : Strength :=
  match nondeterministic with
  | some reason => .nondeterministic reason
  | none =>
    match pinnedNow held keyNow pinsNow with
    | some t => .pinned t
    | none =>
      match pinsNow with
      | t :: _ => .attempted t
      | [] => .«open»

inductive Heard where
  | facts (thm : String) (model : String) (facts : ProofFacts)
  | obligation (model : String) (statement : String)
  | pinFacts (thm : String) (model : String) (facts : PinFacts)
  | missing (model : String)
  deriving Repr, DecidableEq, Inhabited

instance : FromJson Heard where
  fromJson? j := do
    let tag ← j.getObjValAs? String "heard"
    match tag with
    | "facts" =>
      let thm ← j.getObjValAs? String "theorem"
      let model ← j.getObjValAs? String "model"
      return .facts thm model (← j.getObjValAs? ProofFacts "facts")
    | "obligation" => return .obligation (← j.getObjValAs? String "model") (← j.getObjValAs? String "statement")
    | "pinFacts" =>
      let thm ← j.getObjValAs? String "theorem"
      let model ← j.getObjValAs? String "model"
      return .pinFacts thm model (← j.getObjValAs? PinFacts "facts")
    | "missing" => return .missing (← j.getObjValAs? String "model")
    | other => throw s!"unknown heard {other}"

def heardAccepted : Heard → Option (String × String)
  | .facts thm model facts => if (verdict facts).isAccepted then some (model, thm) else none
  | _ => none

/-- @models REQ-STRENGTH.obligation_generated -/
def accepted (heard : List Heard) : List (String × String) :=
  heard.filterMap heardAccepted

structure CheckLink where
  role : Role
  reqId : String
  clause : Option String
  symbol : String
  file : String
  line : Nat
  bodyHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Claimed where
  thm : String
  reqId : String
  clause : Option String
  file : String
  line : Nat
  bodyHash : String
  deriving Repr, DecidableEq, Inhabited

instance : ToJson Claimed where
  toJson c := Json.mkObj [("theorem", toJson c.thm), ("reqId", toJson c.reqId), ("clause", toJson c.clause),
    ("file", toJson c.file), ("line", toJson c.line), ("bodyHash", toJson c.bodyHash)]

instance : FromJson Claimed where
  fromJson? j := return ⟨← j.getObjValAs? String "theorem", ← j.getObjValAs? String "reqId",
    ← j.getObjValAs? (Option String) "clause", ← j.getObjValAs? String "file", ← j.getObjValAs? Nat "line",
    ← j.getObjValAs? String "bodyHash"⟩

structure ModelPlan where
  model : String
  file : String
  bodyHash : String
  clauses : List (String × Option String)
  theorems : List Claimed
  pins : List Claimed
  sources : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def optLe : Option String → Option String → Bool
  | none, _ => true
  | some _, none => false
  | some a, some b => decide (a ≤ b)

def clauseLe (a b : String × Option String) : Bool :=
  if a.1 == b.1 then optLe a.2 b.2 else decide (a.1 ≤ b.1)

def claimedLe (a b : Claimed) : Bool :=
  if a.thm == b.thm then clauseLe (a.reqId, a.clause) (b.reqId, b.clause)
  else decide (a.thm ≤ b.thm)

def modelOf (links : List CheckLink) : List ((String × Option String) × (String × String × String)) :=
  (links.foldl (fun acc l =>
      if l.role == .models && l.file.endsWith ".lean" && !(acc.any (·.1 == (l.reqId, l.clause)))
      then acc ++ [((l.reqId, l.clause), (l.symbol, l.file, l.bodyHash))] else acc) []).mergeSort
    (fun a b => clauseLe a.1 b.1)

def addClause (out : List ModelPlan) (entry : (String × Option String) × (String × String × String)) :
    List ModelPlan :=
  let (clause, model, file, hash) := entry
  if out.any (·.model == model) then
    out.map (fun p => if p.model == model then { p with clauses := p.clauses ++ [clause] } else p)
  else out ++ [⟨model, file, hash, [clause], [], [], ""⟩]

def addClaim (mapped : List ((String × Option String) × (String × String × String)))
    (out : List ModelPlan) (l : CheckLink) : List ModelPlan :=
  if l.role == .proves || l.role == .pins then
    match mapped.find? (·.1 == (l.reqId, l.clause)) with
    | none => out
    | some (_, model, _, _) =>
      let claimed : Claimed := ⟨l.symbol, l.reqId, l.clause, l.file, l.line, l.bodyHash⟩
      out.map (fun p =>
        if p.model != model then p
        else if l.role == .proves then
          if p.theorems.contains claimed then p else { p with theorems := p.theorems ++ [claimed] }
        else if p.pins.contains claimed then p else { p with pins := p.pins ++ [claimed] })
  else out

/-- @models REQ-STRENGTH.strength_per_model -/
def plans (links : List CheckLink) : List ModelPlan :=
  let mapped := modelOf links
  let grouped := mapped.foldl addClause []
  let claimed := links.foldl (addClaim mapped) grouped
  (claimed.map (fun p => { p with theorems := p.theorems.mergeSort claimedLe,
                                  pins := p.pins.mergeSort claimedLe })).mergeSort
    (fun a b => decide (a.model ≤ b.model))

theorem axiom_step_error (rest : List (String × Bool)) (e : Refusal) :
    rest.foldl axiomStep (.error e) = .error e := by
  induction rest with
  | nil => rfl
  | cons a rest ih => simpa [List.foldl, axiomStep] using ih

theorem not_own (a : String) (h : ¬ leanOwn.contains a = true) : ¬ a ∈ leanOwn := by
  simpa using h

theorem axiom_fold_kept (axioms : List (String × Bool)) :
    ∀ kept result, axioms.foldl axiomStep (.ok kept) = .ok result →
      ∀ x, x ∈ result ↔ x ∈ kept ∨ ∃ a ∈ axioms, a.1 = x ∧ ¬ x ∈ leanOwn ∧ a.2 = true := by
  induction axioms with
  | nil =>
    intro kept result h x
    simp only [List.foldl, Except.ok.injEq] at h
    subst h
    simp
  | cons a rest ih =>
    intro kept result h x
    rw [List.foldl, axiomStep.eq_2] at h
    by_cases hs : (a.1 == "sorryAx") = true
    · rw [if_pos hs, axiom_step_error] at h
      cases h
    · rw [if_neg hs] at h
      by_cases ho : leanOwn.contains a.1 = true
      · rw [if_pos ho] at h
        rw [ih kept result h x]
        constructor
        · rintro (k | ⟨b, hb, e, n, d⟩)
          · exact Or.inl k
          · exact Or.inr ⟨b, List.mem_cons_of_mem _ hb, e, n, d⟩
        · rintro (k | ⟨b, hb, e, n, d⟩)
          · exact Or.inl k
          · rcases List.mem_cons.mp hb with rfl | hb
            · subst e
              exact absurd (by simpa using ho) n
            · exact Or.inr ⟨b, hb, e, n, d⟩
      · rw [if_neg ho] at h
        by_cases hd : a.2 = true
        · rw [if_neg (by simp [hd])] at h
          rw [ih (kept ++ [a.1]) result h x]
          constructor
          · rintro (k | ⟨b, hb, e, n, d⟩)
            · rcases List.mem_append.mp k with k | k
              · exact Or.inl k
              · have : x = a.1 := List.mem_singleton.mp k
                subst this
                exact Or.inr ⟨a, List.mem_cons_self _ _, rfl, not_own _ ho, hd⟩
            · exact Or.inr ⟨b, List.mem_cons_of_mem _ hb, e, n, d⟩
          · rintro (k | ⟨b, hb, e, n, d⟩)
            · exact Or.inl (List.mem_append_left _ k)
            · rcases List.mem_cons.mp hb with rfl | hb
              · exact Or.inl (List.mem_append_right _ (List.mem_singleton.mpr e.symm))
              · exact Or.inr ⟨b, hb, e, n, d⟩
        · rw [if_pos (by simpa using hd), axiom_step_error] at h
          cases h

theorem axiom_fold_refuses (axioms : List (String × Bool)) :
    ∀ kept, (∃ a ∈ axioms, a.1 = "sorryAx" ∨ (¬ a.1 ∈ leanOwn ∧ a.2 = false)) →
      ∃ e, axioms.foldl axiomStep (.ok kept) = .error e := by
  induction axioms with
  | nil => intro kept ⟨a, ha, _⟩; cases ha
  | cons a rest ih =>
    intro kept ⟨b, hb, hbad⟩
    rw [List.foldl, axiomStep.eq_2]
    by_cases hs : (a.1 == "sorryAx") = true
    · rw [if_pos hs]
      exact ⟨_, axiom_step_error _ _⟩
    · rw [if_neg hs]
      have hs' : a.1 ≠ "sorryAx" := by simpa using hs
      by_cases ho : leanOwn.contains a.1 = true
      · rw [if_pos ho]
        rcases List.mem_cons.mp hb with rfl | hb
        · rcases hbad with h | ⟨n, _⟩
          · exact absurd h hs'
          · exact absurd (by simpa using ho) n
        · exact ih kept ⟨b, hb, hbad⟩
      · rw [if_neg ho]
        by_cases hd : a.2 = true
        · rw [if_neg (by simp [hd])]
          rcases List.mem_cons.mp hb with rfl | hb
          · rcases hbad with h | ⟨_, d⟩
            · exact absurd h hs'
            · rw [d] at hd
              cases hd
          · exact ih _ ⟨b, hb, hbad⟩
        · rw [if_pos (by simpa using hd)]
          exact ⟨_, axiom_step_error _ _⟩

theorem verdict_accepted (facts : ProofFacts) (assumed : List String)
    (h : verdict facts = .accepted assumed) :
    facts.found = true ∧ facts.isType = false ∧ facts.mentions = true ∧ facts.general = true ∧
      facts.leak = none ∧ facts.ignores = false ∧ facts.closes = false ∧
      axiomVerdict facts.axioms = .ok assumed := by
  unfold verdict at h
  by_cases hf : facts.found = true
  · simp only [hf, Bool.not_true, Bool.false_eq_true, if_false] at h
    by_cases ht : facts.isType = true
    · simp [ht] at h
    · simp only [ht, Bool.false_eq_true, if_false] at h
      cases ha : axiomVerdict facts.axioms with
      | error e => simp [ha] at h
      | ok kept =>
        simp only [ha] at h
        by_cases hm : facts.mentions = true
        · simp only [hm, Bool.not_true, Bool.false_eq_true, if_false] at h
          by_cases hg : facts.general = true
          · simp only [hg, Bool.not_true, Bool.false_eq_true, if_false] at h
            cases hl : facts.leak with
            | some v => simp [hl] at h
            | none =>
              simp only [hl] at h
              by_cases hv : (facts.ignores || facts.closes) = true
              · simp [hv] at h
              · simp only [hv, Bool.false_eq_true, if_false, Verdict.accepted.injEq] at h
                simp only [Bool.or_eq_true, not_or, Bool.not_eq_true] at hv
                exact ⟨hf, by simpa using ht, hm, hg, rfl, hv.1, hv.2, by rw [h]⟩
          · simp [hg] at h
        · simp [hm] at h
  · simp [hf] at h

/-- @proves REQ-STRENGTH.proof_states_the_model -/
theorem a_proof_that_never_applies_the_model_earns_nothing (facts : ProofFacts)
    (silent : facts.mentions = false) : (verdict facts).isAccepted = false := by
  cases h : verdict facts with
  | refused _ => rfl
  | accepted a => simp [(verdict_accepted facts a h).2.2.1] at silent

/-- @proves REQ-STRENGTH.proof_is_general -/
theorem an_example_earns_nothing (facts : ProofFacts) (fixed : facts.general = false) :
    (verdict facts).isAccepted = false := by
  cases h : verdict facts with
  | refused _ => rfl
  | accepted a => simp [(verdict_accepted facts a h).2.2.2.1] at fixed

/-- @proves REQ-STRENGTH.proof_is_direct -/
theorem a_proof_through_another_definition_names_it (facts : ProofFacts) (via : String)
    (found : facts.found = true) (fn : facts.isType = false) (clean : ∃ kept, axiomVerdict facts.axioms = .ok kept)
    (states : facts.mentions = true) (general : facts.general = true) (leak : facts.leak = some via) :
    verdict facts = .refused (.indirect via) := by
  obtain ⟨kept, hk⟩ := clean
  simp [verdict, found, fn, hk, states, general, leak]

/-- @proves REQ-STRENGTH.proof_not_vacuous -/
theorem a_proof_that_holds_of_every_function_earns_nothing (facts : ProofFacts)
    (vacuous : facts.ignores = true ∨ facts.closes = true) : (verdict facts).isAccepted = false := by
  cases h : verdict facts with
  | refused _ => rfl
  | accepted a =>
    have v := verdict_accepted facts a h
    rcases vacuous with i | c
    · simp [v.2.2.2.2.2.1] at i
    · simp [v.2.2.2.2.2.2.1] at c

/-- @proves REQ-STRENGTH.model_is_a_function -/
theorem a_type_is_judged_not_proved (facts : ProofFacts) (found : facts.found = true)
    (isType : facts.isType = true) : verdict facts = .refused .type := by
  simp [verdict, found, isType]

/-- @proves REQ-STRENGTH.axioms_declared -/
theorem only_lean_axioms_and_declared_ones_are_accepted (facts : ProofFacts)
    (bad : ∃ a ∈ facts.axioms, a.1 = "sorryAx" ∨ (¬ a.1 ∈ leanOwn ∧ a.2 = false)) :
    (verdict facts).isAccepted = false := by
  cases h : verdict facts with
  | refused _ => rfl
  | accepted a =>
    have v := (verdict_accepted facts a h).2.2.2.2.2.2.2
    obtain ⟨e, he⟩ := axiom_fold_refuses facts.axioms [] bad
    simp [axiomVerdict, he] at v

/-- @proves REQ-STRENGTH.assumptions_shown -/
theorem an_accepted_proof_lists_every_project_axiom_it_rests_on (facts : ProofFacts)
    (assumed : List String) (h : verdict facts = .accepted assumed) (x : String) :
    x ∈ assumed ↔ ∃ a ∈ facts.axioms, a.1 = x ∧ ¬ x ∈ leanOwn ∧ a.2 = true := by
  have v := (verdict_accepted facts assumed h).2.2.2.2.2.2.2
  unfold axiomVerdict at v
  cases hf : facts.axioms.foldl axiomStep (.ok []) with
  | error e => simp [hf] at v
  | ok kept =>
    simp only [hf, Except.ok.injEq] at v
    subst v
    rw [List.mem_mergeSort, axiom_fold_kept facts.axioms [] kept hf x]
    simp

/-- @proves REQ-STRENGTH.pinned_by_theorem -/
theorem a_pin_counts_only_when_it_states_the_obligation (facts : PinFacts) (assumed : List String)
    (h : pinVerdict facts = .accepted assumed) :
    facts.found = true ∧ facts.statement = true ∧ axiomVerdict facts.axioms = .ok assumed := by
  unfold pinVerdict at h
  by_cases hf : facts.found = true
  · simp only [hf, Bool.not_true, Bool.false_eq_true, if_false] at h
    cases ha : axiomVerdict facts.axioms with
    | error e => simp [ha] at h
    | ok kept =>
      simp only [ha] at h
      by_cases hs : facts.statement = true
      · simp only [hs, Bool.not_true, Bool.false_eq_true, if_false, Verdict.accepted.injEq] at h
        exact ⟨hf, hs, by rw [h]⟩
      · simp [hs] at h
  · simp [hf] at h

theorem pinnedNow_some (held : Option (String × List String)) (keyNow : String) (pins : List String)
    (t : String) (h : pinnedNow held keyNow pins = some t) :
    t ∈ pins ∧ ∃ acc, held = some (keyNow, acc) ∧ t ∈ acc := by
  cases held with
  | none => simp [pinnedNow] at h
  | some p =>
    obtain ⟨key, acc⟩ := p
    simp only [pinnedNow] at h
    by_cases hk : key = keyNow
    · rw [if_pos hk] at h
      subst hk
      exact ⟨by simpa using List.find?_some h, acc, rfl, List.mem_of_find?_eq_some h⟩
    · rw [if_neg hk] at h
      cases h

/-- @proves REQ-STRENGTH.open_is_the_default -/
theorem with_no_pinning_theorem_a_model_is_open (held : Option (String × List String))
    (keyNow : String) : standing held keyNow [] none = .«open» := by
  cases h : pinnedNow held keyNow [] with
  | none => simp [standing, h]
  | some t => exact absurd (pinnedNow_some held keyNow [] t h).1 (List.not_mem_nil t)

/-- @proves REQ-STRENGTH.attempted_distinguished -/
theorem a_pin_never_accepted_is_attempted (held : Option (String × List String)) (keyNow t : String)
    (rest : List String) (never : ∀ acc, held = some (keyNow, acc) → ∀ p ∈ acc, ¬ p ∈ t :: rest) :
    standing held keyNow (t :: rest) none = .attempted t := by
  cases h : pinnedNow held keyNow (t :: rest) with
  | none => simp [standing, h]
  | some q =>
    obtain ⟨hq, acc, hh, ha⟩ := pinnedNow_some held keyNow _ q h
    exact absurd hq (never acc hh q ha)

/-- @proves REQ-STRENGTH.nondeterministic_declared -/
theorem a_declared_nondeterministic_model_is_excluded (held : Option (String × List String))
    (keyNow reason : String) (pins : List String) :
    standing held keyNow pins (some reason) = .nondeterministic reason := rfl

/-- @proves REQ-STRENGTH.verdict_kept -/
theorem a_verdict_about_other_sources_never_pins (held : Option (String × List String))
    (keyNow : String) (pins : List String)
    (changed : ∀ acc, held ≠ some (keyNow, acc)) (t : String) :
    standing held keyNow pins none ≠ .pinned t := by
  cases h : pinnedNow held keyNow pins with
  | none => cases pins <;> simp [standing, h]
  | some q =>
    obtain ⟨_, acc, hh, _⟩ := pinnedNow_some held keyNow pins q h
    exact absurd hh (changed acc)

/-- @proves REQ-STRENGTH.qualifies_proof -/
theorem a_pinned_model_names_a_pin_it_was_checked_with (held : Option (String × List String))
    (keyNow : String) (pins : List String) (t : String)
    (h : standing held keyNow pins none = .pinned t) :
    t ∈ pins ∧ ∃ acc, held = some (keyNow, acc) ∧ t ∈ acc := by
  cases hp : pinnedNow held keyNow pins with
  | none => cases pins <;> simp [standing, hp] at h
  | some q =>
    simp only [standing, hp, Strength.pinned.injEq] at h
    subst h
    exact pinnedNow_some held keyNow pins q hp

/-- @proves REQ-STRENGTH.obligation_generated -/
theorem the_obligation_is_built_from_exactly_the_accepted_statements (heard : List Heard)
    (model thm : String) :
    (model, thm) ∈ accepted heard ↔
      ∃ facts, Heard.facts thm model facts ∈ heard ∧ (verdict facts).isAccepted = true := by
  unfold accepted
  rw [List.mem_filterMap]
  constructor
  · rintro ⟨h, hm, he⟩
    cases h with
    | facts t m f =>
      simp only [heardAccepted] at he
      by_cases ha : (verdict f).isAccepted = true
      · simp only [ha, if_true, Option.some.injEq, Prod.mk.injEq] at he
        obtain ⟨rfl, rfl⟩ := he
        exact ⟨f, hm, ha⟩
      · simp [ha] at he
    | _ => simp [heardAccepted] at he
  · rintro ⟨f, hm, ha⟩
    exact ⟨_, hm, by simp [heardAccepted, ha]⟩

theorem addClause_models (out : List ModelPlan) (entry : (String × Option String) × (String × String × String))
    (p : ModelPlan) (hp : p ∈ addClause out entry) :
    (∃ q ∈ out, q.model = p.model ∧ ∀ c ∈ p.clauses, c ∈ q.clauses ∨ (c = entry.1 ∧ entry.2.1 = p.model)) ∨
      (p.model = entry.2.1 ∧ p.clauses = [entry.1]) := by
  obtain ⟨clause, model, file, hash⟩ := entry
  unfold addClause at hp
  simp only [] at hp
  split at hp
  · obtain ⟨q, hq, e⟩ := List.mem_map.mp hp
    left
    by_cases hm : q.model = model
    · simp only [hm, beq_self_eq_true, if_true] at e
      subst e
      refine ⟨q, hq, hm, fun c hc => ?_⟩
      simp only [List.mem_append, List.mem_singleton] at hc
      rcases hc with hc | hc
      · exact Or.inl hc
      · exact Or.inr ⟨hc, rfl⟩
    · have : (q.model == model) = false := by simpa using hm
      simp only [this, Bool.false_eq_true, if_false] at e
      subst e
      exact ⟨q, hq, rfl, fun c hc => Or.inl hc⟩
  · simp only [List.mem_append, List.mem_singleton] at hp
    rcases hp with hp | rfl
    · exact Or.inl ⟨p, hp, rfl, fun c hc => Or.inl hc⟩
    · exact Or.inr ⟨rfl, rfl⟩

theorem grouped_clauses_are_modelled (mapped : List ((String × Option String) × (String × String × String))) :
    ∀ (out : List ModelPlan),
      (∀ p ∈ out, ∀ c ∈ p.clauses, ∃ e ∈ mapped, e.1 = c ∧ e.2.1 = p.model) →
      ∀ (rest : List ((String × Option String) × (String × String × String))),
        (∀ e ∈ rest, e ∈ mapped) →
        ∀ p ∈ rest.foldl addClause out, ∀ c ∈ p.clauses, ∃ e ∈ mapped, e.1 = c ∧ e.2.1 = p.model := by
  intro out hout rest
  induction rest generalizing out with
  | nil => intro _; simpa using hout
  | cons entry rest ih =>
    intro hrest
    apply ih
    · intro p hp c hc
      rcases addClause_models out entry p hp with ⟨q, hq, hm, hcl⟩ | ⟨hm, hcs⟩
      · rcases hcl c hc with hc | ⟨rfl, he⟩
        · obtain ⟨e, he, e1, e2⟩ := hout q hq c hc
          exact ⟨e, he, e1, by rw [e2, hm]⟩
        · exact ⟨entry, hrest entry (List.mem_cons_self _ _), rfl, he⟩
      · rw [hcs] at hc
        simp only [List.mem_singleton] at hc
        subst hc
        exact ⟨entry, hrest entry (List.mem_cons_self _ _), rfl, hm.symm⟩
    · exact fun e he => hrest e (List.mem_cons_of_mem _ he)

theorem addClaim_keeps (mapped : List ((String × Option String) × (String × String × String)))
    (out : List ModelPlan) (l : CheckLink) (p : ModelPlan) (hp : p ∈ addClaim mapped out l) :
    ∃ q ∈ out, q.model = p.model ∧ q.clauses = p.clauses := by
  unfold addClaim at hp
  split at hp
  · split at hp
    · exact ⟨p, hp, rfl, rfl⟩
    · obtain ⟨q, hq, e⟩ := List.mem_map.mp hp
      refine ⟨q, hq, ?_⟩
      split at e
      · subst e; exact ⟨rfl, rfl⟩
      · split at e
        · split at e <;> (subst e; exact ⟨rfl, rfl⟩)
        · split at e <;> (subst e; exact ⟨rfl, rfl⟩)
  · exact ⟨p, hp, rfl, rfl⟩

theorem modelOf_models (links : List CheckLink) (e : (String × Option String) × (String × String × String))
    (he : e ∈ modelOf links) :
    ∃ l ∈ links, l.role = .models ∧ (l.reqId, l.clause) = e.1 ∧ l.symbol = e.2.1 := by
  unfold modelOf at he
  rw [List.mem_mergeSort] at he
  have step : ∀ (ls : List CheckLink) acc,
      (∀ x ∈ acc, ∃ l ∈ links, l.role = .models ∧ (l.reqId, l.clause) = x.1 ∧ l.symbol = x.2.1) →
      (∀ l ∈ ls, l ∈ links) →
      ∀ x ∈ ls.foldl (fun acc l =>
          if l.role == .models && l.file.endsWith ".lean" && !(acc.any (·.1 == (l.reqId, l.clause)))
          then acc ++ [((l.reqId, l.clause), (l.symbol, l.file, l.bodyHash))] else acc) acc,
        ∃ l ∈ links, l.role = .models ∧ (l.reqId, l.clause) = x.1 ∧ l.symbol = x.2.1 := by
    intro ls
    induction ls with
    | nil => intro acc hacc _; simpa using hacc
    | cons l rest ih =>
      intro acc hacc hls
      simp only [List.foldl]
      apply ih _ _ (fun x hx => hls x (List.mem_cons_of_mem _ hx))
      intro x hx
      split at hx
      · rename_i hc
        simp only [List.mem_append, List.mem_singleton] at hx
        rcases hx with hx | rfl
        · exact hacc x hx
        · simp only [Bool.and_eq_true, beq_iff_eq] at hc
          exact ⟨l, hls l (List.mem_cons_self _ _), hc.1.1, rfl, rfl⟩
      · exact hacc x hx
  exact step links [] (fun x hx => by cases hx) (fun l hl => hl) e he

/-- @proves REQ-STRENGTH.strength_per_model -/
theorem every_planned_clause_is_modelled_by_its_plan (links : List CheckLink) (p : ModelPlan)
    (hp : p ∈ plans links) (c : String × Option String) (hc : c ∈ p.clauses) :
    ∃ l ∈ links, l.role = .models ∧ (l.reqId, l.clause) = c ∧ l.symbol = p.model := by
  unfold plans at hp
  simp only [] at hp
  rw [List.mem_mergeSort] at hp
  obtain ⟨q, hq, rfl⟩ := List.mem_map.mp hp
  have claims : ∀ (ls : List CheckLink) (out : List ModelPlan) (r : ModelPlan),
      r ∈ ls.foldl (addClaim (modelOf links)) out → ∃ q ∈ out, q.model = r.model ∧ q.clauses = r.clauses := by
    intro ls
    induction ls with
    | nil => intro out r hr; exact ⟨r, hr, rfl, rfl⟩
    | cons l rest ih =>
      intro out r hr
      obtain ⟨m, hm, e1, e2⟩ := ih _ r hr
      obtain ⟨q, hq, f1, f2⟩ := addClaim_keeps (modelOf links) out l m hm
      exact ⟨q, hq, f1.trans e1, f2.trans e2⟩
  obtain ⟨g, hg, e1, e2⟩ := claims links _ q hq
  have grouped := grouped_clauses_are_modelled (modelOf links) [] (fun _ h => by cases h)
    (modelOf links) (fun e he => he) g hg c (by rw [e2]; exact hc)
  obtain ⟨e, he, ec, em⟩ := grouped
  obtain ⟨l, hl, hr, hk, hs⟩ := modelOf_models links e he
  exact ⟨l, hl, hr, hk.trans ec, hs.trans (em.trans e1)⟩

end TraceLean.Proofcheck
