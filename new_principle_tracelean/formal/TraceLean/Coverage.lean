import TraceLean.Evidence
import TraceLean.Generator
import Lean

namespace TraceLean.Coverage

open TraceLean.Evidence
open TraceLean.Generator (Schema valueStream)

open Lean (Json ToJson FromJson)

structure Floor where
  situation : String
  atLeast : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Observed where
  situation : String
  reached : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Waiver where
  situations : List String
  reason : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Gap where
  | vacuous (situation : String)
  | short (situation : String) (reached atLeast : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Gap.situation : Gap → String
  | .vacuous s => s
  | .short s _ _ => s

inductive CoverageVerdict where
  | met
  | unmet (gaps : List Gap)
  | undeclared
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def reachedCount (observed : List Observed) (situation : String) : Nat :=
  ((observed.find? (·.situation == situation)).map (·.reached)).getD 0

def gapOf (observed : List Observed) (floor : Floor) : Option Gap :=
  let count := reachedCount observed floor.situation
  if count == 0 then some (Gap.vacuous floor.situation)
  else if count < floor.atLeast then some (Gap.short floor.situation count floor.atLeast)
  else none

def blank (s : String) : Bool := s.trim.isEmpty

def excused (waivers : List Waiver) : List String :=
  (waivers.filter (fun w => !blank w.reason)).bind (·.situations)

/-- @models REQ-DRT-COVER.floor_stated
@models REQ-DRT-COVER.law_coverage
@partial reason="per-law floors over a law's precondition are not designed; a binding's named situations stand in for them"
@models REQ-DRT-COVER.vacuous_named
@models REQ-DRT-COVER.waiver_reasoned -/
def coverageVerdict (floors : List Floor) (observed : List Observed) (waivers : List Waiver)
    : CoverageVerdict :=
  match floors with
  | [] => .undeclared
  | _ =>
    let gaps := (floors.filterMap (gapOf observed)).filter
      (fun g => !(excused waivers).contains g.situation)
    if gaps.isEmpty then .met else .unmet gaps

def waiverUsed (floors : List Floor) (observed : List Observed) (name : String) : Bool :=
  match floors.find? (fun f => f.situation == name) with
  | none => false
  | some floor => (gapOf observed floor).isSome

/-- @models REQ-DRT-COVER.waiver_unused_reported -/
def unusedWaivers (floors : List Floor) (observed : List Observed) (waivers : List Waiver)
    : List String :=
  (waivers.bind (fun w =>
    if blank w.reason then w.situations
    else w.situations.filter (fun s => !waiverUsed floors observed s))).eraseDups

def className (path label : String) : String :=
  if path == "" then label else path ++ " " ++ label

def memberPath (path key : String) : String :=
  if path == "" then key else path ++ "." ++ key

partial def classesAt (path : String) (schema : Schema) : List String :=
  match schema with
  | .nat max edges =>
    [className path "zero"]
      ++ (if max.getD 1000 > 0 || edges.any (· > 0) then [className path "positive"] else [])
  | .int min max =>
    let lo := min.getD (-1000)
    let top := lo + Int.ofNat ((max.getD 1000) - lo).natAbs
    (if lo < 0 then [className path "negative"] else [])
      ++ (if lo ≤ 0 && 0 ≤ top then [className path "zero"] else [])
      ++ (if 0 < top then [className path "positive"] else [])
  | .bool => [className path "true", className path "false"]
  | .str maxLen examples =>
    [className path "empty"]
      ++ (if maxLen.getD 12 > 0 || examples.any (· != "") then [className path "not empty"] else [])
  | .option inner =>
    [className path "absent", className path "present"] ++ classesAt (path ++ "?") inner
  | .list inner maxLen =>
    if maxLen.getD 6 > 0 then
      [className path "empty", className path "not empty"] ++ classesAt (path ++ "[]") inner
    else [className path "empty"]
  | .struct fields => fields.bind (fun f => classesAt (memberPath path f.1) f.2)
  | .tuple items =>
    items.enum.bind (fun p => classesAt (memberPath path (toString p.1)) p.2)
  | .enum variants =>
    variants.bind (fun v =>
      [className path ("is " ++ v.1)]
        ++ (v.2.map (classesAt (memberPath path v.1))).getD [])

def signClass (path : String) (n : Int) : String :=
  if n < 0 then className path "negative"
  else if n == 0 then className path "zero"
  else className path "positive"

partial def classesOf (path : String) (schema : Schema) (value : Json) : List String :=
  match schema with
  | .nat _ _ => ((value.getInt?.toOption).map (fun n => [signClass path n])).getD []
  | .int _ _ => ((value.getInt?.toOption).map (fun n => [signClass path n])).getD []
  | .bool =>
    match value with
    | .bool true => [className path "true"]
    | _ => [className path "false"]
  | .str _ _ =>
    match value with
    | .str "" => [className path "empty"]
    | _ => [className path "not empty"]
  | .option inner =>
    match value with
    | .null => [className path "absent"]
    | _ => className path "present" :: classesOf (path ++ "?") inner value
  | .list inner _ =>
    match value with
    | .arr items =>
      if items.isEmpty then [className path "empty"]
      else className path "not empty" :: items.toList.bind (classesOf (path ++ "[]") inner)
    | _ => [className path "not empty"]
  | .struct fields =>
    fields.bind (fun f => classesOf (memberPath path f.1) f.2 (value.getObjValD f.1))
  | .tuple items =>
    items.enum.bind (fun p =>
      classesOf (memberPath path (toString p.1)) p.2 ((value.getArrVal? p.1).toOption.getD Json.null))
  | .enum variants =>
    match value with
    | .str name => [className path ("is " ++ name)]
    | .obj _ =>
      match TraceLean.Generator.members value with
      | [] => []
      | chosen :: _ =>
        let payload := (variants.find? (fun v => v.1 == chosen.1)).bind (·.2)
        className path ("is " ++ chosen.1)
          :: (payload.map (fun s => classesOf (memberPath path chosen.1) s chosen.2)).getD []
    | _ => []

def reachedClasses (schema : Schema) (seed : Nat) (cases : Nat) : List Observed :=
  let hits := (valueStream seed schema cases).map (classesOf "" schema)
  (classesAt "" schema).map (fun c => Observed.mk c (hits.filter (fun h => h.contains c)).length)

/-- @models REQ-DRT-COVER.classes_reached -/
def reachedShape (shape : Nat) (seed : Nat) (cases : Nat) : List Observed :=
  match TraceLean.Generator.shapes.length with
  | 0 => []
  | n + 1 => reachedClasses (TraceLean.Generator.shapes.getD (shape % (n + 1)) (.bool)) seed cases

def addLine (acc : List Observed) (name : String) (count : Nat) : List Observed :=
  if acc.any (·.situation == name) then
    acc.map (fun o => if o.situation == name then Observed.mk name (min o.reached count) else o)
  else acc ++ [Observed.mk name count]

def lineStep (hits : List (Nat × Nat)) (acc : List Observed) (line : Nat × String)
    : List Observed :=
  match hits.find? (fun h => h.1 == line.1) with
  | none => acc
  | some h => addLine acc ("line: " ++ line.2.trim) h.2

/-- @models REQ-DRT-COVER.lines_run -/
def lineReach (hits : List (Nat × Nat)) (item : List (Nat × String)) : List Observed :=
  item.foldl (lineStep hits) []

/-- @models REQ-DRT-COVER.floor_unmet_is_not_pass -/
def coverageLevel (agreed : Bool) (verdict : CoverageVerdict) : Level :=
  match agreed, verdict with
  | true, .met => Level.L3
  | _, _ => Level.L1

/-- @proves REQ-DRT-COVER.floor_unmet_is_not_pass -/
theorem only_a_covered_agreement_is_evidence (agreed : Bool) (v : CoverageVerdict) :
    coverageLevel agreed v = Level.L3 → agreed = true ∧ v = CoverageVerdict.met := by
  intro h
  cases agreed <;> cases v <;> simp_all [coverageLevel]

theorem gapOf_situation {o : List Observed} {f : Floor} {g : Gap} (h : gapOf o f = some g) :
    g.situation = f.situation := by
  unfold gapOf at h
  simp only at h
  split at h
  · cases h; rfl
  · split at h
    · cases h; rfl
    · cases h

theorem gapOf_none {o : List Observed} {f : Floor} :
    gapOf o f = none ↔ 0 < reachedCount o f.situation ∧ f.atLeast ≤ reachedCount o f.situation := by
  unfold gapOf
  by_cases h0 : reachedCount o f.situation = 0
  · simp [h0]
  · by_cases h1 : reachedCount o f.situation < f.atLeast
    · simp [h0, h1]
    · simp [h0, h1]
      omega

theorem verdict_cons (f : Floor) (fs : List Floor) (o : List Observed) (w : List Waiver) :
    coverageVerdict (f :: fs) o w =
      (if (((f :: fs).filterMap (gapOf o)).filter (fun g => !(excused w).contains g.situation)).isEmpty
        then .met else .unmet (((f :: fs).filterMap (gapOf o)).filter
          (fun g => !(excused w).contains g.situation))) := rfl

/-- @proves REQ-DRT-COVER.floor_stated -/
theorem undeclared_exactly_when_no_floor_is_stated (floors : List Floor) (o : List Observed)
    (w : List Waiver) : coverageVerdict floors o w = .undeclared ↔ floors = [] := by
  cases floors with
  | nil => simp [coverageVerdict]
  | cons f fs =>
    rw [verdict_cons]
    split <;> simp

/-- @proves REQ-DRT-COVER.law_coverage -/
theorem a_met_verdict_reached_every_floor (floors : List Floor) (o : List Observed)
    (w : List Waiver) (h : coverageVerdict floors o w = .met) :
    ∀ f ∈ floors, f.situation ∈ excused w ∨
      (0 < reachedCount o f.situation ∧ f.atLeast ≤ reachedCount o f.situation) := by
  intro f hf
  cases floors with
  | nil => cases hf
  | cons f0 fs =>
    rw [verdict_cons] at h
    split at h
    · rename_i hempty
      by_cases hx : f.situation ∈ excused w
      · exact Or.inl hx
      · refine Or.inr (gapOf_none.mp ?_)
        cases hg : gapOf o f with
        | none => rfl
        | some g =>
          exfalso
          have hgs := gapOf_situation hg
          have hmem : g ∈ ((f0 :: fs).filterMap (gapOf o)).filter
              (fun g => !(excused w).contains g.situation) := by
            rw [List.mem_filter, List.mem_filterMap]
            exact ⟨⟨f, hf, hg⟩, by simp [hgs, hx]⟩
          rw [List.isEmpty_iff] at hempty
          rw [hempty] at hmem
          cases hmem
    · cases h

/-- @proves REQ-DRT-COVER.vacuous_named -/
theorem an_unreached_floor_is_named_vacuous (floors : List Floor) (o : List Observed)
    (w : List Waiver) (f : Floor) (hf : f ∈ floors) (h0 : reachedCount o f.situation = 0)
    (hx : f.situation ∉ excused w) :
    ∃ gaps, coverageVerdict floors o w = .unmet gaps ∧ Gap.vacuous f.situation ∈ gaps := by
  cases floors with
  | nil => cases hf
  | cons f0 fs =>
    have hg : gapOf o f = some (Gap.vacuous f.situation) := by simp [gapOf, h0]
    have hmem : Gap.vacuous f.situation ∈ ((f0 :: fs).filterMap (gapOf o)).filter
        (fun g => !(excused w).contains g.situation) := by
      rw [List.mem_filter, List.mem_filterMap]
      exact ⟨⟨f, hf, hg⟩, by simp [Gap.situation, hx]⟩
    rw [verdict_cons]
    split
    · rename_i hempty
      rw [List.isEmpty_iff] at hempty
      rw [hempty] at hmem
      cases hmem
    · exact ⟨_, rfl, hmem⟩

theorem mem_eraseDups_loop (x : String) : ∀ (l bs : List String), x ∈ l ∨ x ∈ bs →
    x ∈ List.eraseDups.loop l bs := by
  intro l
  induction l with
  | nil => intro bs h; simp [List.eraseDups.loop]; simpa using h
  | cons a l ih =>
    intro bs h
    unfold List.eraseDups.loop
    split
    · rename_i he
      apply ih
      rcases h with h | h
      · rcases List.mem_cons.mp h with h | h
        · subst h; right; simpa using he
        · exact Or.inl h
      · exact Or.inr h
    · apply ih
      rcases h with h | h
      · rcases List.mem_cons.mp h with h | h
        · subst h; right; exact List.mem_cons_self _ _
        · exact Or.inl h
      · exact Or.inr (List.mem_cons_of_mem _ h)

theorem mem_eraseDups {x : String} {l : List String} (h : x ∈ l) : x ∈ l.eraseDups :=
  mem_eraseDups_loop x l [] (Or.inl h)

/-- @proves REQ-DRT-COVER.waiver_reasoned -/
theorem a_waiver_without_a_reason_does_not_count (floors : List Floor) (o : List Observed)
    (w : Waiver) (ws : List Waiver) (hb : blank w.reason = true) :
    coverageVerdict floors o (w :: ws) = coverageVerdict floors o ws ∧
    ∀ s ∈ w.situations, s ∈ unusedWaivers floors o (w :: ws) := by
  have he : excused (w :: ws) = excused ws := by simp [excused, hb]
  refine ⟨by unfold coverageVerdict; rw [he], ?_⟩
  intro s hs
  unfold unusedWaivers
  apply mem_eraseDups
  rw [List.mem_bind]
  exact ⟨w, List.mem_cons_self _ _, by simp [hb, hs]⟩

/-- @proves REQ-DRT-COVER.waiver_unused_reported -/
theorem a_waiver_over_a_reached_situation_is_unused (floors : List Floor) (o : List Observed)
    (ws : List Waiver) (w : Waiver) (s : String) (hw : w ∈ ws) (hs : s ∈ w.situations)
    (hr : ∀ f ∈ floors, f.situation = s →
      0 < reachedCount o s ∧ f.atLeast ≤ reachedCount o s) :
    s ∈ unusedWaivers floors o ws := by
  have hu : waiverUsed floors o s = false := by
    unfold waiverUsed
    cases hf : floors.find? (fun f => f.situation == s) with
    | none => rfl
    | some f =>
      have h1 := List.find?_some hf
      have h2 := List.mem_of_find?_eq_some hf
      simp at h1
      have : gapOf o f = none := gapOf_none.mpr (by rw [h1]; exact hr f h2 h1)
      simp [this]
  unfold unusedWaivers
  apply mem_eraseDups
  rw [List.mem_bind]
  refine ⟨w, hw, ?_⟩
  split
  · exact hs
  · exact List.mem_filter.mpr ⟨hs, by simp [hu]⟩

theorem reached_classes_listed (schema : Schema) (seed cases : Nat) :
    (reachedClasses schema seed cases).map (·.situation) = classesAt "" schema ∧
    ∀ c ∈ reachedClasses schema seed cases, (c.reached = 0 ↔
      ∀ v ∈ valueStream seed schema cases, c.situation ∉ classesOf "" schema v) := by
  constructor
  · simp [reachedClasses, Function.comp_def]
  · intro c hc
    simp only [reachedClasses, List.mem_map] at hc
    obtain ⟨name, _, he⟩ := hc
    subst he
    simp [List.length_eq_zero, List.filter_eq_nil]

/-- @proves REQ-DRT-COVER.classes_reached -/
theorem every_class_of_the_shape_is_listed_with_its_reach (shape seed cases : Nat) :
    ∃ schema, (reachedShape shape seed cases).map (·.situation) = classesAt "" schema ∧
      ∀ c ∈ reachedShape shape seed cases, (c.reached = 0 ↔
        ∀ v ∈ valueStream seed schema cases, c.situation ∉ classesOf "" schema v) := by
  unfold reachedShape
  split
  · rename_i h
    simp [TraceLean.Generator.shapes] at h
  · exact ⟨_, reached_classes_listed _ seed cases⟩

theorem foldl_preserves {α β : Type} {P : β → Prop} {f : β → α → β}
    (hP : ∀ acc x, P acc → P (f acc x)) : ∀ (l : List α) (init : β), P init → P (l.foldl f init) := by
  intro l
  induction l with
  | nil => intro init h; exact h
  | cons a _ ih => intro init h; exact ih _ (hP _ _ h)

theorem addLine_keeps {acc : List Observed} {s : String} {c : Nat} (name : String) (count : Nat)
    (h : ∃ o ∈ acc, o.situation = s ∧ o.reached ≤ c) :
    ∃ o ∈ addLine acc name count, o.situation = s ∧ o.reached ≤ c := by
  obtain ⟨o, ho, hs, hc⟩ := h
  unfold addLine
  split
  · by_cases hn : o.situation = name
    · refine ⟨Observed.mk name (min o.reached count), ?_, by simp [← hn, hs], ?_⟩
      · rw [List.mem_map]
        exact ⟨o, ho, by simp [hn]⟩
      · simp only
        omega
    · refine ⟨o, ?_, hs, hc⟩
      rw [List.mem_map]
      exact ⟨o, ho, by simp [hn]⟩
  · exact ⟨o, List.mem_append_left _ ho, hs, hc⟩

theorem addLine_adds (acc : List Observed) (name : String) (count : Nat) :
    ∃ o ∈ addLine acc name count, o.situation = name ∧ o.reached ≤ count := by
  unfold addLine
  split
  · rename_i h
    rw [List.any_eq_true] at h
    obtain ⟨o, ho, hs⟩ := h
    simp at hs
    refine ⟨Observed.mk name (min o.reached count), ?_, rfl, by simp only; omega⟩
    rw [List.mem_map]
    exact ⟨o, ho, by simp [hs]⟩
  · exact ⟨Observed.mk name count, by simp, rfl, Nat.le_refl _⟩

/-- @proves REQ-DRT-COVER.lines_run -/
theorem every_measured_line_is_recorded_with_at_most_its_hits (hits : List (Nat × Nat))
    (item : List (Nat × String)) (l : Nat × String) (h : Nat × Nat) (hl : l ∈ item)
    (hh : hits.find? (fun h => h.1 == l.1) = some h) :
    ∃ o ∈ lineReach hits item, o.situation = "line: " ++ l.2.trim ∧ o.reached ≤ h.2 := by
  obtain ⟨pre, post, he⟩ := List.append_of_mem hl
  unfold lineReach
  rw [he, List.foldl_append, List.foldl_cons]
  refine foldl_preserves
    (P := fun (acc : List Observed) => ∃ o ∈ acc, o.situation = "line: " ++ l.2.trim ∧ o.reached ≤ h.2) ?_ post _ ?_
  · intro acc x hacc
    unfold lineStep
    split
    · exact hacc
    · exact addLine_keeps _ _ hacc
  · simp only [lineStep, hh]
    exact addLine_adds _ _ _

end TraceLean.Coverage
