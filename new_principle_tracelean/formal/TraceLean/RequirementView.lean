import TraceLean.Layout
import TraceLean.Judge
import TraceLean.Lines

/-!
# One requirement, opened

Models `REQ-SHOW.requirement_opened`: a requirement as a buffer showing each of
its clauses with the level its evidence reached and every annotation that
claims it, each claim a `path:line` link that opens the claiming file there.
-/

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

/-- How much of a clause's implementing items tests run: lines run, lines,
and tests. Read as the three-element array the implementation writes. -/
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

/-- Each path once, in the order first seen. -/
def firstSeen {α : Type} [BEq α] (xs : List α) : List α :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

/-- One measured line of an item: where it is, and whether a test ran it. -/
def ranAt (path : String) (l : TraceLean.Lines.LineHits) : (String × Nat) × Bool :=
  ((path, l.line), l.hits > 0)

/-- Lines run, executable lines and tests over items (each a path and its
measured lines), a line of a file counted once however many items span it.

@models REQ-LINECOV.requirement_summary -/
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
  level : Level
  chain : String
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

/-- `word` and the names, each opening its requirement; nothing for none. -/
def namesLine (out : Lines) (word : String) (names : List String) : Lines :=
  if names.isEmpty then out
  else
    let pieces := names.enum.bind fun (pair : Nat × String) =>
      (if pair.1 > 0 then [(" ", Role.plain, ([] : List String))] else []) ++
        [(pair.2, Role.requirement, ["trace.requirement"])]
    line out ((word, Role.plain, []) :: pieces)

/-- How many clauses each kind of claim reaches, in the claims' colours. -/
def evidenceLine (out : Lines) (clauses : List ClauseShown) : Lines :=
  let kinds : List TraceLean.Annotation.Role := [.implements, .tests, .models, .proves, .drt]
  let counted := kinds.map fun kind =>
    let reached := (clauses.filter fun c =>
      c.claims.any (·.role == roleWord kind) || (kind == .drt && c.tested)).length
    (kind, roleWord kind ++ " " ++ toString reached ++ "/" ++ toString clauses.length)
  let pieces := counted.enum.bind fun (pair : Nat × (TraceLean.Annotation.Role × String)) =>
    (if pair.1 > 0 then [(" ", Role.plain, ([] : List String))] else []) ++
      [(pair.2.2, Role.claim pair.2.1, [])]
  line out (("evidence   ", Role.plain, []) :: pieces)

/-- A claim: its kind in its chip's colour, the link, and the item it sits on. -/
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

/-- `3/4 lines run (75%), by 2 tests`; all of nothing is all of it. -/
def coveredText (c : Covered) : String :=
  let percent := if c.all == 0 then 100 else c.run * 100 / c.all
  toString c.run ++ "/" ++ toString c.all ++ " lines run (" ++ toString percent ++ "%), by " ++
    toString c.tests ++ " test" ++ (if c.tests == 1 then "" else "s")

/-- The requirement or clause, which opens the lines behind the count, then
the count: whole in green, short in red. -/
def coveredLine (lead name : String) (out : Lines) : Option Covered → Lines
  | none => out
  | some c =>
    let role := if c.run == c.all then Role.added else Role.removed
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

/-- One clause: its level, key and chain, its text and narrowings, what can be
done with it, and every claim on it. -/
def clauseLines (id : String) (out : Lines) (clause : ClauseShown) : Lines :=
  let grade := levelName clause.level
  let key := clause.key.getD "(the whole requirement)"
  let out := line (blank out) [(grade, Role.level clause.level, ["trace.rollup"]), ("  ", Role.plain, []),
                               (key, Role.heading, []), ("  " ++ clause.chain, Role.plain, [])]
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
  let out := pinsLines (coveredLine "  covered     " name out clause.lines) clause.pins
  let out :=
    if clause.claims.any (·.role == "models") then
      line out [("  judge       ", Role.plain, []), (name, Role.requirement, ["trace.judge"])]
    else out
  clause.claims.foldl claimLine (judgedLines out clause.judged)

/-- The requirement as a buffer, titled `requirement <id>`.

@models REQ-SHOW.requirement_opened -/
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
  let out := line out [("for agent  ", Role.plain, []), (view.id, Role.requirement, ["trace.context"]),
                       ("  context to copy", Role.plain, [])]
  let out := coveredLine "covered    " view.id (evidenceLine out view.clauses) view.lines
  finish (view.clauses.foldl (clauseLines view.id) out) ("requirement " ++ view.id)

end TraceLean.RequirementView
