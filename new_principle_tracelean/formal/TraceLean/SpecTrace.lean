import TraceLean.Anchor
import TraceLean.Protocol
import TraceLean.Pinning
import TraceLean.DocLink

/-!
# Specifications that pin trace-side models

Each clause below is modelled twice: by a specification, a predicate read off
the requirement saying which answers are right, and by the model the
implementation is tested against. The `@pins` theorem says the model meets the
specification and that no input has two answers the specification accepts.

No specification here mentions its model. Where one needs a notion the model
also needs -- a substring, a lookup -- it states it in its own terms and a lemma
shows the model's helper agrees.
-/

namespace TraceLean.SpecTrace

open TraceLean.Evidence
open TraceLean.Strength
open TraceLean.Pinning
open TraceLean.DocLink
open Lean (Json)

/-! ## A reply carries one of an output and an error -/

/-- A reply is exclusive when it carries an output and no error, or an error
and no output.

@specifies REQ-DRT-PROTO.reply_exclusive -/
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

/-! ## An imprecise anchor caps its links -/

/-- What a link through an anchor may reach: whatever it claims through an
anchor the grammar found, and only the lowest level of the ladder -- the level
at or below every other -- through one that names the whole file because no
grammar was available.

@specifies REQ-ANCHOR.imprecise_capped -/
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

/-! ## A verdict holds only for what it was given -/

/-- A kept verdict that Lean accepted exactly this theorem, about exactly these
declarations. -/
def VerdictFor (record : Option PinRecord) (t key : String) : Prop :=
  ∃ r, record = some r ∧ r.pinned = true ∧ r.theoremName = t ∧ r.key = key

/-- Where a clause stands. With no pinning theorem it is open. With one, it is
pinned only when the verdict kept is Lean's acceptance of that theorem for the
declarations as they now are; a verdict about anything else leaves it
attempted.

@specifies REQ-STRENGTH.verdict_kept -/
def VerdictStanding (theoremName : Option String) (record : Option PinRecord) (key : String)
    (s : Strength) : Prop :=
  (theoremName = none → s = Strength.«open») ∧
  ∀ t, theoremName = some t →
    (VerdictFor record t key → s = Strength.pinned t) ∧
    (¬ VerdictFor record t key → s = Strength.attempted t)

/-- @pins REQ-STRENGTH.verdict_kept -/
theorem verdict_kept_pinned :
    (∀ x1 x2 x3, VerdictStanding x1 x2 x3 (TraceLean.Pinning.standing x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, VerdictStanding x1 x2 x3 y1 → VerdictStanding x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3
    unfold VerdictStanding
    constructor
    · intro h
      subst h
      rfl
    · intro t h
      subst h
      unfold VerdictFor TraceLean.Pinning.standing
      cases x2
      case none =>
        constructor
        · intro hv
          refine hv.elim ?_
          intro r hr
          cases hr.1
        · intro _
          rfl
      case some r =>
        constructor
        · intro hv
          refine hv.elim ?_
          intro r2 hr
          cases hr.1
          simp [hr.2.1, hr.2.2.1, hr.2.2.2]
        · intro hv
          have hn : ¬ ((r.pinned = true ∧ r.theoremName = t) ∧ r.key = x3) := by
            intro hc
            exact hv ⟨r, rfl, hc.1.1, hc.1.2, hc.2⟩
          simp only [Bool.and_eq_true, beq_iff_eq]
          rw [if_neg hn]
  · intro x1 x2 x3 y1 y2 h1 h2
    cases x1
    case none =>
      exact (h1.1 rfl).trans (h2.1 rfl).symm
    case some t =>
      have a1 := h1.2 t rfl
      have a2 := h2.2 t rfl
      refine (Classical.em (VerdictFor x2 t x3)).elim ?_ ?_
      · intro hv
        exact (a1.1 hv).trans (a2.1 hv).symm
      · intro hv
        exact (a1.2 hv).trans (a2.2 hv).symm

/-! ## Only the kernel decides -/

/-- `pat` appears somewhere in `text`. -/
def Contains (text pat : String) : Prop :=
  ∃ pre post, text.data = pre ++ pat.data ++ post

/-- `text` begins with `pat`. -/
def StartsWith (text pat : String) : Prop :=
  ∃ post, text.data = pat.data ++ post

theorem occursIn_nil (pat : List Char) :
    TraceLean.Pinning.occursIn pat [] = true ↔ ∃ pre post, List.nil = pre ++ pat ++ post := by
  unfold TraceLean.Pinning.occursIn
  constructor
  · intro h
    have hp : pat = [] := by
      cases pat
      · rfl
      · simp at h
    have he : List.nil = List.nil ++ pat ++ List.nil := by
      simp [hp]
    exact ⟨[], [], he⟩
  · intro h
    refine h.elim ?_
    intro pre h
    refine h.elim ?_
    intro post he
    have h3 : pat.length = 0 := by
      have h2 := congrArg List.length he
      simp only [List.length_append, List.length_nil] at h2
      omega
    have hp : pat = [] := List.eq_nil_of_length_eq_zero h3
    simp [hp]

theorem occursIn_iff (pat : List Char) (text : List Char) :
    TraceLean.Pinning.occursIn pat text = true ↔ ∃ pre post, text = pre ++ pat ++ post := by
  induction text
  case nil => exact occursIn_nil pat
  case cons c rest ih =>
    unfold TraceLean.Pinning.occursIn
    rw [Bool.or_eq_true, List.isPrefixOf_iff_prefix, ih]
    constructor
    · intro h
      refine h.elim ?_ ?_
      · intro hp
        refine hp.elim ?_
        intro post he
        have he2 : c :: rest = List.nil ++ pat ++ post := by
          simp [he]
        exact ⟨[], post, he2⟩
      · intro hr
        refine hr.elim ?_
        intro pre hr
        refine hr.elim ?_
        intro post he
        have he2 : c :: rest = c :: pre ++ pat ++ post := by
          simp [he]
        exact ⟨c :: pre, post, he2⟩
    · intro h
      refine h.elim ?_
      intro pre h
      refine h.elim ?_
      intro post he
      cases pre
      case nil =>
        have he2 : pat ++ post = c :: rest := by
          simp [he]
        exact Or.inl ⟨post, he2⟩
      case cons d pre =>
        have he2 : c :: rest = d :: (pre ++ pat ++ post) := by
          simpa using he
        cases he2
        exact Or.inr ⟨pre, post, rfl⟩

theorem holds_iff (text pat : String) :
    TraceLean.Pinning.holds text pat = true ↔ Contains text pat := by
  unfold TraceLean.Pinning.holds Contains
  exact occursIn_iff pat.data text.data

theorem not_holds (text pat : String) (h : ¬ Contains text pat) :
    TraceLean.Pinning.holds text pat = false := by
  apply Bool.eq_false_iff.2
  intro hh
  exact h ((holds_iff _ _).1 hh)

theorem starts_iff (line pat : String) :
    List.IsPrefix pat.data line.data ↔ StartsWith line pat := by
  unfold StartsWith
  constructor
  · intro h
    refine h.elim ?_
    intro post hp
    exact ⟨post, hp.symm⟩
  · intro h
    refine h.elim ?_
    intro post hp
    exact ⟨post, hp.symm⟩

/-- Whether Lean accepted a pinning check: it exited cleanly, reported no
error, and some line of what it printed lists the axioms of exactly this one
theorem, without `sorryAx` among them.

@specifies REQ-STRENGTH.kernel_decides -/
def KernelAccepted (theoremName output : String) (exitedOk : Bool) (y : Bool) : Prop :=
  y = true ↔
    (exitedOk = true ∧ ¬ Contains output ": error" ∧
      ∃ line, List.Mem line (output.splitOn "\n") ∧
        StartsWith line ("'" ++ theoremName ++ "'") ∧
        (Contains line "does not depend on any axioms" ∨
          (Contains line "depends on axioms" ∧ ¬ Contains line "sorryAx")))

/-- @pins REQ-STRENGTH.kernel_decides -/
theorem kernel_decides_pinned :
    (∀ x1 x2 x3, KernelAccepted x1 x2 x3 (TraceLean.Pinning.accepted x1 x2 x3)) ∧
    (∀ x1 x2 x3 y1 y2, KernelAccepted x1 x2 x3 y1 → KernelAccepted x1 x2 x3 y2 → y1 = y2) := by
  constructor
  · intro x1 x2 x3
    unfold KernelAccepted TraceLean.Pinning.accepted TraceLean.Pinning.clears
    simp only [Bool.and_eq_true, Bool.not_eq_true', Bool.or_eq_true, List.any_eq_true]
    simp only [List.isPrefixOf_iff_prefix]
    constructor
    · intro h
      have hx := h.1.1
      have he := h.1.2
      refine h.2.elim ?_
      intro line hl
      have hm := hl.1
      have hp := hl.2.1
      have hc := hl.2.2
      refine ⟨hx, ?_, line, hm, (starts_iff _ _).1 hp, ?_⟩
      · intro hc2
        rw [← holds_iff] at hc2
        rw [hc2] at he
        cases he
      · refine hc.elim ?_ ?_
        · intro h1
          exact Or.inl ((holds_iff _ _).1 h1)
        · intro h2
          refine Or.inr ⟨(holds_iff _ _).1 h2.1, ?_⟩
          intro hs
          rw [← holds_iff] at hs
          have h3 := h2.2
          rw [hs] at h3
          cases h3
    · intro h
      have hx := h.1
      have he := h.2.1
      refine h.2.2.elim ?_
      intro line hl
      have hm := hl.1
      have hp := hl.2.1
      have hc := hl.2.2
      refine ⟨⟨hx, not_holds _ _ he⟩, line, hm, (starts_iff _ _).2 hp, ?_⟩
      refine hc.elim ?_ ?_
      · intro h1
        exact Or.inl ((holds_iff _ _).2 h1)
      · intro h2
        exact Or.inr ⟨(holds_iff _ _).2 h2.1, not_holds _ _ h2.2⟩
  · intro x1 x2 x3 y1 y2 h1 h2
    cases y1
    · cases y2
      · rfl
      · have h3 := h1.2 (h2.1 rfl)
        cases h3
    · cases y2
      · have h3 := h2.2 (h1.1 rfl)
        cases h3
      · rfl

/-! ## A document goes into review, or dangles -/

/-- `hash` is what `current` records for `target` now: its first entry for the
target, since a later one for the same target is shadowed by it. -/
def HashedNow (current : List (String × String)) (target hash : String) : Prop :=
  ∃ pre post, current = pre ++ (target, hash) :: post ∧ ∀ p, List.Mem p pre → p.1 ≠ target

/-- Where a document stands. Its target gone, it is dangling. Its target there
and hashing as the document recorded, it is current. Its target there and
hashing to anything else, it is in review, saying what was recorded and what
the target hashes to now.

@specifies REQ-DOCLINK.hash_moves_review
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

end TraceLean.SpecTrace
