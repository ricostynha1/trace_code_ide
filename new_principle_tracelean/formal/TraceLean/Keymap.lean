import Lean

namespace TraceLean.Keymap

open Lean (ToJson FromJson)

inductive Outcome where
  | enter (mode : String)
  | dispatch (action : String)
  | leave (mode : String)
  | passThrough
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

inductive Binding where
  | enter (mode : String) (description : String)
  | dispatch (action : String) (description : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Mode where
  parent : Option String := none
  bindings : List (String × Binding) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Keymap where
  root : String
  modes : List (String × Mode) := []
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def leaveKey : String := "Escape"

def Keymap.mode (k : Keymap) (name : String) : Option Mode :=
  (k.modes.find? (·.1 == name)).map (·.2)

def Mode.binding (m : Mode) (key : String) : Option Binding :=
  (m.bindings.find? (·.1 == key)).map (·.2)

/-- @models REQ-MYTH.totality
@models REQ-MYTH.escape_pops_one
@models REQ-MYTH.outcomes_closed -/
def step (keymap : Keymap) (mode : String) (key : String) : Outcome :=
  match keymap.mode mode with
  | none => .passThrough
  | some current =>
    match current.binding key with
    | some (.enter target _) => .enter target
    | some (.dispatch action _) => .dispatch action
    | none =>
      if key == leaveKey then
        match current.parent with
        | some parent => .leave parent
        | none => .passThrough
      else
        match current.parent with
        | none => .passThrough
        | some _ => .leave keymap.root

def nextMode (keymap : Keymap) (mode : String) (key : String) : String :=
  match step keymap mode key with
  | .enter target => target
  | .leave target => target
  | .dispatch _ => keymap.root
  | .passThrough => mode

def Binding.description : Binding → String
  | .enter _ d => d
  | .dispatch _ d => d

/-- @models REQ-MYTH.whichkey_is_a_query -/
def whichKey (keymap : Keymap) (mode : String) : List (String × String) :=
  match keymap.mode mode with
  | none => []
  | some current =>
    let bound := current.bindings.map (fun b => (b.1, b.2.description))
    bound ++ (if current.parent.isSome then [(leaveKey, "leave this mode")] else [])

inductive Problem where
  | undefinedMode (mode key target : String)
  | undefinedAction (mode key action : String)
  | parentCycle (mode : String)
  | unreachableMode (mode : String)
  | unreachableAction (action : String)
  | noRoot (root : String)
  | undefinedParent (mode parent : String)
  | stranded (mode : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def Problem.key : Problem → Nat × List String
  | .undefinedMode m k t => (0, [m, k, t])
  | .undefinedAction m k a => (1, [m, k, a])
  | .parentCycle m => (2, [m])
  | .unreachableMode m => (3, [m])
  | .unreachableAction a => (4, [a])
  | .noRoot r => (5, [r])
  | .undefinedParent m p => (6, [m, p])
  | .stranded m => (7, [m])

def listLe : List String → List String → Bool
  | [], _ => true
  | _ :: _, [] => false
  | a :: as, b :: bs => if a != b then decide (a ≤ b) else listLe as bs

def problemLe (a b : Problem) : Bool :=
  let (ia, fa) := a.key
  let (ib, fb) := b.key
  if ia != ib then decide (ia ≤ ib) else listLe fa fb

def Keymap.hasMode (k : Keymap) (name : String) : Bool := (k.mode name).isSome

inductive Leaving where
  | reachesRoot
  | loops
  | stops
  deriving Repr, DecidableEq

def leaveFrom (keymap : Keymap) : Nat → List String → String → Leaving
  | 0, _, _ => .loops
  | fuel + 1, seen, mode =>
    match step keymap mode leaveKey with
    | .dispatch _ => .reachesRoot
    | .passThrough => if mode == keymap.root then .reachesRoot else .stops
    | .enter target =>
      if seen.contains target then .loops else leaveFrom keymap fuel (seen ++ [target]) target
    | .leave target =>
      if seen.contains target then .loops else leaveFrom keymap fuel (seen ++ [target]) target

def enterTargets (m : Mode) : List String :=
  m.bindings.filterMap (fun b => match b.2 with | .enter target _ => some target | _ => none)

def reachableFrom (keymap : Keymap) : Nat → List String → List String → List String
  | 0, seen, _ => seen
  | _ + 1, seen, [] => seen
  | fuel + 1, seen, name :: frontier =>
    match keymap.mode name with
    | none => reachableFrom keymap fuel seen frontier
    | some m =>
      let fresh := (enterTargets m).filter (fun t => !seen.contains t && keymap.hasMode t)
      let queue := fresh.foldl (fun acc t => if acc.contains t then acc else acc ++ [t]) []
      reachableFrom keymap fuel (seen ++ queue) (frontier ++ queue)

private def bindingProblem (keymap : Keymap) (actions : List String) (name : String)
    (acc : List Problem) (b : String × Binding) : List Problem :=
  match b.2 with
  | .enter target _ =>
    if keymap.hasMode target then acc else acc ++ [Problem.undefinedMode name b.1 target]
  | .dispatch action _ =>
    if actions.contains action then acc
    else acc ++ [Problem.undefinedAction name b.1 action]

private def parentProblem (keymap : Keymap) (name : String) : Option String → List Problem
  | none => []
  | some parent => if keymap.hasMode parent then [] else [Problem.undefinedParent name parent]

private def leavingProblem (name : String) : Leaving → List Problem
  | .reachesRoot => []
  | .loops => [Problem.parentCycle name]
  | .stops => [Problem.stranded name]

private def modeProblems (keymap : Keymap) (actions : List String) (acc : List Problem)
    (entry : String × Mode) : List Problem :=
  let name := entry.1
  let m := entry.2
  let bindingProblems := m.bindings.foldl (bindingProblem keymap actions name) acc
  let parentProblems := parentProblem keymap name m.parent
  let budget := keymap.modes.length + 2
  bindingProblems ++ parentProblems ++ leavingProblem name (leaveFrom keymap budget [name] name)

private def dispatchableIn (keymap : Keymap) (acc : List String) (name : String) : List String :=
  match keymap.mode name with
  | none => acc
  | some m =>
    m.bindings.foldl
      (fun seen b =>
        match b.2 with
        | .dispatch action _ => if seen.contains action then seen else seen ++ [action]
        | _ => seen)
      acc

/-- @models REQ-MYTH.modes_defined
@models REQ-MYTH.actions_defined
@models REQ-MYTH.escape_terminates
@models REQ-MYTH.actions_reachable
@models REQ-MYTH.keymap_is_data -/
def validate (keymap : Keymap) (actions : List String) : List Problem :=
  let actions := (actions.foldl (fun acc a => if acc.contains a then acc else acc ++ [a])
    []).mergeSort (fun a b => decide (a ≤ b))
  let noRoot := if keymap.hasMode keymap.root then [] else [Problem.noRoot keymap.root]
  let perMode := keymap.modes.foldl (modeProblems keymap actions) []
  let reachable := reachableFrom keymap (keymap.modes.length + 1) [keymap.root] [keymap.root]
  let unreachableModes := (keymap.modes.map (·.1)).filterMap
    (fun name => if reachable.contains name then none else some (Problem.unreachableMode name))
  let dispatchable := reachable.foldl (dispatchableIn keymap) []
  let unreachableActions := actions.filterMap
    (fun a => if dispatchable.contains a then none else some (Problem.unreachableAction a))
  (noRoot ++ perMode ++ unreachableModes ++ unreachableActions).mergeSort problemLe

def nested : Keymap :=
  { root := "Normal", modes := [
      ("Normal", { bindings := [("Space", .enter "Leader" "leader")] }),
      ("Leader", { parent := some "Normal", bindings := [("g", .enter "Design" "design")] }),
      ("Design", { parent := some "Leader", bindings := [("o", .dispatch "design.toggle" "fold")] })] }

/-- @proves REQ-MYTH.outcomes_closed -/
theorem every_outcome_is_one_of_four (keymap : Keymap) (mode key : String) :
    (∃ t, step keymap mode key = .enter t) ∨ (∃ a, step keymap mode key = .dispatch a) ∨
    (∃ t, step keymap mode key = .leave t) ∨ step keymap mode key = .passThrough := by
  cases step keymap mode key <;> simp

theorem mode_mem {k : Keymap} {name : String} {m : Mode} (h : k.mode name = some m) :
    (name, m) ∈ k.modes := by
  unfold Keymap.mode at h
  cases hf : k.modes.find? (·.1 == name) with
  | none => simp [hf] at h
  | some e =>
    simp [hf] at h
    have h1 := List.find?_some hf
    have h2 := List.mem_of_find?_eq_some hf
    simp at h1
    obtain ⟨n, m'⟩ := e
    simp at h h1
    subst h h1
    exact h2

theorem mode_of_mem {k : Keymap} {name : String} (h : k.hasMode name = true) :
    ∃ m, k.mode name = some m := by
  unfold Keymap.hasMode at h
  cases hm : k.mode name with
  | none => simp [hm] at h
  | some m => exact ⟨m, rfl⟩

theorem binding_mem {m : Mode} {key : String} {b : Binding} (h : m.binding key = some b) :
    (key, b) ∈ m.bindings := by
  unfold Mode.binding at h
  cases hf : m.bindings.find? (·.1 == key) with
  | none => simp [hf] at h
  | some e =>
    simp [hf] at h
    have h1 := List.find?_some hf
    have h2 := List.mem_of_find?_eq_some hf
    obtain ⟨n, b'⟩ := e
    simp at h h1
    subst h h1
    exact h2

theorem binding_some_of_mem {m : Mode} {key : String} {b : Binding} (h : (key, b) ∈ m.bindings) :
    ∃ b', m.binding key = some b' := by
  unfold Mode.binding
  have : (m.bindings.find? (·.1 == key)).isSome = true :=
    (List.find?_isSome _ _).mpr ⟨(key, b), h, by simp⟩
  cases hf : m.bindings.find? (·.1 == key) with
  | none => simp [hf] at this
  | some e => exact ⟨e.2, rfl⟩

def acts (as : List String) : List String :=
  (as.foldl (fun acc a => if acc.contains a then acc else acc ++ [a]) []).mergeSort
    (fun a b => decide (a ≤ b))

def reachedModes (k : Keymap) : List String :=
  reachableFrom k (k.modes.length + 1) [k.root] [k.root]

theorem validate_eq (k : Keymap) (as : List String) : validate k as =
    ((if k.hasMode k.root then [] else [Problem.noRoot k.root])
      ++ k.modes.foldl (modeProblems k (acts as)) []
      ++ (k.modes.map (·.1)).filterMap
          (fun name => if (reachedModes k).contains name then none
            else some (Problem.unreachableMode name))
      ++ (acts as).filterMap
          (fun a => if ((reachedModes k).foldl (dispatchableIn k) []).contains a then none
            else some (Problem.unreachableAction a))).mergeSort problemLe := rfl

theorem dedup_mem {α : Type} [BEq α] [LawfulBEq α] (l acc : List α) (x : α) :
    x ∈ l.foldl (fun acc a => if acc.contains a then acc else acc ++ [a]) acc ↔
      x ∈ acc ∨ x ∈ l := by
  induction l generalizing acc with
  | nil => simp
  | cons a l ih =>
    simp only [List.foldl_cons]
    rw [ih]
    by_cases h : a ∈ acc
    · have hc : acc.contains a = true := by simp [h]
      simp only [hc, if_true, List.mem_cons]
      constructor
      · intro h1
        cases h1 with
        | inl h1 => exact Or.inl h1
        | inr h1 => exact Or.inr (Or.inr h1)
      · intro h1
        cases h1 with
        | inl h1 => exact Or.inl h1
        | inr h1 =>
          cases h1 with
          | inl h1 => subst h1; exact Or.inl h
          | inr h1 => exact Or.inr h1
    · have hc : acc.contains a = false := by simp [h]
      simp only [hc, List.mem_cons, List.mem_append, List.mem_singleton]
      simp [or_assoc]

theorem mem_acts (as : List String) (x : String) : x ∈ acts as ↔ x ∈ as := by
  unfold acts
  rw [List.mem_mergeSort, dedup_mem]
  simp

theorem mergeSort_nil {l : List Problem} (h : l.mergeSort problemLe = []) : ∀ x, x ∉ l := by
  intro x hx
  have := (List.mem_mergeSort (le := problemLe)).mpr hx
  rw [h] at this
  simp at this

theorem valid_parts {k : Keymap} {as : List String} (h : validate k as = []) :
    k.hasMode k.root = true ∧ (∀ x, x ∉ k.modes.foldl (modeProblems k (acts as)) []) ∧
    (∀ a ∈ acts as, ((reachedModes k).foldl (dispatchableIn k) []).contains a = true) := by
  rw [validate_eq] at h
  have hn := mergeSort_nil h
  simp only [List.mem_append, not_or] at hn
  refine ⟨?_, fun x hx => (hn x).1.1.2 hx, ?_⟩
  · cases hr : k.hasMode k.root with
    | true => rfl
    | false =>
      exfalso
      apply (hn (Problem.noRoot k.root)).1.1.1
      simp [hr]
  · intro a ha
    cases hc : ((reachedModes k).foldl (dispatchableIn k) []).contains a with
    | true => rfl
    | false =>
      exfalso
      apply (hn (Problem.unreachableAction a)).2
      rw [List.mem_filterMap]
      exact ⟨a, ha, by simp only [hc]; rfl⟩

theorem foldl_keeps {α β : Type} {f : List β → α → List β}
    (hf : ∀ acc a x, x ∈ acc → x ∈ f acc a) :
    ∀ (l : List α) (init : List β) (x : β), x ∈ init → x ∈ l.foldl f init := by
  intro l
  induction l with
  | nil => intro init x h; exact h
  | cons a _ ih => intro init x h; exact ih _ _ (hf _ _ _ h)

theorem foldl_adds {α β : Type} {f : List β → α → List β}
    (hf : ∀ acc a x, x ∈ acc → x ∈ f acc a)
    {l : List α} {a : α} {x : β} (ha : a ∈ l) (hx : ∀ acc, x ∈ f acc a) (init : List β) :
    x ∈ l.foldl f init := by
  induction l generalizing init with
  | nil => cases ha
  | cons b l ih =>
    simp only [List.foldl_cons]
    cases ha with
    | head => exact foldl_keeps hf l _ x (hx init)
    | tail _ h => exact ih h _

theorem bindingProblem_keeps (k : Keymap) (as : List String) (name : String) :
    ∀ acc b x, x ∈ acc → x ∈ bindingProblem k as name acc b := by
  intro acc b x h
  unfold bindingProblem
  split <;> split <;> simp [h]

theorem modeProblems_keeps (k : Keymap) (as : List String) :
    ∀ acc e x, x ∈ acc → x ∈ modeProblems k as acc e := by
  intro acc e x h
  unfold modeProblems
  simp only [List.mem_append]
  exact Or.inl (Or.inl (foldl_keeps (bindingProblem_keeps k as e.1) _ _ _ h))

theorem valid_mode {k : Keymap} {as : List String} (h : validate k as = []) {name : String}
    {m : Mode} (hm : (name, m) ∈ k.modes) :
    (∀ key t d, (key, Binding.enter t d) ∈ m.bindings → k.hasMode t = true) ∧
    (∀ key a d, (key, Binding.dispatch a d) ∈ m.bindings → a ∈ as) ∧
    (∀ p, m.parent = some p → k.hasMode p = true) ∧
    leaveFrom k (k.modes.length + 2) [name] name = .reachesRoot := by
  obtain ⟨_, hper, _⟩ := valid_parts h
  have inMode : ∀ x, (∀ acc, x ∈ modeProblems k (acts as) acc (name, m)) → False :=
    fun x hx => hper x (foldl_adds (modeProblems_keeps k (acts as)) hm hx [])
  refine ⟨?_, ?_, ?_, ?_⟩
  · intro key t d hb
    cases ht : k.hasMode t with
    | true => rfl
    | false =>
      exfalso
      apply inMode (Problem.undefinedMode name key t)
      intro acc
      unfold modeProblems
      simp only [List.mem_append]
      refine Or.inl (Or.inl (foldl_adds (bindingProblem_keeps k (acts as) name) hb ?_ acc))
      intro acc'
      simp [bindingProblem, ht]
  · intro key a d hb
    cases ha : (acts as).contains a with
    | true => simpa [mem_acts] using ha
    | false =>
      exfalso
      apply inMode (Problem.undefinedAction name key a)
      intro acc
      unfold modeProblems
      simp only [List.mem_append]
      refine Or.inl (Or.inl (foldl_adds (bindingProblem_keeps k (acts as) name) hb ?_ acc))
      intro acc'
      have hna : a ∉ acts as := by simpa using ha
      simp [bindingProblem, hna]
  · intro p hp
    cases hpm : k.hasMode p with
    | true => rfl
    | false =>
      exfalso
      apply inMode (Problem.undefinedParent name p)
      intro acc
      unfold modeProblems
      simp only [List.mem_append]
      refine Or.inl (Or.inr ?_)
      simp [hp, parentProblem, hpm]
  · cases hl : leaveFrom k (k.modes.length + 2) [name] name with
    | reachesRoot => rfl
    | loops =>
      exfalso
      apply inMode (Problem.parentCycle name)
      intro acc
      unfold modeProblems
      simp only [List.mem_append]
      refine Or.inr ?_
      simp [hl, leavingProblem]
    | stops =>
      exfalso
      apply inMode (Problem.stranded name)
      intro acc
      unfold modeProblems
      simp only [List.mem_append]
      refine Or.inr ?_
      simp [hl, leavingProblem]

def runKeys (k : Keymap) (mode : String) (keys : List String) : String :=
  keys.foldl (nextMode k) mode

theorem leaveFrom_reaches (k : Keymap) : ∀ (fuel : Nat) (seen : List String) (m : String),
    leaveFrom k fuel seen m = .reachesRoot →
    ∃ n, n ≤ fuel ∧ runKeys k m (List.replicate n leaveKey) = k.root := by
  intro fuel
  induction fuel with
  | zero => intro seen m h; simp [leaveFrom] at h
  | succ fuel ih =>
    intro seen m h
    unfold leaveFrom at h
    cases hs : step k m leaveKey with
    | dispatch a =>
      refine ⟨1, by omega, ?_⟩
      simp [runKeys, nextMode, hs]
    | passThrough =>
      rw [hs] at h
      by_cases hr : m = k.root
      · exact ⟨0, by omega, by simp [runKeys, hr]⟩
      · simp [hr] at h
    | enter t =>
      rw [hs] at h
      by_cases hc : t ∈ seen
      · simp [hc] at h
      · simp only [List.contains, List.elem_eq_mem, decide_eq_true_eq] at h
        rw [if_neg hc] at h
        obtain ⟨n, hn, hit⟩ := ih _ _ h
        refine ⟨n + 1, by omega, ?_⟩
        simp only [runKeys, List.replicate_succ, List.foldl_cons] at hit ⊢
        simpa [nextMode, hs] using hit
    | leave t =>
      rw [hs] at h
      by_cases hc : t ∈ seen
      · simp [hc] at h
      · simp only [List.contains, List.elem_eq_mem, decide_eq_true_eq] at h
        rw [if_neg hc] at h
        obtain ⟨n, hn, hit⟩ := ih _ _ h
        refine ⟨n + 1, by omega, ?_⟩
        simp only [runKeys, List.replicate_succ, List.foldl_cons] at hit ⊢
        simpa [nextMode, hs] using hit

/-- @proves REQ-MYTH.totality -/
theorem only_an_unbound_key_at_a_parentless_mode_passes_through (k : Keymap) (mode key : String) :
    step k mode key = .passThrough →
      k.mode mode = none ∨ ∃ m, k.mode mode = some m ∧ m.binding key = none ∧ m.parent = none := by
  intro h
  unfold step at h
  cases hm : k.mode mode with
  | none => exact Or.inl rfl
  | some m =>
    rw [hm] at h
    simp only at h
    refine Or.inr ⟨m, rfl, ?_⟩
    cases hb : m.binding key with
    | some b => rw [hb] at h; cases b <;> simp at h
    | none =>
      rw [hb] at h
      simp only at h
      cases hp : m.parent with
      | none => exact ⟨rfl, rfl⟩
      | some p => rw [hp] at h; split at h <;> simp at h

/-- @proves REQ-MYTH.escape_pops_one -/
theorem unbound_escape_goes_to_the_parent_and_others_to_the_root (k : Keymap) (mode key : String)
    (m : Mode) (p : String) (hm : k.mode mode = some m) (hb : m.binding key = none)
    (hp : m.parent = some p) :
    step k mode key = (if key = leaveKey then .leave p else .leave k.root) ∧
    nextMode k mode key = (if key = leaveKey then p else k.root) := by
  have hs : step k mode key = (if key = leaveKey then .leave p else .leave k.root) := by
    unfold step
    rw [hm]
    simp only [hb, hp]
    by_cases hk : key = leaveKey <;> simp [hk]
  refine ⟨hs, ?_⟩
  unfold nextMode
  rw [hs]
  by_cases hk : key = leaveKey <;> simp [hk]

/-- @proves REQ-MYTH.modes_defined -/
theorem a_valid_keymap_names_only_defined_modes (k : Keymap) (as : List String)
    (h : validate k as = []) :
    k.hasMode k.root = true ∧ ∀ name m, (name, m) ∈ k.modes →
      (∀ key t d, (key, Binding.enter t d) ∈ m.bindings → k.hasMode t = true) ∧
      (∀ p, m.parent = some p → k.hasMode p = true) := by
  refine ⟨(valid_parts h).1, fun name m hm => ?_⟩
  obtain ⟨h1, _, h3, _⟩ := valid_mode h hm
  exact ⟨h1, h3⟩

/-- @proves REQ-MYTH.actions_defined -/
theorem a_valid_keymap_dispatches_only_defined_actions (k : Keymap) (as : List String)
    (h : validate k as = []) :
    ∀ name m, (name, m) ∈ k.modes → ∀ key a d, (key, Binding.dispatch a d) ∈ m.bindings → a ∈ as :=
  fun _ _ hm => (valid_mode h hm).2.1

/-- @proves REQ-MYTH.escape_terminates -/
theorem repeated_escape_reaches_the_root (k : Keymap) (as : List String)
    (h : validate k as = []) (name : String) (hn : k.hasMode name = true) :
    ∃ n, n ≤ k.modes.length + 2 ∧ runKeys k name (List.replicate n leaveKey) = k.root := by
  obtain ⟨m, hm⟩ := mode_of_mem hn
  exact leaveFrom_reaches k _ _ _ (valid_mode h (mode_mem hm)).2.2.2

theorem run_fixed {k : Keymap} {x key : String} (h : nextMode k x key = x) :
    ∀ n, runKeys k x (List.replicate n key) = x := by
  intro n
  induction n with
  | zero => rfl
  | succ n ih =>
    simp only [runKeys, List.replicate_succ, List.foldl_cons, h] at ih ⊢
    exact ih

/-- @proves REQ-MYTH.escape_terminates -/
theorem a_stranded_mode_is_reported (k : Keymap) (as : List String) (name : String) (m : Mode)
    (hr : name ≠ k.root) (hm : k.mode name = some m) (hp : m.parent = none)
    (hb : m.binding leaveKey = none) : validate k as ≠ [] := by
  intro h
  have hfix : nextMode k name leaveKey = name := by
    simp [nextMode, step, hm, hb, hp]
  obtain ⟨n, _, hit⟩ := repeated_escape_reaches_the_root k as h name (by simp [Keymap.hasMode, hm])
  rw [run_fixed hfix n] at hit
  exact hr hit

/-- @proves REQ-MYTH.keymap_is_data -/
theorem a_valid_keymap_never_fails_on_a_key (k : Keymap) (as : List String)
    (h : validate k as = []) (mode key : String) :
    (∀ t, step k mode key = .enter t → k.hasMode t = true) ∧
    (∀ t, step k mode key = .leave t → k.hasMode t = true) ∧
    (∀ a, step k mode key = .dispatch a → a ∈ as) := by
  unfold step
  cases hm : k.mode mode with
  | none => simp
  | some m =>
    have hv := valid_mode h (mode_mem hm)
    simp only
    cases hb : m.binding key with
    | some b =>
      have hbm := binding_mem hb
      cases b with
      | enter t d => simp; exact hv.1 key t d hbm
      | dispatch a d => simp; exact hv.2.1 key a d hbm
    | none =>
      simp only
      split
      · cases hp : m.parent with
        | none => simp
        | some p => simp; exact hv.2.2.1 p hp
      · cases m.parent with
        | none => simp
        | some _ => simp; exact (valid_parts h).1

/-- @proves REQ-MYTH.whichkey_is_a_query -/
theorem whichKey_lists_what_the_keymap_binds (k : Keymap) (mode key : String) :
    ((∃ d, (key, d) ∈ whichKey k mode) ↔
      ∃ m, k.mode mode = some m ∧
        ((∃ b, (key, b) ∈ m.bindings) ∨ (key = leaveKey ∧ m.parent.isSome = true))) ∧
    (∀ d, (key, d) ∈ whichKey k mode → step k mode key ≠ .passThrough) := by
  unfold whichKey
  cases hm : k.mode mode with
  | none => simp
  | some m =>
    simp only
    constructor
    · constructor
      · rintro ⟨d, hd⟩
        refine ⟨m, rfl, ?_⟩
        simp only [List.mem_append, List.mem_map] at hd
        cases hd with
        | inl hd =>
          obtain ⟨b, hb, he⟩ := hd
          simp at he
          exact Or.inl ⟨b.2, by rw [← he.1]; exact hb⟩
        | inr hd =>
          cases hp : m.parent.isSome with
          | true => simp [hp] at hd; exact Or.inr ⟨hd.1, rfl⟩
          | false => simp [hp] at hd
      · rintro ⟨m', hm', hk⟩
        cases hm'
        cases hk with
        | inl hk =>
          obtain ⟨b, hb⟩ := hk
          exact ⟨b.description, by
            simp only [List.mem_append, List.mem_map]
            exact Or.inl ⟨(key, b), hb, rfl⟩⟩
        | inr hk =>
          exact ⟨"leave this mode", by simp [hk.2, hk.1]⟩
    · intro d hd
      unfold step
      rw [hm]
      simp only [List.mem_append, List.mem_map] at hd
      cases hd with
      | inl hd =>
        obtain ⟨b, hb, he⟩ := hd
        simp at he
        rw [← he.1] at *
        obtain ⟨b', hb'⟩ := binding_some_of_mem hb
        simp only [hb']
        cases b' <;> simp
      | inr hd =>
        cases hp : m.parent with
        | none => simp [hp] at hd
        | some p =>
          simp [hp] at hd
          rw [hd.1]
          cases hb : m.binding leaveKey with
          | some b => cases b <;> simp [hb]
          | none => simp [hb, hp]

theorem enter_reachable {k : Keymap}
    (hs : ∀ name m, k.mode name = some m → ∀ key b, (key, b) ∈ m.bindings → m.binding key = some b)
    {x : String} {m : Mode} (hx : ∃ keys, runKeys k k.root keys = x) (hm : k.mode x = some m)
    {t : String} (ht : t ∈ enterTargets m) : ∃ keys, runKeys k k.root keys = t := by
  obtain ⟨keys, hk⟩ := hx
  unfold enterTargets at ht
  rw [List.mem_filterMap] at ht
  obtain ⟨⟨key, b⟩, hb, he⟩ := ht
  cases b with
  | enter t' d =>
    simp at he
    subst he
    refine ⟨keys ++ [key], ?_⟩
    simp only [runKeys, List.foldl_append, List.foldl_cons, List.foldl_nil] at hk ⊢
    rw [hk]
    simp [nextMode, step, hm, hs x m hm key _ hb]
  | dispatch a d => simp at he

theorem reachable_sound {k : Keymap}
    (hs : ∀ name m, k.mode name = some m → ∀ key b, (key, b) ∈ m.bindings → m.binding key = some b) :
    ∀ (fuel : Nat) (seen frontier : List String),
      (∀ x ∈ seen, ∃ keys, runKeys k k.root keys = x) →
      (∀ x ∈ frontier, ∃ keys, runKeys k k.root keys = x) →
      ∀ x ∈ reachableFrom k fuel seen frontier, ∃ keys, runKeys k k.root keys = x := by
  intro fuel
  induction fuel with
  | zero => intro seen frontier hseen _ x hx; unfold reachableFrom at hx; exact hseen x hx
  | succ fuel ih =>
    intro seen frontier hseen hfr x hx
    cases frontier with
    | nil => unfold reachableFrom at hx; exact hseen x hx
    | cons name rest =>
      unfold reachableFrom at hx
      have hname := hfr name (List.mem_cons_self _ _)
      have hrest : ∀ y ∈ rest, ∃ keys, runKeys k k.root keys = y :=
        fun y hy => hfr y (List.mem_cons_of_mem _ hy)
      cases hm : k.mode name with
      | none =>
        rw [hm] at hx
        exact ih seen rest hseen hrest x hx
      | some m =>
        rw [hm] at hx
        simp only at hx
        have hq : ∀ y ∈ ((enterTargets m).filter (fun t => !seen.contains t && k.hasMode t)).foldl
            (fun acc t => if acc.contains t then acc else acc ++ [t]) [],
            ∃ keys, runKeys k k.root keys = y := by
          intro y hy
          rw [dedup_mem] at hy
          rcases hy with hy | hy
          · cases hy
          · exact enter_reachable hs hname hm (List.mem_filter.mp hy).1
        refine ih _ _ ?_ ?_ x hx
        · intro y hy
          rcases List.mem_append.mp hy with hy | hy
          · exact hseen y hy
          · exact hq y hy
        · intro y hy
          rcases List.mem_append.mp hy with hy | hy
          · exact hrest y hy
          · exact hq y hy

theorem dispatched_in {f : List String → String × Binding → List String}
    (hf : ∀ acc b a, a ∈ f acc b → a ∈ acc ∨ ∃ d, b.2 = Binding.dispatch a d) :
    ∀ (bs : List (String × Binding)) (acc : List String) (a : String), a ∈ bs.foldl f acc →
      a ∈ acc ∨ ∃ b ∈ bs, ∃ d, b.2 = Binding.dispatch a d := by
  intro bs
  induction bs with
  | nil => intro acc a h; exact Or.inl h
  | cons b bs ih =>
    intro acc a h
    rcases ih _ a h with h1 | ⟨b', hb', d, hd⟩
    · rcases hf acc b a h1 with h2 | ⟨d, hd⟩
      · exact Or.inl h2
      · exact Or.inr ⟨b, List.mem_cons_self _ _, d, hd⟩
    · exact Or.inr ⟨b', List.mem_cons_of_mem _ hb', d, hd⟩

theorem dispatchable_mem (k : Keymap) : ∀ (names acc : List String) (a : String),
    a ∈ names.foldl (dispatchableIn k) acc →
      a ∈ acc ∨ ∃ name ∈ names, ∃ m, k.mode name = some m ∧
        ∃ key d, (key, Binding.dispatch a d) ∈ m.bindings := by
  intro names
  induction names with
  | nil => intro acc a h; exact Or.inl h
  | cons name names ih =>
    intro acc a h
    rcases ih _ a h with h1 | ⟨n, hn, rest⟩
    · unfold dispatchableIn at h1
      cases hm : k.mode name with
      | none => rw [hm] at h1; exact Or.inl h1
      | some m =>
        rw [hm] at h1
        simp only at h1
        refine (dispatched_in ?_ m.bindings acc a h1).elim Or.inl ?_
        · intro acc' b a' h'
          obtain ⟨key, b⟩ := b
          cases b with
          | enter t d => exact Or.inl h'
          | dispatch x d =>
            simp only at h'
            split at h'
            · exact Or.inl h'
            · rcases List.mem_append.mp h' with h' | h'
              · exact Or.inl h'
              · simp at h'
                exact Or.inr ⟨d, by simp [h']⟩
        · rintro ⟨⟨key, b⟩, hb, d, hd⟩
          simp at hd
          subst hd
          exact Or.inr ⟨name, List.mem_cons_self _ _, m, hm, key, d, hb⟩
    · exact Or.inr ⟨n, List.mem_cons_of_mem _ hn, rest⟩

/-- @proves REQ-MYTH.actions_reachable -/
theorem every_action_of_a_valid_keymap_is_reachable (k : Keymap) (as : List String)
    (h : validate k as = [])
    (hs : ∀ name m, k.mode name = some m → ∀ key b, (key, b) ∈ m.bindings → m.binding key = some b) :
    ∀ a ∈ as, ∃ keys key, step k (runKeys k k.root keys) key = .dispatch a := by
  intro a ha
  have hc := (valid_parts h).2.2 a ((mem_acts as a).mpr ha)
  simp only [List.contains, List.elem_eq_mem, decide_eq_true_eq] at hc
  rcases dispatchable_mem k _ [] a hc with h0 | ⟨name, hn, m, hm, key, d, hb⟩
  · cases h0
  · have hroot : ∀ x ∈ [k.root], ∃ keys, runKeys k k.root keys = x := by
      intro x hx
      simp at hx
      exact ⟨[], by simp [runKeys, hx]⟩
    obtain ⟨keys, hk⟩ := reachable_sound hs _ _ _ hroot hroot name hn
    refine ⟨keys, key, ?_⟩
    rw [hk]
    simp [step, hm, hs name m hm key _ hb]

end TraceLean.Keymap
