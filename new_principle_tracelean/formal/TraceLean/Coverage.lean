import TraceLean.Evidence
import TraceLean.Generator
import Lean

/-!
# What a passing run is worth

Models `REQ-DRT-COVER`. A differential run that finds no disagreement says
nothing on its own. It says something once you know which situations its cases
reached: a law about deletions, checked over two thousand cases none of which
deleted anything, is satisfied and vacuous, and reporting that as evidence is
the most comfortable lie this system could tell.

The target is every case that matters (ADR-0017): every class of every argument,
derived from its declared shape; every executable line of the implementing
item; and whatever situations a binding names on top. A waiver excuses a named
class or line, with a reason, and a waiver that excuses nothing is reported.
-/

namespace TraceLean.Coverage

open TraceLean.Evidence
open TraceLean.Generator (Schema valueStream)

open Lean (Json ToJson FromJson)

/-- A situation a binding's runs must reach, and how often. -/
structure Floor where
  situation : String
  atLeast : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- How often a run actually reached a situation. -/
structure Observed where
  situation : String
  reached : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Classes or lines a binding excuses from its floors, and why. -/
structure Waiver where
  situations : List String
  reason : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- One floor a run did not meet. *Vacuous* means no case reached the situation
at all, so the law was never asked its question; *short* means it was asked,
just not often enough. They call for different work — a generator that cannot
produce the case, against one that produces it rarely. -/
inductive Gap where
  | vacuous (situation : String)
  | short (situation : String) (reached atLeast : Nat)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Gap.situation : Gap → String
  | .vacuous s => s
  | .short s _ _ => s

/-- What the coverage of a run amounts to. -/
inductive CoverageVerdict where
  | met
  /-- Every floor not met, in the order the floors were declared. -/
  | unmet (gaps : List Gap)
  /-- No floor at all, so nothing is known about what the run reached. Not the
  same as meeting a floor of zero: one is a claim, the other its absence. -/
  | undeclared
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def reachedCount (observed : List Observed) (situation : String) : Nat :=
  ((observed.find? (·.situation == situation)).map (·.reached)).getD 0

/-- What one floor lacks, if anything. Nothing reached is vacuous whatever the
floor says, so a floor of zero cannot hide a situation no case reached. -/
def gapOf (observed : List Observed) (floor : Floor) : Option Gap :=
  let count := reachedCount observed floor.situation
  if count == 0 then some (Gap.vacuous floor.situation)
  else if count < floor.atLeast then some (Gap.short floor.situation count floor.atLeast)
  else none

/-- The whitespace a reason may consist of and still be no reason. -/
def blank (s : String) : Bool := s.trim.isEmpty

/-- What the waivers excuse: only a waiver with a reason excuses anything. -/
def excused (waivers : List Waiver) : List String :=
  (waivers.filter (fun w => !blank w.reason)).bind (·.situations)

/--
Judge a run's coverage against its floors: the classes of its arguments, the
lines of its implementing item, and the situations its binding names.

Every floor not met is reported, in declaration order, so the message is stable
and complete; a waived one is not held against the run.

@models REQ-DRT-COVER.floor_stated
@models REQ-DRT-COVER.law_coverage
@partial reason="per-law floors over a law's precondition are not designed; a binding's named situations stand in for them"
@models REQ-DRT-COVER.vacuous_named
@models REQ-DRT-COVER.waiver_reasoned
-/
def coverageVerdict (floors : List Floor) (observed : List Observed) (waivers : List Waiver)
    : CoverageVerdict :=
  match floors with
  | [] => .undeclared
  | _ =>
    let gaps := (floors.filterMap (gapOf observed)).filter
      (fun g => !(excused waivers).contains g.situation)
    if gaps.isEmpty then .met else .unmet gaps

/-- Whether a waived name excuses something this run did not meet. -/
def waiverUsed (floors : List Floor) (observed : List Observed) (name : String) : Bool :=
  match floors.find? (fun f => f.situation == name) with
  | none => false
  | some floor => (gapOf observed floor).isSome

/--
The waived names that excuse nothing: reached now, naming no floor this run
measured, or given without a reason. Reported, so waivers cannot pile up.

@models REQ-DRT-COVER.waiver_unused_reported
-/
def unusedWaivers (floors : List Floor) (observed : List Observed) (waivers : List Waiver)
    : List String :=
  (waivers.bind (fun w =>
    if blank w.reason then w.situations
    else w.situations.filter (fun s => !waiverUsed floors observed s))).eraseDups

/-! ## Classes

A class is a part of an argument's domain a run must reach: below, at and above
zero; empty and not; absent and present; each truth; each enum case; and the
same, recursively, for every field, element and payload. Only classes the
declared bounds can produce are derived — a `Nat` capped at zero has no positive
class — so a class not reached is the generator's fault, never the schema's. -/

/-- A class's name: the path to the value, then what it is. -/
def className (path label : String) : String :=
  if path == "" then label else path ++ " " ++ label

/-- The path to a member of the value at `path`. -/
def memberPath (path key : String) : String :=
  if path == "" then key else path ++ "." ++ key

/-- Every class a value of `schema` at `path` can fall in. -/
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

/-- The class of a number, by its sign. -/
def signClass (path : String) (n : Int) : String :=
  if n < 0 then className path "negative"
  else if n == 0 then className path "zero"
  else className path "positive"

/-- Which classes one value of `schema` at `path` is in. -/
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

/--
How many of the first `cases` cases a seed draws reach each class of the
schema's arguments, in the order the classes are derived.
-/
def reachedClasses (schema : Schema) (seed : Nat) (cases : Nat) : List Observed :=
  let hits := (valueStream seed schema cases).map (classesOf "" schema)
  (classesAt "" schema).map (fun c => Observed.mk c (hits.filter (fun h => h.contains c)).length)

/--
`reachedClasses` over the `shape`-th of the shapes a seeded stream is checked
over (`Generator.shapes`, counted round): every class of every kind of schema.

@models REQ-DRT-COVER.classes_reached
-/
def reachedShape (shape : Nat) (seed : Nat) (cases : Nat) : List Observed :=
  match TraceLean.Generator.shapes.length with
  | 0 => []
  | n + 1 => reachedClasses (TraceLean.Generator.shapes.getD (shape % (n + 1)) (.bool)) seed cases

/-! ## Lines -/

/-- Add one executable line's count under its name; a name already present
keeps the smaller count, so a repeated line not run still shows as not run. -/
def addLine (acc : List Observed) (name : String) (count : Nat) : List Observed :=
  if acc.any (·.situation == name) then
    acc.map (fun o => if o.situation == name then Observed.mk name (min o.reached count) else o)
  else acc ++ [Observed.mk name count]

def lineStep (hits : List (Nat × Nat)) (acc : List Observed) (line : Nat × String)
    : List Observed :=
  match hits.find? (fun h => h.1 == line.1) with
  | none => acc
  | some h => addLine acc ("line: " ++ line.2.trim) h.2

/--
How often the differential cases ran each executable line of an implementing
item: `hits` is the measured `(line, count)` of its file, `item` its lines with
their text. A line is named by its text, so a waiver survives the line moving.

@models REQ-DRT-COVER.lines_run
-/
def lineReach (hits : List (Nat × Nat)) (item : List (Nat × String)) : List Observed :=
  item.foldl (lineStep hits) []

/--
What a run establishes, given whether it agreed and what it covered.

@models REQ-DRT-COVER.floor_unmet_is_not_pass
-/
def coverageLevel (agreed : Bool) (verdict : CoverageVerdict) : Level :=
  match agreed, verdict with
  | true, .met => Level.L3
  | _, _ => Level.L1

/-- A situation nothing reached is vacuous, which is a different answer from short.

@proves REQ-DRT-COVER.vacuous_named -/
theorem nothing_reached_is_vacuous_not_short :
    coverageVerdict [{ situation := "deletes", atLeast := 10 }] [] []
      = CoverageVerdict.unmet [Gap.vacuous "deletes"] := by
  native_decide

/-- A floor of zero cannot hide a situation no case reached, and every floor
not met is reported, not only the first.

@proves REQ-DRT-COVER.vacuous_named -/
theorem every_gap_is_reported_and_zero_is_vacuous :
    coverageVerdict [{ situation := "a", atLeast := 5 }, { situation := "b", atLeast := 0 }]
        [{ situation := "a", reached := 2 }] []
      = CoverageVerdict.unmet [Gap.short "a" 2 5, Gap.vacuous "b"] := by
  native_decide

/-- Stating no floor is not the same as meeting one.

@proves REQ-DRT-COVER.floor_stated -/
theorem no_floor_is_not_a_met_floor :
    coverageVerdict [] [{ situation := "deletes", reached := 900 }] []
      = CoverageVerdict.undeclared := by
  native_decide

/-- A waiver excuses only with a reason.

@proves REQ-DRT-COVER.waiver_reasoned -/
theorem a_waiver_without_a_reason_excuses_nothing :
    coverageVerdict [{ situation := "x negative", atLeast := 1 }] []
        [{ situations := ["x negative"], reason := " " }]
      = CoverageVerdict.unmet [Gap.vacuous "x negative"]
    ∧ coverageVerdict [{ situation := "x negative", atLeast := 1 }] []
        [{ situations := ["x negative"], reason := "the type admits it, the caller never sends it" }]
      = CoverageVerdict.met := by
  native_decide

/-- A waiver for something reached is reported.

@proves REQ-DRT-COVER.waiver_unused_reported -/
theorem a_reached_waiver_is_reported :
    unusedWaivers [{ situation := "x zero", atLeast := 1 }] [{ situation := "x zero", reached := 3 }]
        [{ situations := ["x zero", "line: unreachable!()"], reason := "old" }]
      = ["x zero", "line: unreachable!()"] := by
  native_decide

/-- Classes come from the bounds: an integer that cannot be negative has no
negative class, and an option's payload brings its own.

@proves REQ-DRT-COVER.classes_reached -/
theorem classes_follow_the_bounds :
    classesAt "" (.struct [("n", .int (some 0) (some 5)), ("o", .option .bool)])
      = ["n zero", "n positive", "o absent", "o present", "o? true", "o? false"] := by
  native_decide

/-- A repeated line keeps its smallest count.

@proves REQ-DRT-COVER.lines_run -/
theorem a_repeated_line_not_run_shows :
    lineReach [(3, 5), (7, 0), (9, 2)] [(3, "  x += 1;"), (7, "x += 1;"), (8, "// no"), (9, "}")]
      = [Observed.mk "line: x += 1;" 0, Observed.mk "line: }" 2] := by
  native_decide

/-- Nothing but an agreeing run that met its floors reaches L3.

@proves REQ-DRT-COVER.floor_unmet_is_not_pass -/
theorem only_a_covered_agreement_is_evidence (agreed : Bool) (v : CoverageVerdict) :
    coverageLevel agreed v = Level.L3 → agreed = true ∧ v = CoverageVerdict.met := by
  intro h
  cases agreed <;> cases v <;> simp_all [coverageLevel]

end TraceLean.Coverage
