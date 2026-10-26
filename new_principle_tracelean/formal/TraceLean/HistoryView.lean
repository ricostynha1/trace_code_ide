import Lean
import TraceLean.Produce

open Lean

namespace TraceLean.HistoryView

structure Point where
  node : Nat
  parent : Option Nat
  here : Bool
  said : String
  file : Option String
  saved : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure GraphRow where
  node : Option Nat
  cells : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Filter where
  | all | file | saved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def filterNamed (name : String) : Option Filter :=
  match name with
  | "All" => some .all
  | "File" => some .file
  | "Saved" => some .saved
  | _ => none

def isHere (ps : List Point) : Option Nat → Bool
  | none => !ps.any (·.here)
  | some n => ps.any (fun p => p.node == n && p.here)

def climbs (ps : List Point) (target : Nat) : Nat → Option Nat → Bool
  | 0, _ => false
  | _, none => false
  | fuel + 1, some current =>
    if current == target then true
    else climbs ps target fuel ((ps.find? (fun p => p.node == current)).bind (·.parent))

def onCurrentBranch (ps : List Point) : Option Nat → Bool
  | none => true
  | some target => climbs ps target (ps.length + 1) ((ps.find? (·.here)).map (·.node))

def freeColumn (open_ : List Bool) (taken : List Nat) : Nat → Nat → Nat
  | 0, c => c
  | fuel + 1, c =>
    if open_.getD c false || taken.contains c then freeColumn open_ taken fuel (c + 1) else c

def kidsOf (ps : List Point) (node : Option Nat) (drawn : List Nat) : List Nat :=
  ps.foldl (fun acc p =>
    if p.parent == node && !drawn.contains p.node && !acc.contains p.node then acc ++ [p.node] else acc) []

def takenFor (open_ : List Bool) (col : Nat) (branches : List Nat) : List Nat :=
  branches.foldl (fun acc _ =>
    acc ++ [freeColumn open_ acc (open_.length + acc.length + 1) (col + 1)]) []

def cellsOf (open_ : List Bool) (col : Nat) (taken : List Nat) (glyph : Char) : List Char :=
  let base := open_.foldr (fun o acc => (if o then '│' else ' ') :: ' ' :: acc) []
  let marked := base.set (col * 2) glyph
  match taken.getLast? with
  | none => marked
  | some last =>
    let dashed := marked.enum.map (fun (i, ch) =>
      if col * 2 + 1 ≤ i && i < last * 2 && ch == ' ' then '─' else ch)
    taken.foldl (fun cs c => cs.set (c * 2) (if c == last then '╮' else '┬')) dashed

def seenAlready (drawn : List Nat) : Option Nat → Bool
  | some n => drawn.contains n
  | none => false

def markDrawn (drawn : List Nat) : Option Nat → List Nat
  | some n => drawn ++ [n]
  | none => drawn

structure Step where
  row : GraphRow
  columns : List Bool
  pending : List (Option Nat × Nat)

def stepOf (ps : List Point) (node : Option Nat) (col : Nat) (rest : List (Option Nat × Nat))
    (open_ : List Bool) (drawn : List Nat) : Step :=
  let kids := kidsOf ps node (markDrawn drawn node)
  let taken := takenFor open_ col (kids.drop 1)
  let width := max (taken.foldl (fun w c => max w (c + 1)) (col + 1)) open_.length
  let widened := open_ ++ List.replicate (width - open_.length) false
  let glyph := if isHere ps node then '●' else '○'
  let closed := widened.set col (!kids.isEmpty)
  let branches := ((kids.drop 1).zip taken).map (fun (k, c) => (some k, c))
  let first := (kids.head?.map (fun k => (some k, col))).toList
  { row := GraphRow.mk node (String.mk (cellsOf widened col taken glyph)),
    columns := taken.foldl (fun o c => o.set c true) closed,
    pending := branches ++ first ++ rest }

def graphLoop (ps : List Point) :
    Nat → List (Option Nat × Nat) → List Bool → List Nat → List GraphRow → List GraphRow
  | 0, _, _, _, out => out
  | _ + 1, [], _, _, out => out
  | fuel + 1, (node, col) :: rest, open_, drawn, out =>
    if seenAlready drawn node then graphLoop ps fuel rest open_ drawn out
    else graphLoop ps fuel (stepOf ps node col rest open_ drawn).pending
      (stepOf ps node col rest open_ drawn).columns (markDrawn drawn node)
      (out ++ [(stepOf ps node col rest open_ drawn).row])

/-- @models REQ-UNDO.tree_is_drawn -/
def historyGraph (points : List Point) : List GraphRow :=
  graphLoop points (points.length + 1) [(none, 0)] [] [] []

def keep (f : Filter) (file : String) (p : Point) : Bool :=
  match f with
  | .all => true
  | .file => p.file == some file
  | .saved => p.saved

def keptAncestor (ps : List Point) (f : Filter) (file : String) : Nat → Option Nat → Option Nat
  | 0, _ => none
  | _ + 1, none => none
  | fuel + 1, some n =>
    match ps.find? (·.node == n) with
    | none => none
    | some p => if keep f file p then some n else keptAncestor ps f file fuel p.parent

/-- @models REQ-UNDO.filtered_view -/
def shownPoints (points : List Point) (filter : Filter) (file : String) : List Point :=
  let fuel := points.length + 1
  let current := (points.find? (·.here)).map (·.node)
  let target := keptAncestor points filter file fuel current
  (points.filter (keep filter file)).map (fun p =>
    { p with parent := keptAncestor points filter file fuel p.parent,
             here := some p.node == target })

/-- @proves REQ-UNDO.filtered_view -/
theorem shown_are_kept (points : List Point) (f : Filter) (file : String) :
    (shownPoints points f file).all (keep f file) = true := by
  unfold shownPoints keep
  cases f
  all_goals simp [List.all_eq_true]

open TraceLean.View
open TraceLean.Produce

def pointName : Option Nat → String
  | none => "#0"
  | some n => "#" ++ toString (n + 1)

def switches : List (Filter × String) := [(.all, "All"), (.file, "File"), (.saved, "Saved")]

def switchLine (filter : Filter) : String × List Span :=
  switches.foldl (fun (acc : String × List Span) (pair : Filter × String) =>
    let at_ := acc.1.length
    let role := if pair.1 == filter then Role.heading else Role.entry
    (acc.1 ++ pair.2 ++ "  ",
     acc.2 ++ [{ start := at_, stop := at_ + pair.2.length, role := role, actions := ["history.filter"] }]))
    ("", [])

def treeRow (all shown : List Point) (graph : Nat) (acc : String × List Span) (row : GraphRow) :
    String × List Span :=
  let text := acc.1 ++ "\n"
  let at_ := text.length
  let cut := row.cells.toList.take graph
  let cells := String.mk (cut ++ List.replicate (graph - cut.length) ' ')
  let said :=
    match row.node with
    | none => "the tree as it was opened"
    | some n => ((shown.find? (·.node == n)).map (·.said)).getD ""
  let name := pointName row.node
  let from_ := at_ + graph + 1
  let role :=
    if row.cells.toList.contains '●' then Role.heading
    else if onCurrentBranch all row.node then Role.added
    else Role.entry
  (text ++ cells ++ " " ++ name ++ "  " ++ said,
   acc.2 ++ [{ start := from_, stop := from_ + name.length, role := role, actions := ["history.jump"] }])

/-- @models REQ-UNDO.tree_is_shown -/
def historyView (points : List Point) (filter : Filter) (file : Option String) : Buffer :=
  let shown := shownPoints points filter (file.getD "")
  let drawn := historyGraph shown
  let top := switchLine filter
  let heading :=
    match filter, file with
    | .file, some f => top.1 ++ f
    | _, _ => top.1
  let graph :=
    match drawn with
    | [] => 0
    | _ => (drawn.foldl (fun m r => max m r.cells.length) 0) - 1
  let done := drawn.foldl (treeRow points shown graph) (heading, top.2)
  { id := "record:history", kind := .record "history", text := done.1,
    spans := tidy done.1.length done.2 }

def near (diff : List DiffLine) (i : Nat) : Bool :=
  ((diff.drop (i - 2)).take (min (i + 3) diff.length - (i - 2))).any (fun d => d.role != Role.plain)

def nearLines (diff : List DiffLine) : Nat → Bool → List DiffLine → List (String × Role)
  | _, _, [] => []
  | i, skipped, d :: rest =>
    if near diff i then (d.text, d.role) :: nearLines diff (i + 1) false rest
    else if !skipped then ("…", Role.plain) :: nearLines diff (i + 1) true rest
    else nearLines diff (i + 1) true rest

structure Changed where
  path : String
  before : String
  after : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def changedLines (c : Changed) : List (String × Role) :=
  let diff := diffLines (c.before.splitOn "\n") (c.after.splitOn "\n")
  (c.path, Role.path) :: nearLines diff 0 false diff

def joinLines (acc : String × List Span) (line : String × Role) : String × List Span :=
  let text := if acc.1.isEmpty then acc.1 else acc.1 ++ "\n"
  let at_ := text.length
  let spans :=
    if line.2 == Role.plain then acc.2
    else acc.2 ++ [{ start := at_, stop := at_ + line.1.length, role := line.2, actions := [] }]
  (text ++ line.1, spans)

/-- @models REQ-UNDO.hover_shows_change -/
def changeView (node : Nat) (said : String) (changed : List Changed) : Buffer :=
  let name := pointName (some node)
  let lines := (name ++ "  " ++ said, Role.heading) :: changed.bind changedLines
  let done := lines.foldl joinLines ("", [])
  { id := "record:change " ++ name, kind := .record ("change " ++ name), text := done.1,
    spans := tidy done.1.length done.2 }

def branched : List Point :=
  [⟨0, none, false, "a", none, false⟩, ⟨1, some 0, false, "b", none, false⟩,
   ⟨2, some 0, true, "c", none, false⟩]

def ParentFirst (ps : List Point) (out : List GraphRow) : Prop :=
  ∀ i (h : i < out.length), out[i].node = none ∨
    ∃ p ∈ ps, out[i].node = some p.node ∧ p.parent ∈ (out.take i).map (·.node)

def Waiting (ps : List Point) (out : List GraphRow) (pending : List (Option Nat × Nat)) : Prop :=
  ∀ e ∈ pending, e.1 = none ∨ ∃ p ∈ ps, e.1 = some p.node ∧ p.parent ∈ out.map (·.node)

theorem parent_first_snoc (ps : List Point) (out : List GraphRow) (row : GraphRow)
    (h : ParentFirst ps out)
    (hr : row.node = none ∨ ∃ p ∈ ps, row.node = some p.node ∧ p.parent ∈ out.map (·.node)) :
    ParentFirst ps (out ++ [row]) := by
  intro i hi
  by_cases lt : i < out.length
  · rw [List.getElem_append_left out [row] lt, List.take_append_of_le_length (Nat.le_of_lt lt)]
    exact h i lt
  · have e : i = out.length := by simp at hi; omega
    subst e
    rw [List.getElem_concat_length out row _ rfl, List.take_left]
    exact hr

theorem waiting_snoc (ps : List Point) (out : List GraphRow) (row : GraphRow)
    (pending : List (Option Nat × Nat)) (h : Waiting ps out pending) :
    Waiting ps (out ++ [row]) pending := by
  intro e he
  rcases h e he with n | ⟨p, hp, ep, pp⟩
  · exact Or.inl n
  · exact Or.inr ⟨p, hp, ep, by simp only [List.map_append, List.mem_append]; exact Or.inl pp⟩

theorem kids_are_children (ps : List Point) (node : Option Nat) (drawn : List Nat) :
    ∀ k ∈ kidsOf ps node drawn, ∃ p ∈ ps, p.node = k ∧ p.parent = node := by
  have go : ∀ (l : List Point) (acc : List Nat), (∀ p ∈ l, p ∈ ps) →
      (∀ k ∈ acc, ∃ p ∈ ps, p.node = k ∧ p.parent = node) →
      ∀ k ∈ l.foldl (fun acc p =>
        if p.parent == node && !drawn.contains p.node && !acc.contains p.node then acc ++ [p.node]
        else acc) acc, ∃ p ∈ ps, p.node = k ∧ p.parent = node := by
    intro l
    induction l with
    | nil => intro acc _ h; exact h
    | cons q rest ih =>
      intro acc sub h
      simp only [List.foldl]
      apply ih _ (fun p hp => sub p (List.mem_cons_of_mem _ hp))
      split
      · rename_i c
        intro k hk
        simp only [List.mem_append, List.mem_singleton] at hk
        rcases hk with hk | hk
        · exact h k hk
        · simp only [Bool.and_eq_true, beq_iff_eq] at c
          exact ⟨q, sub q (List.mem_cons_self _ _), hk.symm, c.1.1⟩
      · exact h
  intro k hk
  exact go ps [] (fun _ hp => hp) (by simp) k hk

theorem step_pending (ps : List Point) (node : Option Nat) (col : Nat)
    (rest : List (Option Nat × Nat)) (open_ : List Bool) (drawn : List Nat) :
    ∀ e ∈ (stepOf ps node col rest open_ drawn).pending,
      e ∈ rest ∨ ∃ k ∈ kidsOf ps node (markDrawn drawn node), e.1 = some k := by
  intro e he
  simp only [stepOf] at he
  generalize kidsOf ps node (markDrawn drawn node) = kids at he ⊢
  cases kids with
  | nil =>
    simp only [List.drop_nil, List.zip_nil_left, List.map_nil, List.head?_nil, Option.map_none',
      Option.toList, List.nil_append] at he
    exact Or.inl he
  | cons k ks =>
    simp only [List.head?_cons, Option.map_some', Option.toList, List.mem_append, List.mem_map,
      List.mem_singleton] at he
    rcases he with (⟨⟨k', c⟩, hz, rfl⟩ | rfl) | he
    · exact Or.inr ⟨k', List.mem_of_mem_drop (List.of_mem_zip hz).1, rfl⟩
    · exact Or.inr ⟨k, List.mem_cons_self _ _, rfl⟩
    · exact Or.inl he

theorem loop_invariant (ps : List Point) (fuel : Nat) :
    ∀ pending open_ drawn out,
      (out.filterMap (·.node)).Nodup → (∀ n ∈ out.filterMap (·.node), n ∈ drawn) →
      ParentFirst ps out → Waiting ps out pending →
      ((graphLoop ps fuel pending open_ drawn out).filterMap (·.node)).Nodup ∧
        ParentFirst ps (graphLoop ps fuel pending open_ drawn out) := by
  induction fuel with
  | zero => intro _ _ _ out nd _ pf _; exact ⟨nd, pf⟩
  | succ fuel ih =>
    intro pending open_ drawn out nd sub pf w
    cases pending with
    | nil => exact ⟨nd, pf⟩
    | cons head rest =>
      rcases head with ⟨node, col⟩
      simp only [graphLoop]
      split
      · exact ih rest open_ drawn out nd sub pf (fun e he => w e (List.mem_cons_of_mem _ he))
      · rename_i unseen
        have rowNode : (stepOf ps node col rest open_ drawn).row.node = node := rfl
        apply ih
        · rw [List.filterMap_append]
          cases node with
          | none =>
            simp only [rowNode, List.filterMap_cons, List.filterMap_nil, List.append_nil]
            exact nd
          | some n =>
            simp only [rowNode, List.filterMap_cons, List.filterMap_nil]
            have notIn : n ∉ out.filterMap (·.node) := by
              intro hn
              apply unseen
              simp only [seenAlready]
              exact List.elem_eq_true_of_mem (sub n hn)
            unfold List.Nodup
            rw [List.pairwise_append]
            refine ⟨nd, List.pairwise_singleton _ _, ?_⟩
            intro a ha b hb
            simp only [List.mem_singleton] at hb
            subst hb
            intro e
            subst e
            exact notIn ha
        · intro m hm
          rw [List.filterMap_append] at hm
          simp only [List.mem_append] at hm
          cases node with
          | none =>
            simp only [rowNode, List.filterMap_cons, List.filterMap_nil, List.not_mem_nil,
              or_false] at hm
            exact sub m hm
          | some n =>
            simp only [rowNode, List.filterMap_cons, List.filterMap_nil, List.mem_singleton] at hm
            simp only [markDrawn, List.mem_append, List.mem_singleton]
            rcases hm with hm | hm
            · exact Or.inl (sub m hm)
            · exact Or.inr hm
        · apply parent_first_snoc ps out _ pf
          rw [rowNode]
          exact w (node, col) (List.mem_cons_self _ _)
        · intro e he
          rcases step_pending ps node col rest open_ drawn e he with hr | ⟨k, hk, ek⟩
          · exact waiting_snoc ps out _ _ (fun e he => w e (List.mem_cons_of_mem _ he)) e hr
          · obtain ⟨p, hp, pk, pp⟩ := kids_are_children ps node _ k hk
            refine Or.inr ⟨p, hp, by rw [ek, pk], ?_⟩
            simp only [List.map_append, List.map_cons, List.map_nil, List.mem_append,
              List.mem_singleton]
            exact Or.inr (by rw [pp, rowNode])

/-- @proves REQ-UNDO.tree_is_drawn -/
theorem each_node_is_drawn_once_under_its_parent (points : List Point) :
    ((historyGraph points).filterMap (·.node)).Nodup ∧
    ∀ i (h : i < (historyGraph points).length), (historyGraph points)[i].node = none ∨
      ∃ p ∈ points, (historyGraph points)[i].node = some p.node ∧
        p.parent ∈ ((historyGraph points).take i).map (·.node) := by
  apply loop_invariant
  · exact List.nodup_nil
  · simp
  · intro i h; simp at h
  · intro e he
    simp only [List.mem_singleton] at he
    subst he
    exact Or.inl rfl

def clampCopy (size : Nat) (span : Span) : Span :=
  { start := min span.start size, stop := min span.stop size, role := span.role,
    actions := span.actions }

def keepCopy (previous : Nat) : List Span → List Span
  | [] => []
  | span :: rest =>
    if span.stop ≤ span.start then keepCopy previous rest
    else if span.start < previous then keepCopy previous rest
    else span :: keepCopy span.stop rest

theorem tidy_shape : ∃ K : Nat → List Span → List Span,
    (∀ size spans, tidy size spans =
      K 0 ((spans.map (clampCopy size)).mergeSort (fun a b => decide (a.start ≤ b.start)))) ∧
    (∀ p, K p [] = []) ∧
    (∀ p x xs, K p (x :: xs) =
      if x.stop ≤ x.start then K p xs else if x.start < p then K p xs else x :: K x.stop xs) :=
  ⟨_, fun _ _ => rfl, fun _ => rfl, fun _ _ _ => rfl⟩

theorem tidy_copy (size : Nat) (spans : List Span) :
    tidy size spans =
      keepCopy 0 ((spans.map (clampCopy size)).mergeSort (fun a b => decide (a.start ≤ b.start))) := by
  obtain ⟨K, ht, hn, hc⟩ := tidy_shape
  rw [ht]
  generalize 0 = prev
  generalize List.mergeSort _ _ = l
  induction l generalizing prev with
  | nil => rw [hn]; rfl
  | cons x xs ih =>
    rw [hc, keepCopy]
    split
    · exact ih prev
    · split
      · exact ih prev
      · rw [ih]

def Ordered (spans : List Span) : Prop := spans.Pairwise (fun a b => a.stop ≤ b.start)

def Within (size : Nat) (spans : List Span) : Prop := ∀ s ∈ spans, s.start ≤ s.stop ∧ s.stop ≤ size

theorem keep_copy_filters (spans : List Span) (ord : Ordered spans) (wf : ∀ s ∈ spans, s.start ≤ s.stop) :
    ∀ prev, (∀ s ∈ spans, s.start < s.stop → prev ≤ s.start) →
      keepCopy prev spans = spans.filter (fun s => decide (s.start < s.stop)) := by
  induction spans with
  | nil => intro _ _; rfl
  | cons x xs ih =>
    intro prev hp
    have ⟨hx, hxs⟩ := List.pairwise_cons.mp ord
    have wfx := wf x (List.mem_cons_self _ _)
    have wfxs := fun s hs => wf s (List.mem_cons_of_mem _ hs)
    rw [keepCopy]
    by_cases e : x.stop ≤ x.start
    · rw [if_pos e, List.filter_cons, if_neg (by simp; omega)]
      exact ih hxs wfxs prev (fun s hs lt => hp s (List.mem_cons_of_mem _ hs) lt)
    · have lt : x.start < x.stop := by omega
      rw [if_neg e, if_neg (by have := hp x (List.mem_cons_self _ _) lt; omega), List.filter_cons,
        if_pos (by simp [lt])]
      rw [ih hxs wfxs x.stop (fun s hs _ => hx s hs)]

theorem tidy_keeps_ordered (size : Nat) (spans : List Span) (ord : Ordered spans)
    (within : Within size spans) : tidy size spans = spans.filter (fun s => decide (s.start < s.stop)) := by
  rw [tidy_copy]
  have clamp : spans.map (clampCopy size) = spans := by
    conv => rhs; rw [← List.map_id spans]
    apply List.map_congr_left
    intro s hs
    have := within s hs
    simp only [clampCopy, id]
    rw [Nat.min_eq_left (by omega), Nat.min_eq_left this.2]
  rw [clamp]
  have sorted : spans.Pairwise (fun a b => decide (a.start ≤ b.start) = true) :=
    List.Pairwise.imp_of_mem (fun {a b} ha _ h => by
      have := within a ha
      simp only [decide_eq_true_eq]
      omega) ord
  rw [List.mergeSort_of_sorted sorted]
  exact keep_copy_filters spans ord (fun s hs => (within s hs).1) 0 (fun _ _ _ => Nat.zero_le _)

theorem ordered_snoc (spans : List Span) (s : Span) (size : Nat) (ord : Ordered spans)
    (within : Within size spans) (after : size ≤ s.start) : Ordered (spans ++ [s]) := by
  unfold Ordered
  rw [List.pairwise_append]
  refine ⟨ord, List.pairwise_singleton _ _, ?_⟩
  intro a ha b hb
  simp only [List.mem_singleton] at hb
  subst hb
  have := within a ha
  omega

def rowRole (all : List Point) (row : GraphRow) : Role :=
  if row.cells.toList.contains '●' then Role.heading
  else if onCurrentBranch all row.node then Role.added
  else Role.entry

def isJump (s : Span) : Bool := s.actions == ["history.jump"]

theorem name_nonempty (node : Option Nat) : 1 ≤ (pointName node).length := by
  cases node with
  | none => simp [pointName, String.length]
  | some n => simp only [pointName, String.length_append]; simp [String.length]

def JumpsNonempty (spans : List Span) : Prop := ∀ s ∈ spans, isJump s = true → s.start < s.stop

theorem tree_row_step (all shown : List Point) (graph : Nat) (acc : String × List Span) (row : GraphRow)
    (ord : Ordered acc.2) (within : Within acc.1.length acc.2) (nj : JumpsNonempty acc.2) :
    Ordered (treeRow all shown graph acc row).2 ∧
      Within (treeRow all shown graph acc row).1.length (treeRow all shown graph acc row).2 ∧
      JumpsNonempty (treeRow all shown graph acc row).2 ∧
      ((treeRow all shown graph acc row).2.filter isJump).map (·.role) =
        (acc.2.filter isJump).map (·.role) ++ [rowRole all row] := by
  have cellsLen : ∀ cs : List Char,
      (String.mk (cs.take graph ++ List.replicate (graph - (cs.take graph).length) ' ')).length = graph := by
    intro cs
    show (cs.take graph ++ List.replicate (graph - (cs.take graph).length) ' ').length = graph
    rw [List.length_append, List.length_replicate, List.length_take]
    omega
  have nm := name_nonempty row.node
  have l1 : "\n".length = 1 := by simp [String.length]
  have l2 : " ".length = 1 := by simp [String.length]
  have l3 : "  ".length = 2 := by simp [String.length]
  simp only [treeRow, String.length_append, cellsLen, l1, l2, l3]
  refine ⟨?_, ?_, ?_, ?_⟩
  · apply ordered_snoc _ _ acc.1.length ord within
    show acc.1.length ≤ acc.1.length + 1 + graph + 1
    omega
  · intro s hs
    simp only [List.mem_append, List.mem_singleton] at hs
    rcases hs with hs | rfl
    · have := within s hs
      omega
    · simp only []
      omega
  · intro s hs hj
    simp only [List.mem_append, List.mem_singleton] at hs
    rcases hs with hs | rfl
    · exact nj s hs hj
    · simp only []
      omega
  · rw [List.filter_append, List.map_append]
    simp [isJump, rowRole]

theorem tree_rows (all shown : List Point) (graph : Nat) (rows : List GraphRow) :
    ∀ acc : String × List Span, Ordered acc.2 → Within acc.1.length acc.2 → JumpsNonempty acc.2 →
      Ordered (rows.foldl (treeRow all shown graph) acc).2 ∧
      Within (rows.foldl (treeRow all shown graph) acc).1.length (rows.foldl (treeRow all shown graph) acc).2 ∧
      JumpsNonempty (rows.foldl (treeRow all shown graph) acc).2 ∧
      ((rows.foldl (treeRow all shown graph) acc).2.filter isJump).map (·.role) =
        (acc.2.filter isJump).map (·.role) ++ rows.map (rowRole all) := by
  induction rows with
  | nil => intro acc o w n; exact ⟨o, w, n, by simp⟩
  | cons row rest ih =>
    intro acc o w n
    have ⟨o', w', n', r'⟩ := tree_row_step all shown graph acc row o w n
    have ⟨o'', w'', n'', r''⟩ := ih _ o' w' n'
    refine ⟨o'', w'', n'', ?_⟩
    simp only [List.foldl] at r'' ⊢
    rw [r'', r', List.map_cons, List.append_assoc, List.singleton_append]

theorem jumps_survive (spans : List Span) (n : JumpsNonempty spans) :
    (spans.filter (fun s => decide (s.start < s.stop))).filter isJump = spans.filter isJump := by
  induction spans with
  | nil => rfl
  | cons x xs ih =>
    have nxs : JumpsNonempty xs := fun s hs => n s (List.mem_cons_of_mem _ hs)
    cases hj : isJump x
    · by_cases lt : x.start < x.stop
      · simp [List.filter_cons, lt, hj, ih nxs]
      · simp [List.filter_cons, lt, hj, ih nxs]
    · have lt := n x (List.mem_cons_self _ _) hj
      simp [List.filter_cons, lt, hj, ih nxs]

theorem view_jumps (all shown : List Point) (graph : Nat) (rows : List GraphRow)
    (acc : String × List Span) (o : Ordered acc.2) (w : Within acc.1.length acc.2)
    (none_yet : acc.2.filter isJump = []) :
    ((tidy (rows.foldl (treeRow all shown graph) acc).1.length
        (rows.foldl (treeRow all shown graph) acc).2).filter isJump).map (·.role) =
      rows.map (rowRole all) := by
  have n : JumpsNonempty acc.2 := by
    intro s hs hj
    have : s ∈ acc.2.filter isJump := List.mem_filter.mpr ⟨hs, hj⟩
    rw [none_yet] at this
    cases this
  have ⟨o', w', n', r'⟩ := tree_rows all shown graph rows acc o w n
  rw [tidy_keeps_ordered _ _ o' w', jumps_survive _ n', r', none_yet]
  rfl

theorem switch_line_facts (filter : Filter) :
    Ordered (switchLine filter).2 ∧ Within (switchLine filter).1.length (switchLine filter).2 ∧
      (switchLine filter).2.filter isJump = [] := by
  cases filter <;>
    simp [switchLine, switches, Ordered, Within, isJump, String.length]

/-- @proves REQ-UNDO.tree_is_shown -/
theorem every_node_is_a_named_jump_with_the_current_marked (points : List Point) (filter : Filter)
    (file : Option String) :
    ((historyView points filter file).spans.filter isJump).map (·.role) =
      (historyGraph (shownPoints points filter (file.getD ""))).map (rowRole points) ∧
    ∀ row ∈ historyGraph (shownPoints points filter (file.getD "")),
      rowRole points row = Role.heading ↔ row.cells.toList.contains '●' = true := by
  constructor
  · have ⟨so, sw, sj⟩ := switch_line_facts filter
    have wide : ∀ h : String, Within ((switchLine filter).1 ++ h).length (switchLine filter).2 := by
      intro h s hs
      have := sw s hs
      refine ⟨this.1, Nat.le_trans this.2 ?_⟩
      simp
    unfold historyView
    cases filter <;> cases file
    case file.some f =>
      exact view_jumps _ _ _ _ ((switchLine Filter.file).1 ++ f, (switchLine Filter.file).2) so (wide f) sj
    all_goals exact view_jumps _ _ _ _ ((switchLine _).1, (switchLine _).2) so sw sj
  · intro row _
    unfold rowRole
    split
    · simp_all
    · rename_i h
      simp only [Bool.not_eq_true] at h
      simp only [h, Bool.false_eq_true, iff_false]
      split <;> simp

theorem near_of_changed (diff : List DiffLine) (i : Nat) (d : DiffLine) (hd : diff[i]? = some d)
    (changed : d.role ≠ Role.plain) : near diff i = true := by
  unfold near
  rw [List.any_eq_true]
  refine ⟨d, ?_, by simpa using changed⟩
  have lt : i < diff.length := (List.getElem?_eq_some.mp hd).1
  apply List.getElem?_mem (n := i - (i - 2))
  rw [List.getElem?_take_of_lt, List.getElem?_drop]
  · rw [show i - 2 + (i - (i - 2)) = i by omega]
    exact hd
  · rw [Nat.min_def]
    split <;> omega

theorem near_lines_keep (diff : List DiffLine) (l : List DiffLine) :
    ∀ i skipped, diff.drop i = l → ∀ d ∈ l, d.role ≠ Role.plain →
      (d.text, d.role) ∈ nearLines diff i skipped l := by
  induction l with
  | nil => intro _ _ _ d hd; cases hd
  | cons d0 rest ih =>
    intro i skipped hl d hd changed
    have at_i : diff[i]? = some d0 := by
      have := List.getElem?_drop diff i 0
      rw [hl, Nat.add_zero] at this
      rw [← this]
      simp
    have next : diff.drop (i + 1) = rest := by
      rw [Nat.add_comm, ← List.drop_drop, hl]
      rfl
    unfold nearLines
    split
    · rcases List.mem_cons.mp hd with rfl | hr
      · exact List.mem_cons_self _ _
      · exact List.mem_cons_of_mem _ (ih (i + 1) false next d hr changed)
    · rename_i notNear
      rcases List.mem_cons.mp hd with rfl | hr
      · exact absurd (near_of_changed diff i d at_i changed) notNear
      · split
        · exact List.mem_cons_of_mem _ (ih (i + 1) true next d hr changed)
        · exact ih (i + 1) true next d hr changed

def Shown (line : String × Role) : Bool := line.2 != Role.plain && decide (0 < line.1.length)

theorem join_step (acc : String × List Span) (line : String × Role) (o : Ordered acc.2)
    (w : Within acc.1.length acc.2) :
    Ordered (joinLines acc line).2 ∧ Within (joinLines acc line).1.length (joinLines acc line).2 ∧
      ((joinLines acc line).2.filter (fun s => decide (s.start < s.stop))).map (·.role) =
        (acc.2.filter (fun s => decide (s.start < s.stop))).map (·.role) ++
          (if Shown line then [line.2] else []) := by
  have grow : acc.1.length ≤ (if acc.1.isEmpty then acc.1 else acc.1 ++ "\n").length := by
    split <;> simp
  simp only [joinLines]
  split
  · rename_i plain
    have plain' : line.2 = Role.plain := by simpa using plain
    have : Shown line = false := by simp [Shown, plain']
    refine ⟨o, fun s hs => ?_, by simp [this]⟩
    have := w s hs
    simp only [String.length_append]
    omega
  · rename_i notPlain
    have notPlain' : line.2 ≠ Role.plain := by simpa using notPlain
    refine ⟨ordered_snoc _ _ _ o w grow, fun s hs => ?_, ?_⟩
    · simp only [List.mem_append, List.mem_singleton] at hs
      rcases hs with hs | rfl
      · have := w s hs
        simp only [String.length_append]
        omega
      · simp only [String.length_append]
        omega
    · rw [List.filter_append, List.map_append]
      by_cases pos : 0 < line.1.length
      · have : Shown line = true := by simp [Shown, notPlain', pos]
        simp [this, pos]
      · have : Shown line = false := by simp [Shown, pos]
        simp [this, pos]

theorem join_lines (lines : List (String × Role)) : ∀ acc : String × List Span,
    Ordered acc.2 → Within acc.1.length acc.2 →
      Ordered (lines.foldl joinLines acc).2 ∧
      Within (lines.foldl joinLines acc).1.length (lines.foldl joinLines acc).2 ∧
      ((lines.foldl joinLines acc).2.filter (fun s => decide (s.start < s.stop))).map (·.role) =
        (acc.2.filter (fun s => decide (s.start < s.stop))).map (·.role) ++
          (lines.filter Shown).map (·.2) := by
  induction lines with
  | nil => intro acc o w; exact ⟨o, w, by simp⟩
  | cons line rest ih =>
    intro acc o w
    have ⟨o', w', r'⟩ := join_step acc line o w
    have ⟨o'', w'', r''⟩ := ih _ o' w'
    refine ⟨o'', w'', ?_⟩
    simp only [List.foldl] at r'' ⊢
    rw [r'', r', List.filter_cons, List.append_assoc]
    split <;> simp_all

/-- @proves REQ-UNDO.hover_shows_change -/
theorem pointing_shows_every_added_and_removed_line (node : Nat) (said : String)
    (changed : List Changed) :
    (∀ c ∈ changed, ∀ d ∈ diffLines (c.before.splitOn "\n") (c.after.splitOn "\n"),
      d.role ≠ Role.plain → (d.text, d.role) ∈ changedLines c) ∧
    (changeView node said changed).spans.map (·.role) =
      (((pointName (some node) ++ "  " ++ said, Role.heading) :: changed.bind changedLines).filter
        Shown).map (·.2) := by
  constructor
  · intro c _ d hd changed
    unfold changedLines
    exact List.mem_cons_of_mem _ (near_lines_keep _ _ 0 false rfl d hd changed)
  · have ⟨o, w, r⟩ := join_lines ((pointName (some node) ++ "  " ++ said, Role.heading) ::
      changed.bind changedLines) ("", []) List.Pairwise.nil (fun s hs => by cases hs)
    unfold changeView
    simp only []
    rw [tidy_keeps_ordered _ _ o w, r]
    rfl

end TraceLean.HistoryView
