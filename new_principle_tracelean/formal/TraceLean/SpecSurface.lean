import TraceLean.Drive
import TraceLean.View
import TraceLean.Context

/-!
# Specifications that pin surface models

A specification is a predicate saying which answers are right, read from a
clause's text rather than from the function that models it. It *pins* the model
when the model meets it and no input has two right answers. Each one here is
stated without calling the model it pins.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.SpecSurface

open TraceLean.Keymap
open TraceLean.Drive
open TraceLean.View
open TraceLean.Context

/-! ## A session -/

/-- The mode the machine is in after `keys`, starting from `mode`: each key
applied to the mode the one before it left. -/
def modeAfter (keymap : Keymap) (mode : String) (keys : List String) : String :=
  keys.foldl (nextMode keymap) mode

/-- What a session reports for the key at index `n` of `keys`: that key, the
mode the machine is in once it is pressed, and the bar offered there. -/
def reportFor (keymap : Keymap) (mode : String) (keys : List String) (n : Nat)
    (key : String) : Step :=
  { key := key,
    mode := modeAfter keymap mode (keys.take (n + 1)),
    menu := whichKey keymap (modeAfter keymap mode (keys.take (n + 1))) }

/-- A run of the editor from a keymap and a starting mode: for every list of
keys, what it produces at each index is that key's report, and there is
something at an index exactly when there is a key there.

@specifies REQ-DRIVE.session_is_a_value -/
def SessionOf (keymap : Keymap) (mode : String) (run : List String → List Step) : Prop :=
  ∀ keys n, (run keys).get? n = (keys.get? n).map (reportFor keymap mode keys n)

theorem drive_reports (keymap : Keymap) (keys : List String) :
    ∀ mode n, (drive keymap mode keys).get? n = (keys.get? n).map (reportFor keymap mode keys n) := by
  induction keys
  case nil =>
    intro mode n
    simp [drive]
  case cons key rest ih =>
    intro mode n
    cases n
    case zero =>
      simp [drive, reportFor, modeAfter]
    case succ n =>
      have later : reportFor keymap mode (key :: rest) (n + 1)
          = reportFor keymap (nextMode keymap mode key) rest n := by
        funext k
        simp [reportFor, modeAfter]
      have h := ih (nextMode keymap mode key) n
      simp at h
      simp [drive, later, h]

/-- `drive` is a run of the editor, and there is one.

@pins REQ-DRIVE.session_is_a_value -/
theorem session_pinned :
    (∀ x1 x2, SessionOf x1 x2 (drive x1 x2)) ∧
    (∀ x1 x2 y1 y2, SessionOf x1 x2 y1 → SessionOf x1 x2 y2 → y1 = y2) := by
  constructor
  · intro keymap mode keys n
    exact drive_reports keymap keys mode n
  · intro keymap mode y1 y2 h1 h2
    funext keys
    apply List.ext_get?
    intro n
    rw [h1 keys n, h2 keys n]

/-! ## Reading a symbolic row -/

/-- What is read off a row is the names of its regions, one after another, and
nothing of what was painted.

@specifies REQ-VIEW.presentation_may_be_symbolic -/
def ReadAsNames (read : List Presented → String) : Prop :=
  ∀ row, read row = String.join (row.map (fun piece => piece.name))

theorem foldl_from (l : List String) :
    ∀ s, l.foldl (fun r t => r ++ t) s = s ++ l.foldl (fun r t => r ++ t) "" := by
  induction l
  case nil =>
    intro s
    simp
  case cons a rest ih =>
    intro s
    rw [List.foldl_cons, List.foldl_cons, ih (s ++ a), ih ("" ++ a)]
    simp [String.append_assoc]

theorem join_cons (first : String) (rest : List String) :
    String.join (first :: rest) = first ++ String.join rest := by
  unfold String.join
  rw [List.foldl_cons, foldl_from rest ("" ++ first), String.empty_append]

/-- `accessible` reads a row by its names, and that reading is the only one.

@pins REQ-VIEW.presentation_may_be_symbolic -/
theorem read_as_names_pinned :
    (ReadAsNames accessible) ∧
    (∀ y1 y2, ReadAsNames y1 → ReadAsNames y2 → y1 = y2) := by
  constructor
  · intro row
    induction row
    case nil => rfl
    case cons piece rest ih =>
      simp [accessible, ih, join_cons]
  · intro y1 y2 h1 h2
    funext row
    rw [h1 row, h2 row]

/-! ## The parts of a context -/

/-- The parts the copied text holds: a part is there exactly when the person
included it and it has something in it, and the parts come in the one fixed
order `allParts` lists them in.

@specifies REQ-CONTEXT.person_chooses -/
def ChosenParts (included : List Part) (filled : List Part) (shown : List Part) : Prop :=
  (∀ p, List.Mem p shown ↔ (List.Mem p included ∧ List.Mem p filled)) ∧
    List.Sublist shown allParts

/-- Two lists drawn in order from a list without repeats, holding the same
things, are the same list. -/
theorem sublists_agree {α : Type} (whole : List α) (nodup : List.Nodup whole) :
    ∀ (l1 l2 : List α), List.Sublist l1 whole → List.Sublist l2 whole →
      (∀ x, List.Mem x l1 ↔ List.Mem x l2) → l1 = l2 := by
  induction whole
  case nil =>
    intro l1 l2 s1 s2 _
    rw [List.sublist_nil.mp s1, List.sublist_nil.mp s2]
  case cons a rest ih =>
    intro l1 l2 s1 s2 same
    have parts := List.nodup_cons.mp nodup
    have inner := ih parts.2
    cases s1
    next t1 =>
      cases s2
      next t2 =>
        exact inner l1 l2 t1 t2 same
      next l2r t2 =>
        exact absurd (t1.subset ((same a).mpr (List.Mem.head l2r))) parts.1
    next l1r t1 =>
      cases s2
      next t2 =>
        exact absurd (t2.subset ((same a).mp (List.Mem.head l1r))) parts.1
      next l2r t2 =>
        have tails : l1r = l2r := by
          apply inner l1r l2r t1 t2
          intro x
          constructor
          · intro m
            have there := (same x).mp (List.Mem.tail a m)
            cases there
            next => exact absurd (t1.subset m) parts.1
            next later => exact later
          · intro m
            have there := (same x).mpr (List.Mem.tail a m)
            cases there
            next => exact absurd (t2.subset m) parts.1
            next later => exact later
        rw [tails]

theorem all_parts_nodup : List.Nodup allParts := by
  decide

theorem mem_iff_contains (q : Part) (l : List Part) : List.Mem q l ↔ l.contains q = true := by
  exact List.elem_iff.symm

theorem every_part_listed (p : Part) : List.Mem p allParts := by
  cases p
  all_goals exact (mem_iff_contains _ allParts).mpr rfl

/-- `partsShown` holds exactly the included, filled parts in the fixed order,
and nothing else does.

@pins REQ-CONTEXT.person_chooses -/
theorem chosen_parts_pinned :
    (∀ x1 x2, ChosenParts x1 x2 (partsShown x1 x2)) ∧
    (∀ x1 x2 y1 y2, ChosenParts x1 x2 y1 → ChosenParts x1 x2 y2 → y1 = y2) := by
  constructor
  · intro included filled
    constructor
    · intro p
      unfold partsShown
      constructor
      · intro h
        have h2 := List.mem_filter.mp h
        have chosen := (Bool.and_eq_true _ _).mp h2.2
        have inIncluded := (mem_iff_contains p included).mpr chosen.1
        have inFilled := (mem_iff_contains p filled).mpr chosen.2
        exact And.intro inIncluded inFilled
      · intro h
        apply List.mem_filter.mpr
        simp only [Bool.and_eq_true]
        have inIncluded := (mem_iff_contains p included).mp h.1
        have inFilled := (mem_iff_contains p filled).mp h.2
        exact And.intro (every_part_listed p) (And.intro inIncluded inFilled)
    · exact List.filter_sublist allParts
  · intro included filled y1 y2 h1 h2
    apply sublists_agree allParts all_parts_nodup y1 y2 h1.2 h2.2
    intro x
    rw [h1.1 x, h2.1 x]

/-- A name picks out the part whose label it is, and only that one: no part
when it is no part's label.

@specifies REQ-CONTEXT.part_named -/
def NamedPart (name : String) (answer : Option Part) : Prop :=
  ∀ p, answer = some p ↔ label p = name

theorem label_injective (a b : Part) : label a = label b → a = b := by
  cases a
  all_goals cases b
  all_goals simp [label]

theorem find_label (name : String) (p : Part) : ∀ (parts : List Part),
    List.find? (fun q => label q == name) parts = some p ↔ (List.Mem p parts ∧ label p = name) := by
  intro parts
  induction parts
  case nil =>
    simp
    intro h
    cases h
  case cons a rest ih =>
    cases Decidable.em (label a = name)
    next here =>
      simp [List.find?, here]
      constructor
      · intro same
        subst same
        exact And.intro (List.Mem.head rest) here
      · intro h
        exact label_injective a p (here.trans h.2.symm)
    next here =>
      have differs : (label a == name) = false := by
        simp [here]
      simp only [List.find?, differs]
      rw [ih]
      constructor
      · intro h
        exact And.intro (List.Mem.tail a h.1) h.2
      · intro h
        have m := h.1
        have named := h.2
        cases m
        next => exact absurd named here
        next later => exact And.intro later named

/-- `partNamed` answers with the part a label names, and nothing else does.

@pins REQ-CONTEXT.part_named -/
theorem part_named_pinned :
    (∀ x1, NamedPart x1 (partNamed x1)) ∧
    (∀ x1 y1 y2, NamedPart x1 y1 → NamedPart x1 y2 → y1 = y2) := by
  constructor
  · intro name p
    unfold partNamed
    rw [find_label name p allParts]
    constructor
    · intro h
      exact h.2
    · intro h
      exact And.intro (every_part_listed p) h
  · intro name y1 y2 h1 h2
    cases y1
    case none =>
      cases y2
      case none => rfl
      case some q =>
        have named := (h2 q).mp rfl
        have absurd := (h1 q).mpr named
        cases absurd
    case some p =>
      have named := (h1 p).mp rfl
      exact ((h2 p).mpr named).symm

end TraceLean.SpecSurface
