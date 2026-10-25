import TraceLean.Layout
import TraceLean.Judge
import TraceLean.Lines

namespace TraceLean.RequirementView

open Lean (ToJson FromJson Json)
open TraceLean.View
open TraceLean.Layout
open TraceLean.Evidence
open TraceLean.Judge

structure Claim where
  role : String
  path : String
  line : Nat
  symbol : Option String
  deriving Repr, Inhabited, ToJson, FromJson

structure Covered where
  run : Nat
  all : Nat
  tests : Nat
  deriving Repr, Inhabited

instance : FromJson Covered where
  fromJson? j := do
    let run ← FromJson.fromJson? (← j.getArrVal? 0)
    let all ← FromJson.fromJson? (← j.getArrVal? 1)
    let tests ← FromJson.fromJson? (← j.getArrVal? 2)
    pure { run := run, all := all, tests := tests }

instance : ToJson Covered where
  toJson c := Json.arr #[ToJson.toJson c.run, ToJson.toJson c.all, ToJson.toJson c.tests]

def firstSeen {α : Type} [BEq α] (xs : List α) : List α :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

def ranAt (path : String) (l : TraceLean.Lines.LineHits) : (String × Nat) × Bool :=
  ((path, l.line), l.hits > 0)

/-- @models REQ-LINECOV.requirement_summary -/
def total (items : List (String × List TraceLean.Lines.LineHits)) : Covered :=
  let marked := items.bind (fun (i : String × List TraceLean.Lines.LineHits) => i.2.map (ranAt i.1))
  let keys := firstSeen (marked.map (·.1))
  let run := (keys.filter (fun k => marked.any (fun p => p.1 == k && p.2))).length
  let tests := firstSeen (items.bind (fun (i : String × List TraceLean.Lines.LineHits) =>
    i.2.bind (fun l => l.tests.map (·.1))))
  { run := run, all := keys.length, tests := tests.length }

structure ClauseShown where
  key : Option String
  text : String
  narrowings : List (String × String)
  marks : Marks
  claims : List Claim
  pins : Option (String × String)
  tested : Bool
  lines : Option Covered
  judged : Option Judged
  deriving Repr, Inhabited, ToJson, FromJson

structure Shown where
  id : String
  title : String
  file : String
  status : String
  refines : List String
  refinedBy : List String
  body : String
  clauses : List ClauseShown
  width : Nat
  lines : Option Covered
  deriving Repr, Inhabited, ToJson, FromJson

def roleWord : TraceLean.Annotation.Role → String
  | .models => "models"
  | .specifies => "specifies"
  | .implements => "implements"
  | .tests => "tests"
  | .drt => "drt"
  | .proves => "proves"
  | .pins => "pins"

def namesLine (out : Lines) (word : String) (names : List String) : Lines :=
  if names.isEmpty then out
  else
    let pieces := names.enum.bind fun (pair : Nat × String) =>
      (if pair.1 > 0 then [(" ", Role.plain, ([] : List String))] else []) ++
        [(pair.2, Role.requirement, ["trace.requirement"])]
    line out ((word, Role.plain, []) :: pieces)

def isPinned (c : ClauseShown) : Bool :=
  match c.pins with
  | some p => p.1 == "pinned"
  | none => false

def evidenceLine (out : Lines) (clauses : List ClauseShown) : Lines :=
  let reaching := fun (kind : TraceLean.Annotation.Role) =>
    (clauses.filter fun c =>
      c.claims.any (·.role == roleWord kind) || (kind == .drt && c.tested)).length
  let of_ := toString clauses.length
  let pinned := (clauses.filter isPinned).length
  let counted : List (TraceLean.Annotation.Role × String) :=
    [(.implements, "implements " ++ toString (reaching .implements) ++ "/" ++ of_),
     (.proves, "proves " ++ toString (reaching .proves) ++ "/" ++ of_),
     (.models, "models " ++ toString (reaching .models) ++ "/" ++ of_),
     (.tests, "tests " ++ toString (reaching .tests) ++ "/" ++ of_),
     (.pins, "strong " ++ toString pinned ++ "/" ++ of_),
     (.drt, "drt " ++ toString (reaching .drt) ++ "/" ++ of_)]
  let pieces := counted.enum.bind fun (pair : Nat × (TraceLean.Annotation.Role × String)) =>
    (if pair.1 > 0 then [(" ", Role.plain, ([] : List String))] else []) ++
      [(pair.2.2, Role.claim pair.2.1, [])]
  line out (("evidence   ", Role.plain, []) :: pieces)

def claimLine (out : Lines) (claim : Claim) : Lines :=
  let role :=
    match TraceLean.Annotation.Role.parse claim.role with
    | some kind => Role.claim kind
    | none => Role.plain
  let symbol :=
    match claim.symbol with
    | some s => [("  " ++ s, Role.token .function, ([] : List String))]
    | none => []
  line out ([("  ", Role.plain, []), (padRight claim.role 10, role, []), (" ", Role.plain, []),
             (claim.path ++ ":" ++ toString claim.line, Role.path, ["file.open"])] ++ symbol)

def noLines : String := "no line a test could run"

def coveredText (c : Covered) : String :=
  if c.all == 0 then noLines else
  toString c.run ++ "/" ++ toString c.all ++ " lines run (" ++ toString (c.run * 100 / c.all) ++ "%), by " ++
    toString c.tests ++ " test" ++ (if c.tests == 1 then "" else "s")

def coveredRole (c : Covered) : Role :=
  if c.all == 0 then Role.plain else if c.run == c.all then Role.added else Role.removed

def notMeasured : String := "not measured: run `tracelean-trace . --coverage`"

def implemented (c : ClauseShown) : Bool := c.claims.any (·.role == "implements")

def marksLine (out : Lines) (clauses : List ClauseShown) : Lines :=
  let modelled := clauses.filter (fun c => !c.marks.structural)
  let count := fun (held : ClauseShown → Bool) => (modelled.filter held).length
  let counts : List (String × Nat × Role) :=
    [("I", count implemented, Role.claim TraceLean.Annotation.Role.implements),
     ("P", count (·.marks.proved), Role.claim TraceLean.Annotation.Role.proves),
     ("M", count (·.claims.any (·.role == "models")), Role.claim TraceLean.Annotation.Role.models),
     ("T", count (·.marks.tested), Role.claim TraceLean.Annotation.Role.tests),
     ("S", count isPinned, Role.claim TraceLean.Annotation.Role.pins),
     ("D", count (·.tested), Role.claim TraceLean.Annotation.Role.drt)]
  let said := counts.map fun (c : String × Nat × Role) =>
    if modelled.length == 0 then (c.1 ++ "·", Role.plain, ([] : List String))
    else (c.1 ++ toString c.2.1 ++ "/" ++ toString modelled.length, c.2.2, ([] : List String))
  line out (("marks      ", Role.plain, []) :: said.intersperse (" ", Role.plain, []))

def coveredLine (lead name : String) (claimed : Bool) (out : Lines) : Option Covered → Lines
  | none => if claimed then line out [(lead, Role.plain, []), (notMeasured, Role.removed, [])] else out
  | some c =>
    let role := coveredRole c
    line out [(lead, Role.plain, []), (name, Role.requirement, ["trace.coverage"]), ("  ", Role.plain, []),
              (coveredText c, role, [])]

def pinsLines (out : Lines) : Option (String × String) → Lines
  | none => out
  | some (state, said) =>
    let role := if state == "pinned" then Role.added else Role.removed
    wrapped (line out [("  ", Role.plain, []), (state, role, [])]) "    " said Role.plain []

def judgedLines (out : Lines) : Option Judged → Lines
  | none => out
  | some judged =>
    let said := judgedText judged
    if judged.verdict == "agrees" then line out [("  judged      ", Role.plain, []), (said, Role.plain, [])]
    else wrapped out "  " said Role.removed []

def markPiecesOf (m : Marks) : List (Mark × Bool × Char) → List (String × Role × List String)
  | [] => []
  | (mark, held, c) :: rest =>
    (if m.applies mark then
      (String.singleton (letter held c), Role.mark mark held, ["trace.rollup"])
    else ("·", Role.plain, [])) :: markPiecesOf m rest

def markPieces (m : Marks) : List (String × Role × List String) := markPiecesOf m m.each

def clauseLines (id : String) (out : Lines) (clause : ClauseShown) : Lines :=
  let key := clause.key.getD "(the whole requirement)"
  let out := line (blank out) (markPieces clause.marks ++ [("  ", Role.plain, []), (key, Role.heading, [])])
  let out := wrapped out "  " clause.text Role.plain []
  let out := clause.narrowings.foldl (fun (o : Lines) (pair : String × String) =>
    wrapped o "    " (pair.1 ++ ": " ++ pair.2) Role.plain []) out
  let out :=
    match clause.key with
    | some k => line out [("  for agent   ", Role.plain, []), (id ++ "." ++ k, Role.requirement, ["trace.context"])]
    | none => out
  let out := if clause.claims.isEmpty then line out [("  nothing claims it yet", Role.removed, [])] else out
  let name :=
    match clause.key with
    | some k => id ++ "." ++ k
    | none => id
  let out := pinsLines (coveredLine "  covered     " name (implemented clause) out clause.lines) clause.pins
  let out :=
    if clause.claims.any (·.role == "models") then
      line out [("  judge       ", Role.plain, []), (name, Role.requirement, ["trace.judge"])]
    else out
  clause.claims.foldl claimLine (judgedLines out clause.judged)

/-- @models REQ-SHOW.requirement_opened -/
def requirementView (view : Shown) : Buffer :=
  let out := line (start view.width) [(view.id, Role.requirement, [])]
  let out := wrapped out "" view.title Role.heading []
  let out := line out [(view.file, Role.path, ["file.open"])]
  let status := "status " ++ view.status
  let out :=
    if view.status == "draft" then
      line out [(status, Role.plain, []), ("  ", Role.plain, []), ("[ Approve ]", Role.added, ["trace.approve"])]
    else line out [(status, Role.plain, [])]
  let out := namesLine (namesLine out "refines    " view.refines) "refined by " view.refinedBy
  let out :=
    if view.body.isEmpty then out
    else
      blank ((view.body.splitOn "\n").foldl (fun (o : Lines) (paragraph : String) =>
        wrapped o "" paragraph Role.plain []) (blank out))
  let out := line out [("for agent  ", Role.plain, []), (view.id, Role.requirement, ["trace.context"]),
                       ("  context to copy", Role.plain, [])]
  let out := coveredLine "covered    " view.id (view.clauses.any implemented)
    (marksLine (evidenceLine out view.clauses) view.clauses) view.lines
  finish (view.clauses.foldl (clauseLines view.id) out) ("requirement " ++ view.id)

/-- @proves REQ-LINECOV.requirement_summary -/
theorem a_shared_line_counts_once :
    (fun c => (c.run, c.all, c.tests))
        (total [("a.rs", [⟨1, 2, [("t1", 2)]⟩, ⟨2, 0, []⟩]), ("a.rs", [⟨2, 1, [("t2", 1)]⟩, ⟨3, 0, []⟩])])
      = (2, 3, 2) := by
  native_decide

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

def oneClause : Shown :=
  ⟨"REQ-A", "A thing", "reqs/REQ-A.md", "draft", [], [], "",
   [⟨some "one", "It shall.", [], ⟨true, false, true, false, false⟩,
     [⟨"implements", "src/a.rs", 4, some "f"⟩, ⟨"tests", "tests/a.rs", 9, none⟩], none, true, none, none⟩],
   60, none⟩

/-- @proves REQ-SHOW.requirement_opened -/
theorem an_opened_requirement_shows_clauses_and_claims :
    ((requirementView oneClause).text.splitOn "A-T-  one\n  It shall.").length = 2 ∧
    (linked (requirementView oneClause)).filter (fun l => l.2 == ["file.open"])
      = [("reqs/REQ-A.md", ["file.open"]), ("src/a.rs:4", ["file.open"]),
         ("tests/a.rs:9", ["file.open"])] := by
  native_decide

end TraceLean.RequirementView
