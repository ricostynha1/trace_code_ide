import TraceLean.LinesProof
import TraceLean.Judge
import TraceLean.Lines

namespace TraceLean.RequirementView

open Lean (ToJson FromJson Json)
open TraceLean.View
open TraceLean.Layout
open TraceLean.LinesProof
open TraceLean.Produce (Offers)
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

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

theorem firstSeen_spec {α : Type} [BEq α] [LawfulBEq α] : ∀ (l acc : List α), acc.Nodup →
    (l.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) acc).Nodup ∧
      ∀ x, x ∈ l.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) acc ↔ x ∈ acc ∨ x ∈ l := by
  intro l
  induction l with
  | nil => intro acc h; exact ⟨h, fun x => by simp⟩
  | cons y ys ih =>
    intro acc h
    simp only [List.foldl]
    by_cases hy : acc.contains y = true
    · rw [if_pos hy]
      obtain ⟨n, m⟩ := ih acc h
      refine ⟨n, fun x => ?_⟩
      rw [m]
      have : y ∈ acc := List.elem_iff.mp hy
      simp only [List.mem_cons]
      constructor
      · rintro (h | h)
        · exact Or.inl h
        · exact Or.inr (Or.inr h)
      · rintro (h | rfl | h)
        · exact Or.inl h
        · exact Or.inl this
        · exact Or.inr h
    · rw [if_neg hy]
      have hn : y ∉ acc := fun e => hy (List.elem_iff.mpr e)
      have nd : (acc ++ [y]).Nodup := List.pairwise_append.mpr
        ⟨h, List.pairwise_singleton _ _, fun a ha b hb => by
          simp only [List.mem_singleton] at hb; subst hb; exact fun e => hn (e ▸ ha)⟩
      obtain ⟨n, m⟩ := ih _ nd
      refine ⟨n, fun x => ?_⟩
      rw [m]
      simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · rintro ((h | h) | h)
        · exact Or.inl h
        · exact Or.inr (Or.inl h)
        · exact Or.inr (Or.inr h)
      · rintro (h | h | h)
        · exact Or.inl (Or.inl h)
        · exact Or.inl (Or.inr h)
        · exact Or.inr h

theorem firstSeen_nodup_mem {α : Type} [BEq α] [LawfulBEq α] (l : List α) :
    (firstSeen l).Nodup ∧ ∀ x, x ∈ firstSeen l ↔ x ∈ l := by
  obtain ⟨n, m⟩ := firstSeen_spec l [] List.nodup_nil
  exact ⟨n, fun x => by rw [firstSeen, m]; simp⟩

/-- @proves REQ-LINECOV.requirement_summary -/
theorem a_requirement_counts_each_line_once (items : List (String × List TraceLean.Lines.LineHits)) :
    ∃ keys tests : List _,
      keys.Nodup ∧ (∀ k, k ∈ keys ↔ ∃ i ∈ items, ∃ l ∈ i.2, (i.1, l.line) = k) ∧
      tests.Nodup ∧ (∀ t, t ∈ tests ↔ ∃ i ∈ items, ∃ l ∈ i.2, ∃ p ∈ l.tests, p.1 = t) ∧
      (total items).all = keys.length ∧ (total items).tests = tests.length ∧
      (total items).run =
        (keys.filter fun k => decide (∃ i ∈ items, ∃ l ∈ i.2, (i.1, l.line) = k ∧ 0 < l.hits)).length := by
  let marked := items.bind (fun (i : String × List TraceLean.Lines.LineHits) => i.2.map (ranAt i.1))
  obtain ⟨kn, km⟩ := firstSeen_nodup_mem (marked.map (·.1))
  obtain ⟨tn, tm⟩ := firstSeen_nodup_mem (items.bind (fun (i : String × List TraceLean.Lines.LineHits) =>
    i.2.bind (fun l => l.tests.map (·.1))))
  refine ⟨_, _, kn, fun k => ?_, tn, fun t => ?_, rfl, rfl, ?_⟩
  · rw [km]
    simp only [marked, ranAt, List.mem_map, List.mem_bind]
    constructor
    · rintro ⟨_, ⟨i, hi, l, hl, rfl⟩, rfl⟩
      exact ⟨i, hi, l, hl, rfl⟩
    · rintro ⟨i, hi, l, hl, rfl⟩
      exact ⟨_, ⟨i, hi, l, hl, rfl⟩, rfl⟩
  · rw [tm]
    simp only [List.mem_map, List.mem_bind]
  · show (List.filter _ _).length = _
    congr 1
    apply List.filter_congr
    intro k _
    rw [Bool.eq_iff_iff]
    simp only [ranAt, List.any_eq_true, List.mem_bind, List.mem_map, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq]
    constructor
    · rintro ⟨_, ⟨i, hi, l, hl, rfl⟩, e, h⟩
      exact ⟨i, hi, l, hl, e, of_decide_eq_true h⟩
    · rintro ⟨i, hi, l, hl, e, h⟩
      exact ⟨_, ⟨i, hi, l, hl, rfl⟩, e, decide_eq_true h⟩

theorem pinsLines_grows (o : Lines) (p : Option (String × String)) : LGrows o (pinsLines o p) := by
  unfold pinsLines
  split
  · exact lgrows_refl _
  · exact lgrows_trans (line_grows _ _) (wrapped_grows _ _ _ _ _)

theorem coveredLine_grows (lead name : String) (claimed : Bool) (o : Lines) (c : Option Covered) :
    LGrows o (coveredLine lead name claimed o c) := by
  unfold coveredLine
  split
  · exact lgrows_ite (line_grows _ _) (lgrows_refl _)
  · exact line_grows _ _

theorem judgedLines_grows (o : Lines) (j : Option Judged) : LGrows o (judgedLines o j) := by
  unfold judgedLines
  split
  · exact lgrows_refl _
  · exact lgrows_ite (line_grows _ _) (wrapped_grows _ _ _ _ _)

theorem claimLine_grows (o : Lines) (c : Claim) : LGrows o (claimLine o c) := line_grows _ _

theorem namesLine_grows (o : Lines) (w : String) (n : List String) : LGrows o (namesLine o w n) := by
  unfold namesLine
  exact lgrows_ite (lgrows_refl _) (line_grows _ _)

theorem evidenceLine_grows (o : Lines) (c : List ClauseShown) : LGrows o (evidenceLine o c) := line_grows _ _

theorem marksLine_grows (o : Lines) (c : List ClauseShown) : LGrows o (marksLine o c) := line_grows _ _

theorem clauseLines_grows (id : String) (o : Lines) (c : ClauseShown) : LGrows o (clauseLines id o c) := by
  unfold clauseLines
  generalize c.key = key
  cases key
  all_goals
    dsimp only
    refine lgrows_trans ?_ (foldl_lgrows _ claimLine_grows _ _)
    refine lgrows_trans ?_ (judgedLines_grows _ _)
    refine lgrows_trans ?_ (lgrows_ite (line_grows _ _) (lgrows_refl _))
    refine lgrows_trans ?_ (pinsLines_grows _ _)
    refine lgrows_trans ?_ (coveredLine_grows _ _ _ _ _)
    refine lgrows_trans ?_ (lgrows_ite (line_grows _ _) (lgrows_refl _))
    refine lgrows_trans ?_ (by first | exact line_grows _ _ | exact lgrows_refl _)
    refine lgrows_trans ?_ (foldl_lgrows _ (fun o x => wrapped_grows _ _ _ _ _) _ _)
    refine lgrows_trans ?_ (wrapped_grows _ _ _ _ _)
    exact lgrows_trans (blank_grows _) (line_grows _ _)

theorem clause_heading_says (id : String) (o : Lines) (c : ClauseShown) :
    ∀ q ∈ markPieces c.marks ++ [("  ", Role.plain, []), (c.key.getD "(the whole requirement)", Role.heading, [])],
      (q.2.1 != Role.plain || !q.2.2.isEmpty) = true → Says (clauseLines id o c) q.2.1 q.2.2 q.1 := by
  intro q hq hc
  unfold clauseLines
  generalize c.key = key at hq ⊢
  cases key
  all_goals
    dsimp only
    refine says_grows ?_ (foldl_lgrows _ claimLine_grows _ _)
    refine says_grows ?_ (judgedLines_grows _ _)
    refine says_grows ?_ (lgrows_ite (line_grows _ _) (lgrows_refl _))
    refine says_grows ?_ (pinsLines_grows _ _)
    refine says_grows ?_ (coveredLine_grows _ _ _ _ _)
    refine says_grows ?_ (lgrows_ite (line_grows _ _) (lgrows_refl _))
    refine says_grows ?_ (by first | exact line_grows _ _ | exact lgrows_refl _)
    refine says_grows ?_ (foldl_lgrows _ (fun o x => wrapped_grows _ _ _ _ _) _ _)
    refine says_grows ?_ (wrapped_grows _ _ _ _ _)
    exact line_says _ _ _ _ _ hq hc

theorem claim_says (o : Lines) (c : Claim) :
    Says (claimLine o c) Role.path ["file.open"] (c.path ++ ":" ++ toString c.line) :=
  line_says _ _ _ _ _ (by simp) rfl

theorem mem_markPiecesOf (m : Marks) : ∀ (l : List (Mark × Bool × Char)) (mark : Mark) (held : Bool) (ch : Char),
    (mark, held, ch) ∈ l → m.applies mark = true →
      (String.singleton (letter held ch), Role.mark mark held, ["trace.rollup"]) ∈ markPiecesOf m l := by
  intro l
  induction l with
  | nil => intro _ _ _ h; cases h
  | cons x rest ih =>
    intro mark held ch h ha
    unfold markPiecesOf
    rcases List.mem_cons.mp h with rfl | h
    · simp [ha]
    · exact List.mem_cons_of_mem _ (ih _ _ _ h ha)

theorem good_of {o o' : Lines} (g : LGrows o o') (h : Good o) : Good o' := g.2.2.2 h

/-- @proves REQ-SHOW.requirement_opened -/
theorem a_requirement_shows_each_clause_with_its_marks_and_claims (view : Shown) :
    ∀ c ∈ view.clauses,
      (c.key.getD "(the whole requirement)" ≠ "" →
        Offers (requirementView view) Role.heading [] (c.key.getD "(the whole requirement)")) ∧
      (∀ mark held ch, (mark, held, ch) ∈ c.marks.each → c.marks.applies mark = true →
        Offers (requirementView view) (Role.mark mark held) ["trace.rollup"] (String.singleton (letter held ch))) ∧
      ∀ claim ∈ c.claims,
        Offers (requirementView view) Role.path ["file.open"] (claim.path ++ ":" ++ toString claim.line) := by
  obtain ⟨o, good, eq⟩ : ∃ o, Good o ∧
      requirementView view = finish (view.clauses.foldl (clauseLines view.id) o) ("requirement " ++ view.id) := by
    refine ⟨_, ?_, rfl⟩
    repeat (first
      | with_reducible refine good_of (coveredLine_grows _ _ _ _ _) ?_
      | with_reducible refine good_of (marksLine_grows _ _) ?_
      | with_reducible refine good_of (evidenceLine_grows _ _) ?_
      | with_reducible refine good_of (namesLine_grows _ _ _) ?_
      | with_reducible refine good_of (blank_grows _) ?_
      | with_reducible refine good_of (wrapped_grows _ _ _ _ _) ?_
      | with_reducible refine good_of (foldl_lgrows _ (fun o x => wrapped_grows _ _ _ _ _) _ _) ?_
      | with_reducible refine good_of (line_grows _ _) ?_
      | with_reducible exact good_start _
      | split)
  intro c hc
  rw [eq]
  have fin := (foldl_lgrows _ (clauseLines_grows view.id) view.clauses o).2.2.2 good
  have says := fun r a t (h : ∀ o, Says (clauseLines view.id o c) r a t) =>
    foldl_says (clauseLines view.id) (clauseLines_grows view.id) c h view.clauses o hc
  refine ⟨fun ne => finish_offers _ _ fin (says _ _ _ fun o =>
      clause_heading_says view.id o c (c.key.getD "(the whole requirement)", Role.heading, [])
        (List.mem_append_right _ (by simp)) (by simp)) ne,
    fun mark held ch hm ha => finish_offers _ _ fin (says _ _ _ fun o =>
      clause_heading_says view.id o c _ (List.mem_append_left _ (mem_markPiecesOf _ _ _ _ _ hm ha)) (by simp))
      (TraceLean.Produce.lit_ne 0 (by simp [String.singleton])),
    fun claim hcl => finish_offers _ _ fin (says _ _ _ fun o => by
      unfold clauseLines
      exact foldl_says claimLine claimLine_grows claim (fun o => claim_says o claim) _ _ hcl)
      (fun h => by
        have := congrArg String.length h
        have one : (":" : String).length = 1 := rfl
        simp only [String.length_append, one, String.length_empty] at this
        omega)⟩

end TraceLean.RequirementView
