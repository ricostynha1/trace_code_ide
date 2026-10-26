import TraceLean.Anchor
import TraceLean.Protocol
import TraceLean.Strength
import TraceLean.DocLink

namespace TraceLean.SpecTrace

open TraceLean.Evidence
open TraceLean.Strength
open TraceLean.DocLink
open Lean (Json)

/-- @specifies REQ-DRT-PROTO.reply_exclusive -/
def ReplyExclusive (output : Option Json) (error : Option String) (y : Bool) : Prop :=
  y = true ↔ ((output ≠ none ∧ error = none) ∨ (output = none ∧ error ≠ none))

/-- @pins REQ-DRT-PROTO.reply_exclusive -/
theorem reply_exclusive_pinned :
    (∀ x1 x2, ReplyExclusive x1 x2 (TraceLean.Protocol.isExclusive x1 x2)) ∧
    (∀ x1 x2 y1 y2, ReplyExclusive x1 x2 y1 → ReplyExclusive x1 x2 y2 → y1 = y2) := by
  constructor
  · intro x1 x2
    unfold ReplyExclusive TraceLean.Protocol.isExclusive
    cases x1
    · cases x2
      · simp
      · simp
    · cases x2
      · simp
      · simp
  · intro x1 x2 y1 y2 h1 h2
    cases y1
    · cases y2
      · rfl
      · have h3 := h1.2 (h2.1 rfl)
        cases h3
    · cases y2
      · have h3 := h2.2 (h1.1 rfl)
        cases h3
      · rfl

/-- @specifies REQ-ANCHOR.imprecise_capped -/
def CappedLevel (precise : Bool) (claimed : Level) (y : Level) : Prop :=
  (precise = true → y = claimed) ∧ (precise = false → ∀ l : Level, y ≤ l)

/-- @pins REQ-ANCHOR.imprecise_capped -/
theorem imprecise_capped_pinned :
    (∀ x1 x2, CappedLevel x1 x2 (TraceLean.Anchor.anchorCeiling x1 x2)) ∧
    (∀ x1 x2 y1 y2, CappedLevel x1 x2 y1 → CappedLevel x1 x2 y2 → y1 = y2) := by
  constructor
  · intro x1 x2
    unfold CappedLevel TraceLean.Anchor.anchorCeiling
    cases x1
    · constructor
      · intro h
        cases h
      · intro _ l
        show Level.L1.toNat ≤ l.toNat
        cases l
        all_goals decide
    · constructor
      · intro _
        rfl
      · intro h
        cases h
  · intro x1 x2 y1 y2 h1 h2
    cases x1
    · have a := h1.2 rfl y2
      have b := h2.2 rfl y1
      revert a b
      cases y1
      all_goals cases y2
      all_goals decide
    · exact (h1.1 rfl).trans (h2.1 rfl).symm

def HashedNow (current : List (String × String)) (target hash : String) : Prop :=
  ∃ pre post, current = pre ++ (target, hash) :: post ∧ ∀ p, List.Mem p pre → p.1 ≠ target

/-- @specifies REQ-DOCLINK.hash_moves_review
@specifies REQ-DOCLINK.dangling_reported -/
def DocStanding (link : DocLink) (current : List (String × String)) (y : DocState) : Prop :=
  ((∀ p, List.Mem p current → p.1 ≠ link.target) → y = DocState.dangling) ∧
  ∀ now, HashedNow current link.target now →
    (now = link.recordedHash → y = DocState.current) ∧
    (now ≠ link.recordedHash → y = DocState.inReview link.recordedHash now)

theorem lookup_none (current : List (String × String)) (target : String) :
    lookupHash current target = none ↔ ∀ p, List.Mem p current → p.1 ≠ target := by
  unfold lookupHash
  simp only [Option.map_eq_none', List.find?_eq_none, beq_iff_eq]
  constructor
  · intro h p hp
    exact h p hp
  · intro h p hp
    exact h p hp

theorem hashed_now_nil (target hash : String) : ¬ HashedNow [] target hash := by
  intro h
  refine h.elim ?_
  intro pre h
  refine h.elim ?_
  intro post he
  have h2 := congrArg List.length he.1
  simp at h2
  omega

theorem lookup_cons (q : String × String) (rest : List (String × String)) (target : String) :
    lookupHash (q :: rest) target =
      if q.1 == target then some q.2 else lookupHash rest target := by
  unfold lookupHash
  rw [List.find?_cons]
  split
  · rename_i heq
    simp [heq]
  · rename_i heq
    simp [heq]

theorem lookup_some (target hash : String) (current : List (String × String)) :
    lookupHash current target = some hash ↔ HashedNow current target hash := by
  induction current
  case nil =>
    unfold lookupHash
    simp only [List.find?_nil, Option.map_none', reduceCtorEq, false_iff]
    exact hashed_now_nil target hash
  case cons q rest ih =>
    rw [lookup_cons]
    unfold HashedNow
    refine (Classical.em (q.1 = target)).elim ?_ ?_
    · intro hqe
      have hq : (q.1 == target) = true := by
        simp [hqe]
      rw [hq]
      rw [if_pos rfl]
      simp only [Option.some.injEq]
      constructor
      · intro h
        have he : q :: rest = List.nil ++ (target, hash) :: rest := by
          cases q
          simp_all
        refine ⟨[], rest, he, ?_⟩
        intro p hp
        cases hp
      · intro h
        refine h.elim ?_
        intro pre h0
        refine h0.elim ?_
        intro post he
        clear h0
        cases pre
        case nil =>
          have he2 := he.1
          simp at he2
          rw [he2.1]
        case cons d pre =>
          have he2 := he.1
          simp at he2
          have hd := he.2 d (List.Mem.head _)
          rw [← he2.1] at hd
          exact absurd hqe hd
    · intro hqe
      have hq : (q.1 == target) = false := by
        simp [hqe]
      rw [hq]
      simp only [Bool.false_eq_true, if_false]
      rw [ih]
      unfold HashedNow
      constructor
      · intro h
        refine h.elim ?_
        intro pre h
        refine h.elim ?_
        intro post he
        have he2 : q :: rest = (q :: pre) ++ (target, hash) :: post := by
          simp [he.1]
        refine ⟨q :: pre, post, he2, ?_⟩
        intro p hp
        cases hp
        · exact hqe
        · rename_i hm
          exact he.2 p hm
      · intro h
        refine h.elim ?_
        intro pre h0
        refine h0.elim ?_
        intro post he
        clear h0
        cases pre
        case nil =>
          have he2 := he.1
          simp at he2
          rw [he2.1] at hqe
          exact absurd rfl hqe
        case cons d pre =>
          have he2 := he.1
          simp at he2
          refine ⟨pre, post, he2.2, ?_⟩
          intro p hp
          exact he.2 p (List.Mem.tail _ hp)

/-- @pins REQ-DOCLINK.hash_moves_review
@pins REQ-DOCLINK.dangling_reported -/
theorem doc_standing_pinned :
    (∀ x1 x2, DocStanding x1 x2 (docState x1 x2)) ∧
    (∀ x1 x2 y1 y2, DocStanding x1 x2 y1 → DocStanding x1 x2 y2 → y1 = y2) := by
  constructor
  · intro x1 x2
    unfold DocStanding
    constructor
    · intro h
      unfold docState
      rw [(lookup_none x2 x1.target).2 h]
    · intro now hn
      have hl := (lookup_some x1.target now x2).2 hn
      unfold docState
      rw [hl]
      constructor
      · intro he
        simp [he]
      · intro he
        simp [he]
  · intro x1 x2 y1 y2 h1 h2
    refine (Classical.em (lookupHash x2 x1.target = none)).elim ?_ ?_
    · intro hl
      have ha := (lookup_none x2 x1.target).1 hl
      exact (h1.1 ha).trans (h2.1 ha).symm
    · intro hl
      have hs := Option.ne_none_iff_exists'.1 hl
      refine hs.elim ?_
      intro now hnow
      have hn := (lookup_some x1.target now x2).1 hnow
      have a1 := h1.2 now hn
      have a2 := h2.2 now hn
      refine (Classical.em (now = x1.recordedHash)).elim ?_ ?_
      · intro he
        exact (a1.1 he).trans (a2.1 he).symm
      · intro he
        exact (a1.2 he).trans (a2.2 he).symm

/-- @proves REQ-DOCLINK.dangling_reported -/
theorem a_link_to_nothing_is_dangling (link : DocLink) (current : List (String × String)) :
    DocStanding link current (docState link current) :=
  doc_standing_pinned.1 link current

end TraceLean.SpecTrace
