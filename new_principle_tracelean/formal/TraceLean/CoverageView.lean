import TraceLean.RequirementView

namespace TraceLean.CoverageView

open Lean (ToJson FromJson)
open TraceLean.View
open TraceLean.Layout
open TraceLean.Lines (LineHits)
open TraceLean.RequirementView (Covered coveredRole coveredText total)
open TraceLean.Strength (sortedUnique)

structure Item where
  path : String
  start : Nat
  symbol : Option String
  clause : Option String
  lines : List LineHits
  text : List String
  deriving Repr, Inhabited, ToJson, FromJson

def countLine (out : Lines) (lead : String) (c : Covered) : Lines :=
  let role := coveredRole c
  if lead == "" then line out [(coveredText c, role, [])]
  else line out [(lead, Role.plain, []), (coveredText c, role, [])]

def ranBy (lines : List LineHits) (test : String) : Nat :=
  TraceLean.Lines.sum (lines.map (fun l => TraceLean.Lines.sum ((l.tests.filter (·.1 == test)).map (·.2))))

def testLine (lines : List LineHits) (out : Lines) (test : String) : Lines :=
  line out [("  ran by  ", Role.plain, []), (test, Role.token .function, []),
            (" ×" ++ toString (ranBy lines test), Role.plain, [])]

def sourceAt (item : Item) (n : Nat) : String :=
  if n < item.start then "" else ((item.text.get? (n - item.start)).getD "").trim

def unrunLine (item : Item) (out : Lines) (l : LineHits) : Lines :=
  line out [("    ", Role.plain, []), (item.path ++ ":" ++ toString l.line, Role.path, ["file.open"]),
            ("  " ++ sourceAt item l.line, Role.plain, [])]

def itemLines (out : Lines) (item : Item) : Lines :=
  let link := (item.path ++ ":" ++ toString item.start, Role.path, ["file.open"])
  let head :=
    match item.symbol with
    | some s => [(s, Role.token .function, []), ("  ", Role.plain, []), link]
    | none => [link]
  let out := countLine (line (blank out) head) "  " (total [(item.path, item.lines)])
  let tests := sortedUnique (fun a b => decide (a ≤ b)) (item.lines.bind (fun l => l.tests.map (·.1)))
  let out := tests.foldl (testLine item.lines) out
  let unrun := item.lines.filter (fun l => l.hits == 0)
  let out := if unrun.isEmpty then out else line out [("  no test runs", Role.removed, [])]
  unrun.foldl (unrunLine item) out

/-- @models REQ-LINECOV.lines_listed -/
def coverageView (named : String) (items : List Item) (width : Nat) : Buffer :=
  let out := line (start width) [("Coverage of ", Role.heading, []), (named, Role.requirement, ["trace.requirement"])]
  if items.isEmpty then
    finish (wrapped out "" "Nothing implementing it was measured against the text it has now. Run `tracelean-trace . --coverage`." Role.plain []) ("coverage " ++ named)
  else
    let out := countLine out "" (total (items.map (fun i => (i.path, i.lines))))
    finish (items.foldl itemLines out) ("coverage " ++ named)

def linked (b : Buffer) : List (String × List String) :=
  (b.spans.filter (!·.actions.isEmpty)).map
    (fun s => (String.mk ((b.text.toList.drop s.start).take (s.stop - s.start)), s.actions))

def measured : List Item :=
  [⟨"src/a.rs", 10, some "f", some "one", [⟨10, 2, [("t1", 2)]⟩, ⟨11, 0, []⟩], ["fn f()", "  x"]⟩]

def clampCopy (size : Nat) (span : Span) : Span :=
  { start := min span.start size, stop := min span.stop size, role := span.role, actions := span.actions }

def keepsCopy (previous : Nat) : List Span → List Span
  | [] => []
  | span :: rest =>
    if span.stop ≤ span.start then keepsCopy previous rest
    else if span.start < previous then keepsCopy previous rest
    else span :: keepsCopy span.stop rest

set_option smartUnfolding false in
theorem tidy_eq (size : Nat) (spans : List Span) : TraceLean.Produce.tidy size spans =
    keepsCopy 0 ((spans.map (clampCopy size)).mergeSort (fun a b => decide (a.start ≤ b.start))) :=
  rfl

def slice (text : String) (sp : Span) : List Char :=
  (text.data.drop sp.start).take (sp.stop - sp.start)

def Good (out : Lines) : Prop :=
  out.spans.Pairwise (fun a b => a.stop ≤ b.start) ∧
    ∀ x ∈ out.spans, x.start ≤ x.stop ∧ x.stop ≤ out.text.data.length

def Grows (a b : Lines) : Prop :=
  (∃ more, b.text.data = a.text.data ++ more) ∧ a.spans <+: b.spans

def pieceSpan (at_ : Nat) (p : Piece) : Span :=
  { start := at_, stop := at_ + p.1.length, role := p.2.1, actions := p.2.2 }

def stepP (acc : String × List Span) (p : Piece) : String × List Span :=
  (acc.1 ++ p.1,
    if p.2.1 != Role.plain || !p.2.2.isEmpty then acc.2 ++ [pieceSpan acc.1.length p] else acc.2)

theorem str_len (s : String) : s.length = s.data.length := by
  cases s
  rfl

def begun (out : Lines) : String := if out.text.isEmpty then out.text else out.text ++ "\n"

theorem line_eq (out : Lines) (ps : List Piece) : line out ps =
    { out with text := (ps.foldl stepP (begun out, out.spans)).1,
               spans := (ps.foldl stepP (begun out, out.spans)).2 } := rfl

def Tidy (acc : String × List Span) : Prop :=
  acc.2.Pairwise (fun a b => a.stop ≤ b.start) ∧
    ∀ x ∈ acc.2, x.start ≤ x.stop ∧ x.stop ≤ acc.1.data.length

theorem step_ext (acc : String × List Span) (p : Piece) :
    (stepP acc p).1.data = acc.1.data ++ p.1.data ∧ acc.2 <+: (stepP acc p).2 := by
  refine ⟨String.data_append _ _, ?_⟩
  unfold stepP
  split
  · exact List.prefix_append _ _
  · exact List.prefix_refl _

theorem fold_ext : ∀ (ps : List Piece) (acc : String × List Span),
    (∃ more, (ps.foldl stepP acc).1.data = acc.1.data ++ more) ∧ acc.2 <+: (ps.foldl stepP acc).2 := by
  intro ps
  induction ps with
  | nil => intro acc; exact ⟨⟨[], by simp⟩, List.prefix_refl _⟩
  | cons p ps ih =>
    intro acc
    obtain ⟨⟨more, h1⟩, h2⟩ := ih (stepP acc p)
    obtain ⟨e1, e2⟩ := step_ext acc p
    refine ⟨⟨p.1.data ++ more, ?_⟩, e2.trans h2⟩
    simp only [List.foldl_cons]
    rw [h1, e1, List.append_assoc]

theorem step_tidy (acc : String × List Span) (p : Piece) (h : Tidy acc) : Tidy (stepP acc p) := by
  obtain ⟨hp, hb⟩ := h
  have hlen : (stepP acc p).1.data.length = acc.1.data.length + p.1.data.length := by
    simp [stepP, String.data_append]
  unfold Tidy
  rw [hlen]
  unfold stepP
  split
  · simp only
    refine ⟨?_, ?_⟩
    · rw [List.pairwise_append]
      refine ⟨hp, List.pairwise_singleton _ _, ?_⟩
      intro a ha b hb'
      simp at hb'
      subst hb'
      simp only [pieceSpan, str_len]
      exact (hb a ha).2
    · intro x hx
      rcases List.mem_append.mp hx with hx | hx
      · exact ⟨(hb x hx).1, Nat.le_trans (hb x hx).2 (Nat.le_add_right _ _)⟩
      · simp at hx
        subst hx
        simp [pieceSpan, str_len]
  · simp only
    exact ⟨hp, fun x hx => ⟨(hb x hx).1, Nat.le_trans (hb x hx).2 (Nat.le_add_right _ _)⟩⟩

theorem fold_tidy : ∀ (ps : List Piece) (acc : String × List Span), Tidy acc →
    Tidy (ps.foldl stepP acc) := by
  intro ps
  induction ps with
  | nil => intro acc h; exact h
  | cons p _ ih => intro acc h; exact ih _ (step_tidy acc p h)

def Witness (text : String) (spans : List Span) (role : Role) (actions : List String)
    (shown : List Char) : Prop :=
  ∃ sp ∈ spans, sp.role = role ∧ sp.actions = actions ∧ sp.start ≤ sp.stop ∧
    sp.stop - sp.start = shown.length ∧ sp.stop ≤ text.data.length ∧ slice text sp = shown

theorem witness_grows {t t' : String} {s s' : List Span} {role : Role} {actions : List String}
    {shown : List Char} (ht : ∃ more, t'.data = t.data ++ more) (hs : s <+: s')
    (h : Witness t s role actions shown) : Witness t' s' role actions shown := by
  obtain ⟨sp, hsp, h1, h2, h0, h3, h4, h5⟩ := h
  obtain ⟨more, hm⟩ := ht
  refine ⟨sp, hs.subset hsp, h1, h2, h0, h3, ?_, ?_⟩
  · rw [hm, List.length_append]
    exact Nat.le_trans h4 (Nat.le_add_right _ _)
  · unfold slice at h5 ⊢
    rw [hm, List.drop_append_of_le_length (by omega), List.take_append_of_le_length
      (by rw [List.length_drop]; omega)]
    exact h5

theorem fold_piece (p : Piece) (hc : (p.2.1 != Role.plain || !p.2.2.isEmpty) = true) :
    ∀ (ps : List Piece) (acc : String × List Span), p ∈ ps →
      Witness (ps.foldl stepP acc).1 (ps.foldl stepP acc).2 p.2.1 p.2.2 p.1.data := by
  intro ps
  induction ps with
  | nil => intro acc h; cases h
  | cons q ps ih =>
    intro acc h
    simp only [List.foldl_cons]
    by_cases hq : p = q
    · subst hq
      obtain ⟨⟨more, hm⟩, hpre⟩ := fold_ext ps (stepP acc p)
      refine witness_grows ⟨more, hm⟩ hpre ?_
      refine ⟨pieceSpan acc.1.length p, ?_, rfl, rfl, ?_, ?_, ?_, ?_⟩
      · simp [stepP, hc]
      · simp [pieceSpan]
      · simp [pieceSpan, str_len, Nat.add_sub_cancel_left]
      · simp [stepP, pieceSpan, String.data_append, str_len]
      · simp [slice, stepP, pieceSpan, String.data_append, str_len, Nat.add_sub_cancel_left]
    · have h' : p ∈ ps := by
        rcases List.mem_cons.mp h with h | h
        · exact absurd h hq
        · exact h
      exact ih _ h'

theorem line_good (out : Lines) (ps : List Piece) (h : Good out) :
    Good (line out ps) ∧ Grows out (line out ps) := by
  rw [line_eq]
  have hbegin : ∃ more, (begun out).data = out.text.data ++ more := by
    unfold begun
    split
    · exact ⟨[], by simp⟩
    · exact ⟨['\n'], String.data_append _ _⟩
  obtain ⟨m0, hm0⟩ := hbegin
  have ht0 : Tidy (begun out, out.spans) := by
    refine ⟨h.1, fun x hx => ⟨(h.2 x hx).1, ?_⟩⟩
    simp only
    rw [hm0, List.length_append]
    exact Nat.le_trans (h.2 x hx).2 (Nat.le_add_right _ _)
  have ht := fold_tidy ps _ ht0
  obtain ⟨⟨more, hm⟩, hpre⟩ := fold_ext ps (begun out, out.spans)
  refine ⟨ht, ⟨m0 ++ more, ?_⟩, hpre⟩
  simp only at hm ⊢
  rw [hm, hm0, List.append_assoc]

theorem line_piece (out : Lines) (ps : List Piece) (p : Piece) (hp : p ∈ ps)
    (hc : (p.2.1 != Role.plain || !p.2.2.isEmpty) = true) :
    Witness (line out ps).text (line out ps).spans p.2.1 p.2.2 p.1.data := by
  rw [line_eq]
  exact fold_piece p hc ps _ hp

theorem grows_refl (a : Lines) : Grows a a := ⟨⟨[], by simp⟩, List.prefix_refl _⟩

theorem grows_trans {a b c : Lines} (h1 : Grows a b) (h2 : Grows b c) : Grows a c := by
  obtain ⟨⟨m1, e1⟩, p1⟩ := h1
  obtain ⟨⟨m2, e2⟩, p2⟩ := h2
  exact ⟨⟨m1 ++ m2, by rw [e2, e1, List.append_assoc]⟩, p1.trans p2⟩

theorem fold_good {α : Type} {f : Lines → α → Lines}
    (hf : ∀ acc x, Good acc → Good (f acc x) ∧ Grows acc (f acc x)) :
    ∀ (xs : List α) (init : Lines), Good init → Good (xs.foldl f init) ∧ Grows init (xs.foldl f init) := by
  intro xs
  induction xs with
  | nil => intro init h; exact ⟨h, grows_refl _⟩
  | cons x xs ih =>
    intro init h
    obtain ⟨g1, r1⟩ := hf init x h
    obtain ⟨g2, r2⟩ := ih _ g1
    exact ⟨g2, grows_trans r1 r2⟩

theorem line_after {out o : Lines} (ps : List Piece) (h : Good o ∧ Grows out o) :
    Good (line o ps) ∧ Grows out (line o ps) := by
  have := line_good o ps h.1
  exact ⟨this.1, grows_trans h.2 this.2⟩

theorem blank_after {out o : Lines} (h : Good o ∧ Grows out o) :
    Good (blank o) ∧ Grows out (blank o) := line_after [] h

theorem countLine_after {out o : Lines} (lead : String) (c : Covered) (h : Good o ∧ Grows out o) :
    Good (countLine o lead c) ∧ Grows out (countLine o lead c) := by
  unfold countLine
  split
  · exact line_after _ h
  · exact line_after _ h

theorem fold_after {α : Type} {f : Lines → α → Lines}
    (hf : ∀ acc x, Good acc → Good (f acc x) ∧ Grows acc (f acc x)) (xs : List α) {out init : Lines}
    (h : Good init ∧ Grows out init) : Good (xs.foldl f init) ∧ Grows out (xs.foldl f init) := by
  have := fold_good hf xs init h.1
  exact ⟨this.1, grows_trans h.2 this.2⟩

theorem testLine_good (lines : List LineHits) :
    ∀ acc x, Good acc → Good (testLine lines acc x) ∧ Grows acc (testLine lines acc x) :=
  fun acc _ h => line_good acc _ h

theorem unrunLine_good (item : Item) :
    ∀ acc x, Good acc → Good (unrunLine item acc x) ∧ Grows acc (unrunLine item acc x) :=
  fun acc _ h => line_good acc _ h

def itemHead (out : Lines) (item : Item) : Lines :=
  let link := (item.path ++ ":" ++ toString item.start, Role.path, ["file.open"])
  let head :=
    match item.symbol with
    | some s => [(s, Role.token .function, []), ("  ", Role.plain, []), link]
    | none => [link]
  let out := countLine (line (blank out) head) "  " (total [(item.path, item.lines)])
  let tests := sortedUnique (fun a b => decide (a ≤ b)) (item.lines.bind (fun l => l.tests.map (·.1)))
  let out := tests.foldl (testLine item.lines) out
  let unrun := item.lines.filter (fun l => l.hits == 0)
  if unrun.isEmpty then out else line out [("  no test runs", Role.removed, [])]

theorem itemLines_eq (out : Lines) (item : Item) :
    itemLines out item = (item.lines.filter (fun l => l.hits == 0)).foldl (unrunLine item)
      (itemHead out item) := rfl

theorem itemHead_good (out : Lines) (item : Item) (h : Good out) :
    Good (itemHead out item) ∧ Grows out (itemHead out item) := by
  unfold itemHead
  simp only
  split
  · exact fold_after (testLine_good item.lines) _
      (countLine_after _ _ (line_after _ (blank_after ⟨h, grows_refl out⟩)))
  · exact line_after _ (fold_after (testLine_good item.lines) _
      (countLine_after _ _ (line_after _ (blank_after ⟨h, grows_refl out⟩))))

theorem itemLines_good (out : Lines) (item : Item) (h : Good out) :
    Good (itemLines out item) ∧ Grows out (itemLines out item) := by
  rw [itemLines_eq]
  exact fold_after (unrunLine_good item) _ (itemHead_good out item h)

theorem keeps_ordered : ∀ (spans : List Span) (previous : Nat) (sp : Span),
    spans.Pairwise (fun a b => a.stop ≤ b.start) → (∀ x ∈ spans, x.start ≤ x.stop) →
    (∀ x ∈ spans, previous ≤ x.start) → sp ∈ spans → sp.start < sp.stop →
    sp ∈ keepsCopy previous spans := by
  intro spans
  induction spans with
  | nil => intro _ _ _ _ _ h; cases h
  | cons a rest ih =>
    intro previous sp hp hle hprev hm hne
    rw [List.pairwise_cons] at hp
    unfold keepsCopy
    by_cases h1 : a.stop ≤ a.start
    · simp only [h1, if_true]
      rcases List.mem_cons.mp hm with hm | hm
      · subst hm; omega
      · exact ih previous sp hp.2 (fun x hx => hle x (List.mem_cons_of_mem _ hx))
          (fun x hx => hprev x (List.mem_cons_of_mem _ hx)) hm hne
    · have h2 : ¬ a.start < previous := by
        have := hprev a (List.mem_cons_self _ _)
        omega
      simp only [h1, h2, if_false]
      rcases List.mem_cons.mp hm with hm | hm
      · subst hm; exact List.mem_cons_self _ _
      · exact List.mem_cons_of_mem _ (ih a.stop sp hp.2
          (fun x hx => hle x (List.mem_cons_of_mem _ hx)) (fun x hx => hp.1 x hx) hm hne)

theorem tidy_keeps (out : Lines) (h : Good out) (sp : Span) (hm : sp ∈ out.spans)
    (hne : sp.start < sp.stop) : sp ∈ TraceLean.Produce.tidy out.text.length out.spans := by
  have hclamp : out.spans.map (clampCopy out.text.length) = out.spans := by
    conv => rhs; rw [← List.map_id out.spans]
    apply List.map_congr_left
    intro x hx
    have hb := h.2 x hx
    rw [str_len]
    simp [clampCopy, Nat.min_eq_left hb.2, Nat.min_eq_left (Nat.le_trans hb.1 hb.2)]
  have hsorted : out.spans.Pairwise (fun a b => decide (a.start ≤ b.start) = true) := by
    apply List.Pairwise.imp_of_mem _ h.1
    intro a b ha _ hab
    have := (h.2 a ha).1
    simp only [decide_eq_true_eq]
    omega
  rw [tidy_eq, hclamp, List.mergeSort_of_sorted hsorted]
  exact keeps_ordered out.spans 0 sp h.1 (fun x hx => (h.2 x hx).1) (fun _ _ => Nat.zero_le _)
    hm hne

theorem unrun_witness (item : Item) (l : LineHits) (hl : l ∈ item.lines) (h0 : l.hits = 0)
    (o : Lines) (ho : Good o) :
    Witness ((item.lines.filter (fun l => l.hits == 0)).foldl (unrunLine item) o).text
      ((item.lines.filter (fun l => l.hits == 0)).foldl (unrunLine item) o).spans Role.path
      ["file.open"] (item.path ++ ":" ++ toString l.line).data := by
  have hlf : l ∈ item.lines.filter (fun l => l.hits == 0) := List.mem_filter.mpr ⟨hl, by simp [h0]⟩
  obtain ⟨pre, post, he⟩ := List.append_of_mem hlf
  rw [he, List.foldl_append, List.foldl_cons]
  have hpre := fold_good (unrunLine_good item) pre o ho
  have hstep := line_good (pre.foldl (unrunLine item) o)
    [("    ", Role.plain, []), (item.path ++ ":" ++ toString l.line, Role.path, ["file.open"]),
     ("  " ++ sourceAt item l.line, Role.plain, [])] hpre.1
  have hpost := fold_good (unrunLine_good item) post _ hstep.1
  have hw := line_piece (pre.foldl (unrunLine item) o)
    [("    ", Role.plain, []), (item.path ++ ":" ++ toString l.line, Role.path, ["file.open"]),
     ("  " ++ sourceAt item l.line, Role.plain, [])]
    (item.path ++ ":" ++ toString l.line, Role.path, ["file.open"]) (by simp) (by simp)
  exact witness_grows hpost.2.1 hpost.2.2 hw

/-- @proves REQ-LINECOV.lines_listed -/
theorem every_unrun_line_is_a_link_to_that_line (named : String) (items : List Item) (width : Nat)
    (item : Item) (hi : item ∈ items) (l : LineHits) (hl : l ∈ item.lines) (h0 : l.hits = 0) :
    ∃ sp ∈ (coverageView named items width).spans, sp.role = Role.path ∧
      sp.actions = ["file.open"] ∧
      String.mk (((coverageView named items width).text.data.drop sp.start).take
        (sp.stop - sp.start)) = item.path ++ ":" ++ toString l.line := by
  have hne : items.isEmpty = false := by
    cases items with
    | nil => cases hi
    | cons _ _ => rfl
  unfold coverageView
  simp only [hne, Bool.false_eq_true, if_false]
  have hs : Good (start width) := ⟨List.Pairwise.nil, fun x hx => by cases hx⟩
  have h2 := countLine_after "" (total (items.map (fun i => (i.path, i.lines))))
    (line_after [("Coverage of ", Role.heading, []), (named, Role.requirement, ["trace.requirement"])]
      ⟨hs, grows_refl (start width)⟩)
  have hall := fold_good itemLines_good items _ h2.1
  obtain ⟨pre, post, he⟩ := List.append_of_mem hi
  have key : ∀ init, Good init → Witness (items.foldl itemLines init).text
      (items.foldl itemLines init).spans Role.path ["file.open"]
      (item.path ++ ":" ++ toString l.line).data := by
    intro init hinit
    rw [he, List.foldl_append, List.foldl_cons]
    have hpre := fold_good itemLines_good pre _ hinit
    have hstep := itemLines_good _ item hpre.1
    have hpost := fold_good itemLines_good post _ hstep.1
    rw [itemLines_eq] at hpost ⊢
    exact witness_grows hpost.2.1 hpost.2.2
      (unrun_witness item l hl h0 _ (itemHead_good _ item hpre.1).1)
  have hw2 := key _ h2.1
  obtain ⟨sp, hsp, hr, ha, _, hlen, _, hsl⟩ := hw2
  refine ⟨sp, ?_, hr, ha, ?_⟩
  · unfold finish
    apply tidy_keeps _ hall.1 sp hsp
    have : 0 < (item.path ++ ":" ++ toString l.line).data.length := by
      simp [String.data_append] <;> omega
    omega
  · unfold finish
    unfold slice at hsl
    simp only
    rw [hsl]

end TraceLean.CoverageView
