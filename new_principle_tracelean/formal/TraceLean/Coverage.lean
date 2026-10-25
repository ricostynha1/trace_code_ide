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

/-- @proves REQ-DRT-COVER.vacuous_named -/
theorem nothing_reached_is_vacuous_not_short :
    coverageVerdict [{ situation := "deletes", atLeast := 10 }] [] []
      = CoverageVerdict.unmet [Gap.vacuous "deletes"] := by
  native_decide

/-- @proves REQ-DRT-COVER.vacuous_named -/
theorem every_gap_is_reported_and_zero_is_vacuous :
    coverageVerdict [{ situation := "a", atLeast := 5 }, { situation := "b", atLeast := 0 }]
        [{ situation := "a", reached := 2 }] []
      = CoverageVerdict.unmet [Gap.short "a" 2 5, Gap.vacuous "b"] := by
  native_decide

/-- @proves REQ-DRT-COVER.floor_stated -/
theorem no_floor_is_not_a_met_floor :
    coverageVerdict [] [{ situation := "deletes", reached := 900 }] []
      = CoverageVerdict.undeclared := by
  native_decide

/-- @proves REQ-DRT-COVER.waiver_reasoned -/
theorem a_waiver_without_a_reason_excuses_nothing :
    coverageVerdict [{ situation := "x negative", atLeast := 1 }] []
        [{ situations := ["x negative"], reason := " " }]
      = CoverageVerdict.unmet [Gap.vacuous "x negative"]
    ∧ coverageVerdict [{ situation := "x negative", atLeast := 1 }] []
        [{ situations := ["x negative"], reason := "the type admits it, the caller never sends it" }]
      = CoverageVerdict.met := by
  native_decide

/-- @proves REQ-DRT-COVER.waiver_unused_reported -/
theorem a_reached_waiver_is_reported :
    unusedWaivers [{ situation := "x zero", atLeast := 1 }] [{ situation := "x zero", reached := 3 }]
        [{ situations := ["x zero", "line: unreachable!()"], reason := "old" }]
      = ["x zero", "line: unreachable!()"] := by
  native_decide

/-- @proves REQ-DRT-COVER.classes_reached -/
theorem classes_follow_the_bounds :
    classesAt "" (.struct [("n", .int (some 0) (some 5)), ("o", .option .bool)])
      = ["n zero", "n positive", "o absent", "o present", "o? true", "o? false"] := by
  native_decide

/-- @proves REQ-DRT-COVER.lines_run -/
theorem a_repeated_line_not_run_shows :
    lineReach [(3, 5), (7, 0), (9, 2)] [(3, "  x += 1;"), (7, "x += 1;"), (8, "// no"), (9, "}")]
      = [Observed.mk "line: x += 1;" 0, Observed.mk "line: }" 2] := by
  native_decide

/-- @proves REQ-DRT-COVER.floor_unmet_is_not_pass -/
theorem only_a_covered_agreement_is_evidence (agreed : Bool) (v : CoverageVerdict) :
    coverageLevel agreed v = Level.L3 → agreed = true ∧ v = CoverageVerdict.met := by
  intro h
  cases agreed <;> cases v <;> simp_all [coverageLevel]

/-- @proves REQ-DRT-COVER.law_coverage -/
theorem whether_a_situation_was_reached_is_recorded :
    coverageVerdict [⟨"overlap", 1⟩] [⟨"overlap", 4⟩] [] = .met ∧
    coverageVerdict [⟨"overlap", 1⟩, ⟨"empty", 1⟩] [⟨"overlap", 4⟩] []
      = .unmet [.vacuous "empty"] := by
  native_decide

end TraceLean.Coverage
