import Lean

namespace TraceLean.Diff

open Lean (ToJson FromJson)

inductive Mark where
  | annotated
  | judged
  | tested
  | proved
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def allMarks : List Mark := [.annotated, .judged, .tested, .proved]

structure Linked where
  role : String
  symbol : String
  bodyHash : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure ClauseState where
  reqId : String
  clause : Option String
  text : String
  held : List Mark
  links : List Linked
  assumed : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Change where
  | added
  | removed
  | reworded (before : String) (after : String)
  | markLost (mark : Mark)
  | markGained (mark : Mark)
  | linkRemoved (role : String) (symbol : String)
  | linkAdded (role : String) (symbol : String)
  | bodyChanged (role : String) (symbol : String)
  | assumptionDropped (name : String)
  | assumptionAdded (name : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure ClauseDiff where
  reqId : String
  clause : Option String
  changes : List Change
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Change.isLoss : Change → Bool
  | .removed => true
  | .markLost _ => true
  | _ => false

def sameLink (a b : Linked) : Bool := a.role == b.role && a.symbol == b.symbol

def empty (reqId : String) (clause : Option String) : ClauseState := ⟨reqId, clause, "", [], [], []⟩

def presence : Option ClauseState → Option ClauseState → List Change
  | some _, none => [.removed]
  | none, some _ => [.added]
  | _, _ => []

def textChange : Option ClauseState → Option ClauseState → List Change
  | some b, some a => if b.text == a.text then [] else [.reworded b.text a.text]
  | _, _ => []

def markChanges (b a : ClauseState) : List Change :=
  (allMarks.filter (fun m => b.held.contains m && !a.held.contains m)).map .markLost ++
  (allMarks.filter (fun m => !b.held.contains m && a.held.contains m)).map .markGained

def bodyMoved (a : ClauseState) (l : Linked) : Bool :=
  a.links.any (sameLink l) && !a.links.contains l

def linkChanges (b a : ClauseState) : List Change :=
  (b.links.filter (fun l => !a.links.any (sameLink l))).map (fun l => .linkRemoved l.role l.symbol) ++
  (a.links.filter (fun l => !b.links.any (sameLink l))).map (fun l => .linkAdded l.role l.symbol) ++
  (b.links.filter (bodyMoved a)).map (fun l => .bodyChanged l.role l.symbol)

def assumptionChanges (b a : ClauseState) : List Change :=
  (b.assumed.filter (fun x => !a.assumed.contains x)).map .assumptionDropped ++
  (a.assumed.filter (fun x => !b.assumed.contains x)).map .assumptionAdded

def changesOf (reqId : String) (clause : Option String) (before after : Option ClauseState) : List Change :=
  let b := before.getD (empty reqId clause)
  let a := after.getD (empty reqId clause)
  presence before after ++ textChange before after ++ markChanges b a ++ linkChanges b a ++
    assumptionChanges b a

def keyLe (a b : String × Option String) : Bool :=
  if a.1 == b.1 then
    match a.2, b.2 with
    | none, _ => true
    | some _, none => false
    | some x, some y => decide (x ≤ y)
  else decide (a.1 ≤ b.1)

def uniq (xs : List (String × Option String)) : List (String × Option String) :=
  xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) []

def keys (before after : List ClauseState) : List (String × Option String) :=
  (uniq ((before ++ after).map (fun s => (s.reqId, s.clause)))).mergeSort keyLe

def find (states : List ClauseState) (key : String × Option String) : Option ClauseState :=
  states.find? (fun s => s.reqId == key.1 && s.clause == key.2)

def diffOf (before after : List ClauseState) (key : String × Option String) : ClauseDiff :=
  ⟨key.1, key.2, changesOf key.1 key.2 (find before key) (find after key)⟩

def isLoss (d : ClauseDiff) : Bool := d.changes.any Change.isLoss

/-- @models REQ-DIFF.per_clause
@models REQ-DIFF.text_shown
@models REQ-DIFF.marks_moved
@models REQ-DIFF.links_moved
@models REQ-DIFF.assumptions_moved
@models REQ-DIFF.unchanged_omitted
@models REQ-DIFF.losses_first -/
def diff (before after : List ClauseState) : List ClauseDiff :=
  let all := ((keys before after).map (diffOf before after)).filter (fun d => !d.changes.isEmpty)
  all.filter isLoss ++ all.filter (fun d => !isLoss d)

theorem mem_diff (before after : List ClauseState) (d : ClauseDiff) :
    d ∈ diff before after ↔
      (∃ key ∈ keys before after, diffOf before after key = d) ∧ d.changes ≠ [] := by
  unfold diff
  simp only [List.mem_append, List.mem_filter, List.mem_map]
  constructor
  · rintro (⟨⟨⟨key, hk, rfl⟩, ne⟩, _⟩ | ⟨⟨⟨key, hk, rfl⟩, ne⟩, _⟩) <;>
      exact ⟨⟨key, hk, rfl⟩, by simpa using ne⟩
  · rintro ⟨⟨key, hk, rfl⟩, ne⟩
    by_cases h : isLoss (diffOf before after key) = true
    · exact Or.inl ⟨⟨⟨key, hk, rfl⟩, by simpa using ne⟩, h⟩
    · exact Or.inr ⟨⟨⟨key, hk, rfl⟩, by simpa using ne⟩, by simpa using h⟩

theorem mem_uniq_fold (xs : List (String × Option String)) :
    ∀ acc x, x ∈ xs.foldl (fun acc x => if acc.contains x then acc else acc ++ [x]) acc ↔ x ∈ acc ∨ x ∈ xs := by
  induction xs with
  | nil => intro acc x; simp
  | cons y rest ih =>
    intro acc x
    simp only [List.foldl]
    rw [ih]
    by_cases h : acc.contains y = true
    · rw [if_pos h]
      have hy : y ∈ acc := by simpa using h
      constructor
      · rintro (k | k)
        · exact Or.inl k
        · exact Or.inr (List.mem_cons_of_mem _ k)
      · rintro (k | k)
        · exact Or.inl k
        · rcases List.mem_cons.mp k with rfl | k
          · exact Or.inl hy
          · exact Or.inr k
    · rw [if_neg h]
      simp only [List.mem_append, List.mem_singleton, List.mem_cons, List.not_mem_nil, or_false]
      constructor
      · rintro ((k | rfl) | k)
        · exact Or.inl k
        · exact Or.inr (Or.inl rfl)
        · exact Or.inr (Or.inr k)
      · rintro (k | rfl | k)
        · exact Or.inl (Or.inl k)
        · exact Or.inl (Or.inr rfl)
        · exact Or.inr k

theorem mem_keys (before after : List ClauseState) (key : String × Option String) :
    key ∈ keys before after ↔ (∃ s ∈ before, (s.reqId, s.clause) = key) ∨ ∃ s ∈ after, (s.reqId, s.clause) = key := by
  unfold keys uniq
  rw [List.mem_mergeSort, mem_uniq_fold]
  simp [List.mem_map, List.mem_append, or_and_right, exists_or]

theorem find_some (states : List ClauseState) (key : String × Option String) (s : ClauseState)
    (h : find states key = some s) : s ∈ states ∧ (s.reqId, s.clause) = key := by
  unfold find at h
  have hs := List.find?_some h
  simp only [Bool.and_eq_true, beq_iff_eq] at hs
  exact ⟨List.mem_of_find?_eq_some h, by rw [hs.1, hs.2]⟩

theorem any_false (xs : List Linked) (p : Linked → Bool) (h : ∀ x ∈ xs, p x = false) : xs.any p = false := by
  induction xs with
  | nil => rfl
  | cons y rest ih => simp [h y (List.mem_cons_self _ _), ih (fun x hx => h x (List.mem_cons_of_mem _ hx))]

/-- @proves REQ-DIFF.per_clause -/
theorem a_clause_present_on_one_side_only_is_added_or_removed (before after : List ClauseState)
    (key : String × Option String) :
    (find before key = none → (∃ s, find after key = some s) →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ d.changes.head? = some .added) ∧
    ((∃ s, find before key = some s) → find after key = none →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ d.changes.head? = some .removed) := by
  constructor
  · rintro hb ⟨s, ha⟩
    refine ⟨diffOf before after key, (mem_diff _ _ _).mpr ⟨⟨key, ?_, rfl⟩, ?_⟩, rfl, rfl, ?_⟩
    · exact (mem_keys _ _ _).mpr (Or.inr ⟨s, (find_some _ _ _ ha).1, (find_some _ _ _ ha).2⟩)
    · simp [diffOf, changesOf, hb, ha, presence]
    · simp [diffOf, changesOf, hb, ha, presence]
  · rintro ⟨s, hb⟩ ha
    refine ⟨diffOf before after key, (mem_diff _ _ _).mpr ⟨⟨key, ?_, rfl⟩, ?_⟩, rfl, rfl, ?_⟩
    · exact (mem_keys _ _ _).mpr (Or.inl ⟨s, (find_some _ _ _ hb).1, (find_some _ _ _ hb).2⟩)
    · simp [diffOf, changesOf, hb, ha, presence]
    · simp [diffOf, changesOf, hb, ha, presence]

/-- @proves REQ-DIFF.text_shown -/
theorem a_reworded_clause_shows_both_texts (before after : List ClauseState) (key : String × Option String)
    (b a : ClauseState) (hb : find before key = some b) (ha : find after key = some a) (h : b.text ≠ a.text) :
    ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.reworded b.text a.text ∈ d.changes := by
  refine ⟨diffOf before after key, (mem_diff _ _ _).mpr ⟨⟨key, ?_, rfl⟩, ?_⟩, rfl, rfl, ?_⟩
  · exact (mem_keys _ _ _).mpr (Or.inl ⟨b, (find_some _ _ _ hb).1, (find_some _ _ _ hb).2⟩)
  · simp [diffOf, changesOf, hb, ha, presence, textChange, h]
  · simp [diffOf, changesOf, hb, ha, presence, textChange, h]

theorem changes_of_key (before after : List ClauseState) (key : String × Option String) (c : Change)
    (hc : c ∈ changesOf key.1 key.2 (find before key) (find after key)) (hn : (key ∈ keys before after)) :
    ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ c ∈ d.changes :=
  ⟨diffOf before after key, (mem_diff _ _ _).mpr ⟨⟨key, hn, rfl⟩, List.ne_nil_of_mem hc⟩, rfl, rfl, hc⟩

theorem key_of_side (before after : List ClauseState) (key : String × Option String)
    (h : (∃ s, find before key = some s) ∨ ∃ s, find after key = some s) : key ∈ keys before after := by
  rcases h with ⟨s, hs⟩ | ⟨s, hs⟩
  · exact (mem_keys _ _ _).mpr (Or.inl ⟨s, (find_some _ _ _ hs).1, (find_some _ _ _ hs).2⟩)
  · exact (mem_keys _ _ _).mpr (Or.inr ⟨s, (find_some _ _ _ hs).1, (find_some _ _ _ hs).2⟩)

/-- @proves REQ-DIFF.marks_moved -/
theorem every_mark_lost_or_gained_is_reported (before after : List ClauseState) (key : String × Option String)
    (b a : ClauseState) (hb : find before key = some b) (ha : find after key = some a) (m : Mark) :
    (m ∈ b.held → m ∉ a.held → ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.markLost m ∈ d.changes) ∧
    (m ∉ b.held → m ∈ a.held → ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.markGained m ∈ d.changes) := by
  have hk := key_of_side before after key (Or.inl ⟨b, hb⟩)
  have all : m ∈ allMarks := by cases m <;> simp [allMarks]
  constructor
  · intro hin hout
    apply changes_of_key before after key _ _ hk
    simp [changesOf, hb, ha, markChanges, hin, hout, all]
  · intro hout hin
    apply changes_of_key before after key _ _ hk
    simp [changesOf, hb, ha, markChanges, hin, hout, all]

/-- @proves REQ-DIFF.links_moved -/
theorem every_annotation_added_removed_or_moved_is_reported (before after : List ClauseState)
    (key : String × Option String) (b a : ClauseState) (hb : find before key = some b)
    (ha : find after key = some a) (l : Linked) :
    (l ∈ b.links → (∀ m ∈ a.links, sameLink l m = false) →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.linkRemoved l.role l.symbol ∈ d.changes) ∧
    (l ∈ a.links → (∀ m ∈ b.links, sameLink l m = false) →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.linkAdded l.role l.symbol ∈ d.changes) ∧
    (l ∈ b.links → (∃ m ∈ a.links, sameLink l m = true) → l ∉ a.links →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.bodyChanged l.role l.symbol ∈ d.changes) := by
  have hk := key_of_side before after key (Or.inl ⟨b, hb⟩)
  refine ⟨fun hin hout => ?_, fun hin hout => ?_, fun hin hsame hnot => ?_⟩
  · apply changes_of_key before after key _ _ hk
    have none := any_false _ _ hout
    simp only [changesOf, hb, ha, Option.getD_some, linkChanges, List.mem_append, List.mem_map, List.mem_filter]
    exact Or.inl (Or.inr (Or.inl (Or.inl ⟨l, ⟨hin, by simp [none]⟩, rfl⟩)))
  · apply changes_of_key before after key _ _ hk
    have none := any_false _ _ hout
    simp only [changesOf, hb, ha, Option.getD_some, linkChanges, List.mem_append, List.mem_map, List.mem_filter]
    exact Or.inl (Or.inr (Or.inl (Or.inr ⟨l, ⟨hin, by simp [none]⟩, rfl⟩)))
  · apply changes_of_key before after key _ _ hk
    have some : a.links.any (sameLink l) = true := List.any_eq_true.mpr hsame
    have absent : a.links.contains l = false := by simpa using hnot
    simp only [changesOf, hb, ha, Option.getD_some, linkChanges, List.mem_append, List.mem_map, List.mem_filter]
    exact Or.inl (Or.inr (Or.inr ⟨l, ⟨hin, by simp [bodyMoved, some, absent, hnot]⟩, rfl⟩))

/-- @proves REQ-DIFF.assumptions_moved -/
theorem every_axiom_taken_on_or_let_go_is_reported (before after : List ClauseState)
    (key : String × Option String) (b a : ClauseState) (hb : find before key = some b)
    (ha : find after key = some a) (x : String) :
    (x ∈ b.assumed → x ∉ a.assumed →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.assumptionDropped x ∈ d.changes) ∧
    (x ∉ b.assumed → x ∈ a.assumed →
      ∃ d ∈ diff before after, d.reqId = key.1 ∧ d.clause = key.2 ∧ Change.assumptionAdded x ∈ d.changes) := by
  have hk := key_of_side before after key (Or.inl ⟨b, hb⟩)
  constructor
  · intro hin hout
    apply changes_of_key before after key _ _ hk
    simp only [changesOf, hb, ha, Option.getD_some, assumptionChanges, List.mem_append, List.mem_map, List.mem_filter]
    exact Or.inr (Or.inl ⟨x, ⟨hin, by simpa using hout⟩, rfl⟩)
  · intro hout hin
    apply changes_of_key before after key _ _ hk
    simp only [changesOf, hb, ha, Option.getD_some, assumptionChanges, List.mem_append, List.mem_map, List.mem_filter]
    exact Or.inr (Or.inr ⟨x, ⟨hin, by simpa using hout⟩, rfl⟩)

theorem unchanged_has_no_changes (s : ClauseState) :
    changesOf s.reqId s.clause (some s) (some s) = [] := by
  have links : s.links.filter (fun l => !s.links.any (sameLink l)) = [] := by
    apply List.filter_eq_nil.mpr
    intro l hl
    have : s.links.any (sameLink l) = true := List.any_eq_true.mpr ⟨l, hl, by simp [sameLink]⟩
    simp [this]
  have moved : s.links.filter (bodyMoved s) = [] := by
    apply List.filter_eq_nil.mpr
    intro l hl
    simp [bodyMoved, hl]
  have marks : markChanges s s = [] := by
    simp only [markChanges, List.append_eq_nil, List.map_eq_nil]
    constructor <;> (apply List.filter_eq_nil.mpr; intro m _; cases s.held.contains m <;> simp)
  have assumed : assumptionChanges s s = [] := by
    simp only [assumptionChanges, List.append_eq_nil, List.map_eq_nil]
    constructor <;> (apply List.filter_eq_nil.mpr; intro x hx; simp [hx])
  simp [changesOf, presence, textChange, marks, linkChanges, links, moved, assumed]

/-- @proves REQ-DIFF.unchanged_omitted -/
theorem a_clause_the_change_left_alone_is_not_listed (before after : List ClauseState)
    (key : String × Option String) (s : ClauseState) (hb : find before key = some s)
    (ha : find after key = some s) (d : ClauseDiff) (hd : d ∈ diff before after) :
    ¬ (d.reqId = key.1 ∧ d.clause = key.2) := by
  rintro ⟨e1, e2⟩
  obtain ⟨⟨k, _, rfl⟩, ne⟩ := (mem_diff _ _ _).mp hd
  have hk : k = key := Prod.ext e1 e2
  subst hk
  have hs := (find_some _ _ _ hb).2
  apply ne
  simp only [diffOf, hb, ha]
  rw [← hs]
  exact unchanged_has_no_changes s

/-- @proves REQ-DIFF.losses_first -/
theorem every_loss_is_listed_before_every_other_change (before after : List ClauseState) :
    ∃ losses rest, diff before after = losses ++ rest ∧
      (∀ d ∈ losses, isLoss d = true) ∧ (∀ d ∈ rest, isLoss d = false) := by
  refine ⟨_, _, rfl, fun d hd => (List.mem_filter.mp hd).2, fun d hd => ?_⟩
  simpa using (List.mem_filter.mp hd).2

end TraceLean.Diff
