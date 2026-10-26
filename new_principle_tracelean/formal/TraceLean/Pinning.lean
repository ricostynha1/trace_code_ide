import TraceLean.Strength

namespace TraceLean.Pinning

open TraceLean.Strength

open Lean (ToJson FromJson)

structure PinRecord where
  theoremName : String
  key : String
  pinned : Bool
  said : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def argsOf (n : Nat) : String :=
  String.join ((List.range n).map (fun i => " x" ++ toString (i + 1)))

/-- @models REQ-STRENGTH.per_input -/
def statement (spec model : String) (inputs : Nat) : String :=
  let args := argsOf inputs
  let meets :=
    if inputs == 0 then spec ++ " " ++ model
    else "∀" ++ args ++ ", " ++ spec ++ args ++ " (" ++ model ++ args ++ ")"
  "(" ++ meets ++ ") ∧ (∀" ++ args ++ " y1 y2, " ++ spec ++ args ++ " y1 → " ++ spec ++ args
    ++ " y2 → y1 = y2)"

structure Scan where
  depth : Nat
  names : Nat
  group : List Char
  explicit : Bool
  typed : Bool
  arrows : Nat
  result : List Char
  deriving Repr, Inhabited

def isOpener (c : Char) : Bool := c == '(' || c == '{' || c == '['

def isCloser (c : Char) : Bool := c == ')' || c == '}' || c == ']'

def isBlank (c : Char) : Bool := c == ' ' || c == '\t' || c == '\n' || c == '\r'

def wordCount (cs : List Char) : Nat :=
  (cs.foldl (fun acc c =>
    if isBlank c then (acc.1, false) else if acc.2 then acc else (acc.1 + 1, true)) (0, false)).1

def bindersIn (group : List Char) : Nat :=
  if group.contains ':' then wordCount (group.takeWhile (· != ':')) else 0

def stepBinders (s : Scan) (c : Char) : Scan :=
  if isOpener c then
    if s.depth == 0 then { s with depth := 1, group := [], explicit := c == '(' }
    else { s with depth := s.depth + 1, group := s.group ++ [c] }
  else if isCloser c then
    if s.depth - 1 > 0 then { s with depth := s.depth - 1, group := s.group ++ [c] }
    else if s.explicit then { s with depth := 0, names := s.names + bindersIn s.group }
    else { s with depth := 0 }
  else if c == ':' && s.depth == 0 then { s with typed := true }
  else if s.depth ≥ 1 then { s with group := s.group ++ [c] }
  else s

def stepType (s : Scan) (c : Char) : Option Scan :=
  if isOpener c then some { s with depth := s.depth + 1, result := s.result ++ [c] }
  else if isCloser c then some { s with depth := s.depth - 1, result := s.result ++ [c] }
  else if s.depth == 0 && c == '|' then none
  else if s.depth == 0 && c == '→' then some { s with arrows := s.arrows + 1, result := [] }
  else if s.depth == 0 && c == '>' && s.result.getLast? == some '-' then
    some { s with arrows := s.arrows + 1, result := [] }
  else some { s with result := s.result ++ [c] }

def scan : Scan → List Char → Scan
  | s, [] => s
  | s, c :: rest =>
    if s.typed then
      match stepType s c with
      | none => s
      | some t => scan t rest
    else scan (stepBinders s c) rest

def suffixFrom (pat : List Char) : List Char → Option (List Char)
  | [] => if pat.isEmpty then some [] else none
  | c :: rest => if pat.isPrefixOf (c :: rest) then some (c :: rest) else suffixFrom pat rest

def before (pat : List Char) : List Char → List Char
  | [] => []
  | c :: rest => if pat.isPrefixOf (c :: rest) then [] else c :: before pat rest

def fresh : Scan :=
  { depth := 0, names := 0, group := [], explicit := false, typed := false, arrows := 0,
    result := [] }

/-- @models REQ-STRENGTH.inputs_counted -/
def shape (declaration : String) : Option (Bool × Nat) :=
  let cs := declaration.toList
  let start := (suffixFrom "def ".toList cs).orElse (fun _ => suffixFrom "abbrev ".toList cs)
  start.map (fun rest =>
    let s := scan fresh (before ":=".toList rest)
    (s.typed && (String.mk s.result).trim == "Prop", s.names + s.arrows))

def occursIn (pat text : List Char) : Bool :=
  match text with
  | [] => pat.isEmpty
  | c :: rest => pat.isPrefixOf (c :: rest) || occursIn pat rest

def holds (text pat : String) : Bool := occursIn pat.toList text.toList

def clears (named line : String) : Bool :=
  named.toList.isPrefixOf line.toList
    && (holds line "does not depend on any axioms"
      || (holds line "depends on axioms" && !(holds line "sorryAx")))

/-- @models REQ-STRENGTH.kernel_decides -/
def accepted (theoremName output : String) (exitedOk : Bool) : Bool :=
  exitedOk && !(holds output ": error")
    && (output.splitOn "\n").any (clears ("'" ++ theoremName ++ "'"))

/-- @models REQ-STRENGTH.verdict_kept -/
def standing (theoremName : Option String) (record : Option PinRecord) (key : String) : Strength :=
  match theoremName, record with
  | none, _ => Strength.«open»
  | some t, some r =>
    if r.pinned && r.theoremName == t && r.key == key then Strength.pinned t
    else Strength.attempted t
  | some t, none => Strength.attempted t

/-- @proves REQ-STRENGTH.open_is_the_default -/
theorem no_theorem_is_open (record : Option PinRecord) (key : String) :
    standing none record key = Strength.«open» := by
  cases record
  all_goals rfl

/-- @proves REQ-STRENGTH.verdict_kept -/
theorem another_key_does_not_pin (t key : String) (r : PinRecord) (h : r.key ≠ key) :
    standing (some t) (some r) key = Strength.attempted t := by
  simp [standing, h]

/-- @proves REQ-STRENGTH.kernel_decides -/
theorem a_failed_run_is_not_accepted (t output : String) : accepted t output false = false := by
  simp [accepted]

theorem ite_ne {p : Prop} [Decidable p] {a b x : Char} (ha : a ≠ x) (hb : b ≠ x) :
    (if p then a else b) ≠ x := by
  by_cases h : p <;> simp [h, ha, hb]

theorem digitChar_not_space (n : Nat) : Nat.digitChar n ≠ ' ' := by
  unfold Nat.digitChar
  repeat' apply ite_ne
  all_goals simp

theorem toDigitsCore_no_space (base : Nat) : ∀ (fuel n : Nat) (ds : List Char), ' ' ∉ ds →
    ' ' ∉ Nat.toDigitsCore base fuel n ds := by
  intro fuel
  induction fuel with
  | zero => intro _ ds h; exact h
  | succ fuel ih =>
    intro n ds h
    unfold Nat.toDigitsCore
    simp only
    have hd : ' ' ∉ Nat.digitChar (n % base) :: ds := by
      intro hm
      rcases List.mem_cons.mp hm with hm | hm
      · exact digitChar_not_space _ hm.symm
      · exact h hm
    split
    · exact hd
    · exact ih _ _ hd

theorem nat_string_no_space (n : Nat) : (toString n).toList.count ' ' = 0 := by
  apply List.count_eq_zero.mpr
  show ' ' ∉ (Nat.toDigits 10 n)
  exact toDigitsCore_no_space 10 (n + 1) n [] (by simp)

theorem join_snoc (l : List String) (x : String) : String.join (l ++ [x]) = String.join l ++ x := by
  unfold String.join
  rw [List.foldl_append]
  rfl

theorem argsOf_succ (n : Nat) : argsOf (n + 1) = argsOf n ++ (" x" ++ toString (n + 1)) := by
  unfold argsOf
  rw [List.range_succ, List.map_append, List.map_singleton, join_snoc]

theorem argsOf_binders (n : Nat) : (argsOf n).toList.count ' ' = n := by
  induction n with
  | zero => rfl
  | succ n ih =>
    rw [argsOf_succ]
    show (argsOf n ++ (" x" ++ toString (n + 1))).data.count ' ' = n + 1
    rw [String.data_append, String.data_append, List.count_append, List.count_append]
    have h := nat_string_no_space (n + 1)
    simp only [String.toList] at ih h
    rw [ih, h]
    rfl

/-- @proves REQ-STRENGTH.per_input -/
theorem the_obligation_is_meets_and_unique_over_one_binder_per_input (spec model : String)
    (n : Nat) :
    (argsOf n).toList.count ' ' = n ∧
    statement spec model (n + 1) =
      "(" ++ "∀" ++ argsOf (n + 1) ++ ", " ++ spec ++ argsOf (n + 1) ++ " (" ++ model
        ++ argsOf (n + 1) ++ ")" ++ ") ∧ (∀" ++ argsOf (n + 1) ++ " y1 y2, " ++ spec ++ argsOf (n + 1) ++ " y1 → " ++ spec
        ++ argsOf (n + 1) ++ " y2 → y1 = y2)" ∧
    statement spec model 0 =
      "(" ++ (spec ++ " " ++ model) ++ ") ∧ (∀" ++ argsOf 0 ++ " y1 y2, " ++ spec ++ argsOf 0
        ++ " y1 → " ++ spec ++ argsOf 0 ++ " y2 → y1 = y2)" ∧ argsOf 0 = "" := by
  refine ⟨argsOf_binders n, ?_, ?_, rfl⟩
  · unfold statement
    simp only [show (n + 1 == 0) = false from rfl, Bool.false_eq_true, if_false,
      String.append_assoc]
  · unfold statement
    simp only [show (0 == 0) = true from rfl, if_true, String.append_assoc]

def binderText (gs : List (Bool × List Char)) : List Char :=
  gs.bind (fun g => ' ' :: (if g.1 then '(' else '{') :: (g.2 ++ [if g.1 then ')' else '}']))

def explicitCount (gs : List (Bool × List Char)) : Nat :=
  gs.foldr (fun g acc => (if g.1 then bindersIn g.2 else 0) + acc) 0

theorem scan_untyped (s : Scan) (c : Char) (rest : List Char) (ht : s.typed = false) :
    scan s (c :: rest) = scan (stepBinders s c) rest := by
  rw [scan.eq_2, ht]
  rfl

theorem scan_typed (s t : Scan) (c : Char) (rest : List Char) (ht : s.typed = true)
    (h : stepType s c = some t) : scan s (c :: rest) = scan t rest := by
  rw [scan.eq_2, ht]
  simp [h]

def plainChar (c : Char) : Prop := isOpener c = false ∧ isCloser c = false ∧ c ≠ ':' ∧ c ≠ '='

theorem scan_plain (s : Scan) (hd : s.depth = 0) (ht : s.typed = false) :
    ∀ (cs rest : List Char), (∀ c ∈ cs, plainChar c) → scan s (cs ++ rest) = scan s rest := by
  intro cs
  induction cs with
  | nil => intro rest _; rfl
  | cons c cs ih =>
    intro rest h
    obtain ⟨h1, h2, h3, _⟩ := h c (List.mem_cons_self _ _)
    have hs : stepBinders s c = s := by
      simp [stepBinders, h1, h2, h3, hd]
    rw [List.cons_append, scan_untyped s c _ ht, hs]
    exact ih rest (fun c' hc => h c' (List.mem_cons_of_mem _ hc))

theorem scan_group (s : Scan) (hd : s.depth = 1) (ht : s.typed = false) :
    ∀ (g rest : List Char), (∀ c ∈ g, isOpener c = false ∧ isCloser c = false) →
      scan s (g ++ rest) = scan { s with group := s.group ++ g } rest := by
  intro g
  induction g generalizing s with
  | nil => intro rest _; simp
  | cons c g ih =>
    intro rest h
    obtain ⟨h1, h2⟩ := h c (List.mem_cons_self _ _)
    have hs : stepBinders s c = { s with group := s.group ++ [c] } := by
      simp [stepBinders, h1, h2, hd]
    rw [List.cons_append, scan_untyped s c _ ht, hs]
    rw [ih { s with group := s.group ++ [c] } hd ht rest
      (fun c' hc => h c' (List.mem_cons_of_mem _ hc))]
    simp

def afterGroup (s : Scan) (e : Bool) (cs : List Char) : Scan :=
  { s with depth := 0, group := cs, explicit := e, names := s.names + (if e then bindersIn cs else 0) }

theorem scan_one_group (s : Scan) (e : Bool) (cs rest : List Char) (hd : s.depth = 0)
    (ht : s.typed = false) (hcs : ∀ c ∈ cs, isOpener c = false ∧ isCloser c = false) :
    scan s (' ' :: (if e then '(' else '{') :: (cs ++ (if e then ')' else '}') :: rest)) =
      scan (afterGroup s e cs) rest := by
  have hsp : stepBinders s ' ' = s := by simp [stepBinders, isOpener, isCloser, hd]
  rw [scan_untyped s _ _ ht, hsp, scan_untyped s _ _ ht]
  have hopen : stepBinders s (if e then '(' else '{') =
      { s with depth := 1, group := [], explicit := e } := by
    cases e <;> simp [stepBinders, isOpener, hd]
  rw [hopen, scan_group { s with depth := 1, group := [], explicit := e } rfl ht cs _ hcs]
  cases e <;> simp [scan, stepBinders, isOpener, isCloser, afterGroup, ht]

theorem scan_binders : ∀ (gs : List (Bool × List Char)) (s : Scan) (rest : List Char),
    s.depth = 0 → s.typed = false →
    (∀ g ∈ gs, ∀ c ∈ g.2, isOpener c = false ∧ isCloser c = false) →
    ∃ t : Scan, t.depth = 0 ∧ t.typed = false ∧ t.names = s.names + explicitCount gs ∧
      t.arrows = s.arrows ∧ scan s (binderText gs ++ rest) = scan t rest := by
  intro gs
  induction gs with
  | nil => intro s rest hd ht _; exact ⟨s, hd, ht, by simp [explicitCount], rfl, rfl⟩
  | cons g gs ih =>
    intro s rest hd ht hg
    obtain ⟨e, cs⟩ := g
    have hcs := hg (e, cs) (List.mem_cons_self _ _)
    obtain ⟨t, h1, h2, h3, h4, h5⟩ := ih (afterGroup s e cs) rest rfl ht
      (fun g' hg' => hg g' (List.mem_cons_of_mem _ hg'))
    refine ⟨t, h1, h2, ?_, h4, ?_⟩
    · rw [h3]
      simp [explicitCount, afterGroup, Nat.add_assoc]
    · have hsplit : binderText ((e, cs) :: gs) ++ rest =
          ' ' :: (if e then '(' else '{') :: (cs ++ (if e then ')' else '}') ::
            (binderText gs ++ rest)) := by
        simp [binderText]
      rw [hsplit, scan_one_group s e cs _ hd ht hcs]
      exact h5

theorem scan_type : ∀ (t : List Char) (s : Scan), s.depth = 0 → s.typed = true →
    (∀ c ∈ t, isOpener c = false ∧ isCloser c = false ∧ c ≠ '|' ∧ c ≠ '>') →
    (scan s t).names = s.names ∧ (scan s t).arrows = s.arrows + t.count '→' := by
  intro t
  induction t with
  | nil => intro s _ _ _; simp [scan]
  | cons c t ih =>
    intro s hd ht h
    obtain ⟨h1, h2, h3, h4⟩ := h c (List.mem_cons_self _ _)
    have ht' : ∀ c ∈ t, isOpener c = false ∧ isCloser c = false ∧ c ≠ '|' ∧ c ≠ '>' :=
      fun c' hc => h c' (List.mem_cons_of_mem _ hc)
    by_cases ha : c = '→'
    · subst ha
      have hs : stepType s '→' = some { s with arrows := s.arrows + 1, result := [] } := by
        simp [stepType, isOpener, isCloser, hd]
      rw [scan_typed _ _ _ _ ht hs]
      obtain ⟨e1, e2⟩ := ih { s with arrows := s.arrows + 1, result := [] } hd ht ht'
      refine ⟨e1, ?_⟩
      rw [e2, List.count_cons_self]
      simp only
      omega
    · have hs : stepType s c = some { s with result := s.result ++ [c] } := by
        simp [stepType, h1, h2, hd, h3, ha, h4]
      rw [scan_typed _ _ _ _ ht hs]
      obtain ⟨e1, e2⟩ := ih { s with result := s.result ++ [c] } hd ht ht'
      refine ⟨e1, ?_⟩
      rw [e2, List.count_cons_of_ne (Ne.symm ha)]

theorem before_whole : ∀ (cs : List Char), '=' ∉ cs → before ":=".toList cs = cs := by
  intro cs
  induction cs with
  | nil => intro _; rfl
  | cons c cs ih =>
    intro h
    have hp : List.isPrefixOf [':', '='] (c :: cs) = false := by
      cases cs with
      | nil => simp [List.isPrefixOf]
      | cons d ds =>
        have hd : d ≠ '=' := fun e => h (by simp [e])
        simp [List.isPrefixOf, hd.symm]
    show (if List.isPrefixOf [':', '='] (c :: cs) = true then [] else c :: before ":=".toList cs) = _
    rw [hp]
    simp only [Bool.false_eq_true, if_false]
    rw [ih (fun hm => h (List.mem_cons_of_mem _ hm))]

/-- @proves REQ-STRENGTH.inputs_counted -/
theorem every_explicit_binder_and_arrow_is_an_input (name : List Char)
    (gs : List (Bool × List Char)) (ty : List Char)
    (hn : ∀ c ∈ name, plainChar c)
    (hg : ∀ g ∈ gs, ∀ c ∈ g.2, isOpener c = false ∧ isCloser c = false ∧ c ≠ '=')
    (ht : ∀ c ∈ ty, isOpener c = false ∧ isCloser c = false ∧ c ≠ '|' ∧ c ≠ '>' ∧ c ≠ '=') :
    (shape (String.mk ("def ".toList ++ name ++ binderText gs ++ ':' :: ty))).map (·.2) =
      some (explicitCount gs + ty.count '→') := by
  have hdef : "def ".toList = ['d', 'e', 'f', ' '] := rfl
  have hne : '=' ∉ "def ".toList ++ name ++ binderText gs ++ ':' :: ty := by
    intro hm
    simp only [List.mem_append, List.mem_cons] at hm
    rcases hm with ((hm | hm) | hm) | hm | hm
    · rw [hdef] at hm; simp at hm
    · exact (hn _ hm).2.2.2 rfl
    · simp only [binderText, List.mem_bind] at hm
      obtain ⟨⟨e, cs⟩, hgm, hm⟩ := hm
      simp only [List.mem_cons, List.mem_append, List.mem_singleton] at hm
      rcases hm with hm | hm | hm | hm
      · simp at hm
      · cases e <;> simp at hm
      · exact (hg _ hgm _ hm).2.2 rfl
      · cases e <;> simp at hm
    · simp at hm
    · exact (ht _ hm).2.2.2.2 rfl
  have hsuf : suffixFrom "def ".toList ("def ".toList ++ name ++ binderText gs ++ ':' :: ty) =
      some ("def ".toList ++ name ++ binderText gs ++ ':' :: ty) := by
    rw [hdef]
    simp [suffixFrom, List.isPrefixOf]
  unfold shape
  show Option.map _ (Option.map _ ((suffixFrom "def ".toList
    ("def ".toList ++ name ++ binderText gs ++ ':' :: ty)).orElse _)) = _
  rw [hsuf]
  simp only [Option.orElse, Option.map_some', before_whole _ hne]
  have hplain : ∀ c ∈ "def ".toList ++ name, plainChar c := by
    intro c hc
    rcases List.mem_append.mp hc with hc | hc
    · rw [hdef] at hc
      simp at hc
      rcases hc with hc | hc | hc | hc <;> subst hc <;>
        simp [plainChar, isOpener, isCloser]
    · exact hn c hc
  have hp := scan_plain fresh rfl rfl _ (binderText gs ++ ':' :: ty) hplain
  simp only [List.append_assoc] at hp ⊢
  rw [hp]
  obtain ⟨s, hd, hty, hnames, harrows, he⟩ := scan_binders gs fresh (':' :: ty) rfl rfl
    (fun g hg' c hc => ⟨(hg g hg' c hc).1, (hg g hg' c hc).2.1⟩)
  rw [he]
  have hcolon : stepBinders s ':' = { s with typed := true } := by
    simp [stepBinders, isOpener, isCloser, hd]
  rw [scan_untyped s ':' ty hty, hcolon]
  obtain ⟨e1, e2⟩ := scan_type ty { s with typed := true } hd rfl
    (fun c hc => ⟨(ht c hc).1, (ht c hc).2.1, (ht c hc).2.2.1, (ht c hc).2.2.2.1⟩)
  rw [e1, e2]
  simp only at hnames harrows ⊢
  rw [hnames, harrows]
  simp [fresh]

end TraceLean.Pinning
