import Lean

open Lean

namespace TraceLean.Context

structure Node where
  id : String
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def parentsOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (·.id == id)).bind (·.refines)

def childrenOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (fun n => n.refines.contains id)).map (·.id)

def reach (next : String → List String) : Nat → List String → List String → List String
  | 0, _, seen => seen
  | _ + 1, [], seen => seen
  | fuel + 1, x :: rest, seen =>
    let fresh := ((next x).filter (fun y => !seen.contains y)).eraseDups
    reach next fuel (rest ++ fresh) (seen ++ fresh)

def fuelOf (nodes : List Node) : Nat :=
  nodes.length + (nodes.map (·.refines.length)).foldl (· + ·) 0 + 1

def closure (next : String → List String) (nodes : List Node) (id : String) : List String :=
  ((reach next (fuelOf nodes) [id] [id]).filter (· != id)).mergeSort (fun a b => decide (a ≤ b))

def ancestors (nodes : List Node) (id : String) : List String :=
  closure (parentsOf nodes) nodes id

def descendants (nodes : List Node) (id : String) : List String :=
  closure (childrenOf nodes) nodes id

/-- @models REQ-CONTEXT.neighbourhood_is_closed -/
def neighbourhood (nodes : List Node) (id : String) : List String × List String :=
  (ancestors nodes id, descendants nodes id)

inductive Part where
  | requirement | refines | refinedBy | code | tests | models | affected
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def allParts : List Part :=
  [.requirement, .refines, .refinedBy, .code, .tests, .models, .affected]

/-- @models REQ-CONTEXT.person_chooses -/
def partsShown (included : List Part) (filled : List Part) : List Part :=
  allParts.filter (fun p => included.contains p && filled.contains p)

def label : Part → String
  | .requirement => "requirement"
  | .refines => "refines"
  | .refinedBy => "refined by"
  | .code => "code"
  | .tests => "tests"
  | .models => "models"
  | .affected => "affected tests"

/-- @models REQ-CONTEXT.part_named -/
def partNamed (name : String) : Option Part :=
  allParts.find? (fun p => label p == name)

def defaultParts : List Part :=
  allParts.filter (fun p => p != .refinedBy)

inductive ShellParts where
  | chosen (parts : List Part)
  | unknown (name : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def blank (c : Char) : Bool :=
  c == ' ' || c == '\t' || c == '\r' || c == '\n'

def shellName (raw : String) : String :=
  let trimmed := ((raw.toList.dropWhile blank).reverse.dropWhile blank).reverse
  String.mk (trimmed.map (fun c => if c == '-' then ' ' else c))

/-- @models REQ-CONTEXT.from_the_shell -/
def shellParts (parts : Option String) : ShellParts :=
  match parts with
  | none => ShellParts.chosen defaultParts
  | some "all" => ShellParts.chosen allParts
  | some list =>
    let names := (list.splitOn ",").map shellName
    match names.find? (fun n => (partNamed n).isNone) with
    | some bad => ShellParts.unknown bad
    | none => ShellParts.chosen (allParts.filter (fun p => names.any (fun n => partNamed n == some p)))

/-- @proves REQ-CONTEXT.person_chooses -/
theorem shown_were_included (included filled : List Part) (p : Part)
    (h : List.Mem p (partsShown included filled)) : List.Mem p included := by
  have chosen := (Bool.and_eq_true _ _).mp (List.mem_filter.mp h).2
  exact List.elem_iff.mp chosen.1

structure Claim where
  role : String
  req : String
  clause : Option String
  ident : String
  path : String
  line : Nat
  symbol : Option String
  source : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Item where
  role : String
  claims : String
  path : String
  line : Nat
  symbol : Option String
  source : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Claimed where
  code : List Item
  tests : List Item
  models : List Item
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def onTarget (c : Claim) (id : String) (clause : Option String) : Bool :=
  c.req == id && (clause.isNone || c.clause == clause)

def clauseLabel (c : Claim) : String :=
  match c.clause with
  | some k => c.req ++ "." ++ k
  | none => c.req

def itemOf (c : Claim) : Item :=
  { role := c.role, claims := clauseLabel c, path := c.path, line := c.line,
    symbol := c.symbol, source := c.source }

def addClaim : List (String × Item) → String → Item → List (String × Item)
  | [], ident, found => [(ident, found)]
  | entry :: rest, ident, found =>
    if entry.1 == ident then
      if entry.2.role == found.role then
        (entry.1, { entry.2 with claims := entry.2.claims ++ ", " ++ found.claims }) :: rest
      else entry :: (rest ++ [(ident, found)])
    else entry :: addClaim rest ident found

def ofRoles (claims : List Claim) (id : String) (clause : Option String) (roles : List String)
    : List Item :=
  let chosen := claims.filter (fun c => onTarget c id clause && roles.contains c.role)
  (chosen.foldl (fun out c => addClaim out c.ident (itemOf c)) []).map (fun e => e.2)

/-- @models REQ-CONTEXT.claims_with_source -/
def claimsOn (claims : List Claim) (id : String) (clause : Option String) : Claimed :=
  { code := ofRoles claims id clause ["implements"],
    tests := ofRoles claims id clause ["tests"],
    models := ofRoles claims id clause ["models", "proves", "drt", "pins"] }

def isWord (c : Char) : Bool :=
  c.isAlphanum || c == '_'

def boundaryOk (text word : List Char) (place : Nat) : Bool :=
  let before := if place == 0 then none else text.get? (place - 1)
  let after := text.get? (place + word.length)
  !((before.map isWord).getD false) && !((after.map isWord).getD false)

def scanAt (text word : List Char) : Nat → Nat → Bool
  | 0, _ => false
  | fuel + 1, place =>
    if place > text.length then false
    else if word.isPrefixOf (text.drop place) then
      (if boundaryOk text word place then true
       else scanAt text word fuel (place + max word.length 1))
    else scanAt text word fuel (place + 1)

def names (text word : String) : Bool :=
  scanAt text.toList word.toList (text.length + 2) 0

def isTestPath (path : String) : Bool :=
  (path.splitOn "/").any (fun part =>
    part == "tests" || part == "test" || (part.splitOn "_test").length > 1
      || part.startsWith "test_")

def linesOf (text : String) : List String :=
  let pieces0 := text.splitOn "\n"
  let ended := pieces0.getLast? == some ""
  let pieces := if ended then pieces0.dropLast else pieces0
  let last := pieces.length
  pieces.enum.map (fun e =>
    if e.1 + 1 < last || ended then
      (if e.2.endsWith "\r" then e.2.dropRight 1 else e.2)
    else e.2)

def mostUses : Nat := 20

def trimBlank (raw : String) : String :=
  String.mk ((raw.toList.dropWhile blank).reverse.dropWhile blank).reverse

def fromBelow (claims : List Claim) (down : List String)
    (state : List String × List Item) : List String × List Item :=
  (claims.filter (fun c => c.role == "tests" && down.contains c.req)).foldl
    (fun acc c =>
      if acc.1.contains c.ident then acc else (acc.1 ++ [c.ident], acc.2 ++ [itemOf c]))
    state

def fromNames (claims : List Claim) (symbols : List String)
    (state : List String × List Item) : List String × List Item :=
  (claims.filter (fun c => c.role == "tests")).foldl
    (fun acc c =>
      if acc.1.contains c.ident then acc
      else if symbols.any (fun s => names c.source s) then (acc.1 ++ [c.ident], acc.2 ++ [itemOf c])
      else acc)
    state

def useOf (path : String) (line : Nat) (text : String) : Item :=
  { role := "uses", claims := "", path := path, line := line + 1, symbol := none,
    source := trimBlank text }

def useStep (symbols : List String) (path : String) (acc : Nat × List Item)
    (numbered : Nat × String) : Nat × List Item :=
  if acc.1 < mostUses && symbols.any (fun s => names numbered.2 s) then
    (acc.1 + 1, acc.2 ++ [useOf path numbered.1 numbered.2])
  else acc

def useWalk (symbols : List String) (acc : Nat × List Item) (file : String × String)
    : Nat × List Item :=
  (linesOf file.2).enum.foldl (fun a l => useStep symbols file.1 a l) acc

def usesIn (symbols : List String) (skip : List String) (files : List (String × String))
    (out : List Item) : List Item :=
  ((files.filter (fun f => isTestPath f.1 && !skip.contains f.1)).foldl
    (useWalk symbols) (0, out)).2

/-- @models REQ-CONTEXT.affected_tests -/
def affected (claims : List Claim) (id : String) (clause : Option String) (down : List String)
    (files : List (String × String)) : List Item :=
  let claimed := claimsOn claims id clause
  let placed := (claims.filter (fun c => c.role == "tests" && onTarget c id clause)).map (·.ident)
  let symbols := (claimed.code.filterMap (·.symbol)).map (fun s => ((s.splitOn "::").getLast?).getD s)
  let state := fromNames claims symbols (fromBelow claims down (placed, []))
  let claimedFiles := (state.2 ++ claimed.tests).map (·.path)
  usesIn symbols claimedFiles files state.2

def graph : List Node :=
  [⟨"A", []⟩, ⟨"B", ["A"]⟩, ⟨"C", ["B"]⟩, ⟨"D", ["C", "A"]⟩, ⟨"X", ["Y"]⟩, ⟨"Y", ["X"]⟩]

def claims : List Claim :=
  [⟨"implements", "REQ-A", some "one", "src/m.rs::convert", "src/m.rs", 3, some "m::convert", "fn convert()"⟩,
   ⟨"tests", "REQ-A", some "one", "tests/a.rs::t2", "tests/a.rs", 9, some "t2", "fn t2()"⟩,
   ⟨"models", "REQ-A", some "one", "M.lean::convert", "M.lean", 5, some "convert", "def convert"⟩,
   ⟨"tests", "REQ-B", none, "tests/b.rs::t1", "tests/b.rs", 2, some "t1", "fn t1()"⟩,
   ⟨"tests", "REQ-C", none, "tests/c.rs::t3", "tests/c.rs", 7, some "t3", "fn t3() convert()"⟩]


theorem partNamed_sound (name : String) (p : Part) (h : partNamed name = some p) : label p = name := by
  have := List.find?_some h
  simpa using this

theorem mem_allParts (p : Part) : p ∈ allParts := by
  cases p <;> simp [allParts]

/-- @proves REQ-CONTEXT.part_named -/
theorem parts_are_named_by_their_labels (name : String) (p : Part) :
    partNamed (label p) = some p ∧
    (partNamed name = some p → label p = name) ∧
    ((∀ q, label q ≠ name) → partNamed name = none) := by
  refine ⟨?_, partNamed_sound name p, ?_⟩
  · cases p <;> rfl
  · intro h
    cases e : partNamed name with
    | none => rfl
    | some q => exact absurd (partNamed_sound name q e) (h q)

/-- @proves REQ-CONTEXT.from_the_shell -/
theorem shell_parts_chosen_by_label_or_refused (list : String) (h : list ≠ "all") :
    shellParts none = .chosen defaultParts ∧
    shellParts (some "all") = .chosen allParts ∧
    (∀ bad, shellParts (some list) = .unknown bad →
      bad ∈ (list.splitOn ",").map shellName ∧ partNamed bad = none) ∧
    ((∃ n ∈ (list.splitOn ",").map shellName, partNamed n = none) →
      ∃ bad, shellParts (some list) = .unknown bad) ∧
    (∀ ps, shellParts (some list) = .chosen ps →
      ∀ p, p ∈ ps ↔ ∃ n ∈ (list.splitOn ",").map shellName, label p = n) := by
  have unfolded : shellParts (some list) =
      (match ((list.splitOn ",").map shellName).find? (fun n => (partNamed n).isNone) with
       | some bad => ShellParts.unknown bad
       | none => ShellParts.chosen (allParts.filter (fun p =>
           ((list.splitOn ",").map shellName).any (fun n => partNamed n == some p)))) := by
    simp only [shellParts]
  refine ⟨rfl, rfl, ?_, ?_, ?_⟩
  · intro bad hb
    rw [unfolded] at hb
    split at hb
    · rename_i b hf
      cases hb
      refine ⟨List.mem_of_find?_eq_some hf, ?_⟩
      have := List.find?_some hf
      simpa using this
    · cases hb
  · intro ⟨n, hn, hp⟩
    rw [unfolded]
    split
    · rename_i b _
      exact ⟨b, rfl⟩
    · rename_i hf
      have := List.find?_eq_none.mp hf n hn
      simp [hp] at this
  · intro ps hps p
    rw [unfolded] at hps
    split at hps
    · cases hps
    · cases hps
      simp only [List.mem_filter, List.any_eq_true, beq_iff_eq]
      constructor
      · intro ⟨_, n, hn, e⟩
        exact ⟨n, hn, partNamed_sound n p e⟩
      · intro ⟨n, hn, e⟩
        refine ⟨mem_allParts p, n, hn, ?_⟩
        subst e
        exact (parts_are_named_by_their_labels "" p).1

def Like (x y : String × Item) : Prop :=
  x.1 = y.1 ∧ x.2.role = y.2.role ∧ x.2.path = y.2.path ∧ x.2.line = y.2.line ∧
    x.2.symbol = y.2.symbol ∧ x.2.source = y.2.source

theorem like_refl (x : String × Item) : Like x x := ⟨rfl, rfl, rfl, rfl, rfl, rfl⟩

theorem addClaim_sound : ∀ (out : List (String × Item)) (ident : String) (found : Item),
    ∀ e ∈ addClaim out ident found, ∃ e0 ∈ (ident, found) :: out, Like e e0
  | [], ident, found, e, he => by
    simp only [addClaim, List.mem_singleton] at he
    exact ⟨e, by simp [he], like_refl e⟩
  | entry :: rest, ident, found, e, he => by
    simp only [addClaim] at he
    split at he
    · split at he
      · rcases List.mem_cons.mp he with h | h
        · subst h
          exact ⟨entry, by simp, rfl, rfl, rfl, rfl, rfl, rfl⟩
        · exact ⟨e, by simp [h], like_refl e⟩
      · rcases List.mem_cons.mp he with h | h
        · exact ⟨e, by simp [h], like_refl e⟩
        · rcases List.mem_append.mp h with h | h
          · exact ⟨e, by simp [h], like_refl e⟩
          · exact ⟨e, by simp_all, like_refl e⟩
    · rcases List.mem_cons.mp he with h | h
      · exact ⟨e, by simp [h], like_refl e⟩
      · obtain ⟨e0, h0, hl⟩ := addClaim_sound rest ident found e h
        refine ⟨e0, ?_, hl⟩
        rcases List.mem_cons.mp h0 with h0 | h0
        · simp [h0]
        · simp [h0]

theorem addClaim_new : ∀ (out : List (String × Item)) (ident : String) (found : Item),
    ∃ e ∈ addClaim out ident found, e.1 = ident ∧ e.2.role = found.role ∧
      found.claims.data <:+: e.2.claims.data
  | [], ident, found => ⟨(ident, found), by simp [addClaim], rfl, rfl, List.infix_refl _⟩
  | entry :: rest, ident, found => by
    simp only [addClaim]
    split
    · rename_i h1
      split
      · rename_i h2
        refine ⟨_, List.mem_cons_self _ _, by simpa using h1, by simpa using h2, ?_⟩
        exact ⟨(entry.2.claims ++ ", ").data, [], by simp [String.append]⟩
      · exact ⟨(ident, found), by simp, rfl, rfl, List.infix_refl _⟩
    · obtain ⟨e, he, h⟩ := addClaim_new rest ident found
      exact ⟨e, List.mem_cons_of_mem _ he, h⟩

theorem addClaim_keeps : ∀ (out : List (String × Item)) (ident : String) (found : Item),
    ∀ e0 ∈ out, ∃ e ∈ addClaim out ident found, Like e e0 ∧ e0.2.claims.data <:+: e.2.claims.data
  | [], _, _, e0, h => absurd h (List.not_mem_nil _)
  | entry :: rest, ident, found, e0, h0 => by
    simp only [addClaim]
    split
    · split
      · rcases List.mem_cons.mp h0 with h | h
        · subst h
          refine ⟨_, List.mem_cons_self _ _, ⟨rfl, rfl, rfl, rfl, rfl, rfl⟩, ?_⟩
          exact ⟨[], (", " ++ found.claims).data, by simp [String.append]⟩
        · exact ⟨e0, List.mem_cons_of_mem _ h, like_refl e0, List.infix_refl _⟩
      · exact ⟨e0, by rcases List.mem_cons.mp h0 with h | h <;> simp [h], like_refl e0, List.infix_refl _⟩
    · rcases List.mem_cons.mp h0 with h | h
      · exact ⟨e0, by simp [h], like_refl e0, List.infix_refl _⟩
      · obtain ⟨e, he, hl⟩ := addClaim_keeps rest ident found e0 h
        exact ⟨e, List.mem_cons_of_mem _ he, hl⟩

def ClaimFold (chosen : List Claim) : List Claim → List (String × Item) → List (String × Item)
  | [], out => out
  | c :: rest, out => ClaimFold chosen rest (addClaim out c.ident (itemOf c))

theorem claimFold_eq (chosen : List Claim) : ∀ (l : List Claim) (out : List (String × Item)),
    l.foldl (fun out c => addClaim out c.ident (itemOf c)) out = ClaimFold chosen l out
  | [], _ => rfl
  | _ :: rest, _ => claimFold_eq chosen rest _

theorem claimFold_inv (chosen : List Claim) : ∀ (l : List Claim) (out : List (String × Item)),
    (∀ c ∈ l, c ∈ chosen) →
    (∀ e ∈ out, ∃ c ∈ chosen, Like e (c.ident, itemOf c)) →
    (∀ e ∈ ClaimFold chosen l out, ∃ c ∈ chosen, Like e (c.ident, itemOf c)) ∧
    (∀ e0 ∈ out, ∃ e ∈ ClaimFold chosen l out, Like e e0 ∧ e0.2.claims.data <:+: e.2.claims.data) ∧
    (∀ c ∈ l, ∃ e ∈ ClaimFold chosen l out, e.1 = c.ident ∧ e.2.role = c.role ∧
      (clauseLabel c).data <:+: e.2.claims.data)
  | [], out, _, hs => ⟨hs, fun e0 h => ⟨e0, h, like_refl e0, List.infix_refl _⟩, fun _ h => absurd h (List.not_mem_nil _)⟩
  | c :: rest, out, hl, hs => by
    have hc : c ∈ chosen := hl c (List.mem_cons_self _ _)
    have hs' : ∀ e ∈ addClaim out c.ident (itemOf c), ∃ c ∈ chosen, Like e (c.ident, itemOf c) := by
      intro e he
      obtain ⟨e0, h0, hl0⟩ := addClaim_sound out c.ident (itemOf c) e he
      rcases List.mem_cons.mp h0 with h0 | h0
      · subst h0
        exact ⟨c, hc, hl0⟩
      · obtain ⟨c', hc', hl'⟩ := hs e0 h0
        refine ⟨c', hc', ?_⟩
        obtain ⟨a1, a2, a3, a4, a5, a6⟩ := hl0
        obtain ⟨b1, b2, b3, b4, b5, b6⟩ := hl'
        exact ⟨a1.trans b1, a2.trans b2, a3.trans b3, a4.trans b4, a5.trans b5, a6.trans b6⟩
    obtain ⟨r1, r2, r3⟩ := claimFold_inv chosen rest (addClaim out c.ident (itemOf c))
      (fun x hx => hl x (List.mem_cons_of_mem _ hx)) hs'
    refine ⟨r1, ?_, ?_⟩
    · intro e0 h0
      obtain ⟨e1, h1, l1, i1⟩ := addClaim_keeps out c.ident (itemOf c) e0 h0
      obtain ⟨e2, h2, l2, i2⟩ := r2 e1 h1
      obtain ⟨a1, a2, a3, a4, a5, a6⟩ := l2
      obtain ⟨b1, b2, b3, b4, b5, b6⟩ := l1
      exact ⟨e2, h2, ⟨a1.trans b1, a2.trans b2, a3.trans b3, a4.trans b4, a5.trans b5, a6.trans b6⟩,
        i1.trans i2⟩
    · intro c' hc'
      rcases List.mem_cons.mp hc' with h | h
      · subst h
        obtain ⟨e1, h1, n1, n2, n3⟩ := addClaim_new out c'.ident (itemOf c')
        obtain ⟨e2, h2, l2, i2⟩ := r2 e1 h1
        exact ⟨e2, h2, l2.1.trans n1, l2.2.1.trans n2, n3.trans i2⟩
      · exact r3 c' h

def ClaimsShown (claims : List Claim) (id : String) (clause : Option String) (roles : List String)
    (items : List Item) : Prop :=
  (∀ c ∈ claims, onTarget c id clause = true → c.role ∈ roles →
    ∃ it ∈ items, ∃ c' ∈ claims, onTarget c' id clause = true ∧ c'.ident = c.ident ∧
      it.role = c.role ∧ it.path = c'.path ∧ it.line = c'.line ∧ it.symbol = c'.symbol ∧
      it.source = c'.source ∧ (clauseLabel c).data <:+: it.claims.data) ∧
  (∀ it ∈ items, ∃ c' ∈ claims, onTarget c' id clause = true ∧ c'.role ∈ roles ∧
      it.role = c'.role ∧ it.path = c'.path ∧ it.line = c'.line ∧ it.symbol = c'.symbol ∧
      it.source = c'.source)

theorem ofRoles_shown (claims : List Claim) (id : String) (clause : Option String) (roles : List String) :
    ClaimsShown claims id clause roles (ofRoles claims id clause roles) := by
  let chosen := claims.filter (fun c => onTarget c id clause && roles.contains c.role)
  have hof : ofRoles claims id clause roles = (ClaimFold chosen chosen []).map (fun e => e.2) := by
    simp only [ofRoles]
    rw [claimFold_eq chosen]
  have mem_chosen : ∀ c, c ∈ chosen ↔ c ∈ claims ∧ onTarget c id clause = true ∧ c.role ∈ roles := by
    intro c
    simp [chosen, List.mem_filter]
  obtain ⟨r1, _, r3⟩ := claimFold_inv chosen chosen [] (fun _ h => h) (fun _ h => absurd h (List.not_mem_nil _))
  rw [hof]
  constructor
  · intro c hc ht hr
    obtain ⟨e, he, e1, e2, e3⟩ := r3 c ((mem_chosen c).mpr ⟨hc, ht, hr⟩)
    obtain ⟨c', hc', l1, _, l3, l4, l5, l6⟩ := r1 e he
    obtain ⟨m1, m2, _⟩ := (mem_chosen c').mp hc'
    exact ⟨e.2, List.mem_map_of_mem _ he, c', m1, m2, l1.symm.trans e1, e2, l3, l4, l5, l6, e3⟩
  · intro it hit
    obtain ⟨e, he, rfl⟩ := List.mem_map.mp hit
    obtain ⟨c', hc', _, l2, l3, l4, l5, l6⟩ := r1 e he
    obtain ⟨m1, m2, m3⟩ := (mem_chosen c').mp hc'
    exact ⟨c', m1, m2, m3, l2, l3, l4, l5, l6⟩

/-- @proves REQ-CONTEXT.claims_with_source -/
theorem every_claim_shown_with_its_source (claims : List Claim) (id : String) (clause : Option String) :
    ClaimsShown claims id clause ["implements"] (claimsOn claims id clause).code ∧
    ClaimsShown claims id clause ["tests"] (claimsOn claims id clause).tests ∧
    ClaimsShown claims id clause ["models", "proves", "drt", "pins"] (claimsOn claims id clause).models :=
  ⟨ofRoles_shown _ _ _ _, ofRoles_shown _ _ _ _, ofRoles_shown _ _ _ _⟩

def placedOf (claims : List Claim) (id : String) (clause : Option String) : List String :=
  (claims.filter (fun c => c.role == "tests" && onTarget c id clause)).map (·.ident)

def symbolsOf (claims : List Claim) (id : String) (clause : Option String) : List String :=
  ((claimsOn claims id clause).code.filterMap (·.symbol)).map (fun s => ((s.splitOn "::").getLast?).getD s)

theorem affected_eq (claims : List Claim) (id : String) (clause : Option String) (down : List String)
    (files : List (String × String)) :
    affected claims id clause down files =
      usesIn (symbolsOf claims id clause)
        (((fromNames claims (symbolsOf claims id clause)
            (fromBelow claims down (placedOf claims id clause, []))).2 ++
          (claimsOn claims id clause).tests).map (·.path)) files
        (fromNames claims (symbolsOf claims id clause)
          (fromBelow claims down (placedOf claims id clause, []))).2 := rfl

def TestState (claims : List Claim) (placed : List String) (P : Claim → Prop)
    (st : List String × List Item) : Prop :=
  (∀ x ∈ placed, x ∈ st.1) ∧
  (∀ x ∈ st.1, x ∈ placed ∨ ∃ c ∈ claims, c.ident = x ∧ itemOf c ∈ st.2) ∧
  (∀ it ∈ st.2, ∃ c ∈ claims, c.role = "tests" ∧ it = itemOf c ∧ c.ident ∉ placed ∧ P c)

theorem testFold (claims : List Claim) (placed : List String) (P : Claim → Prop)
    (cond : Claim → Bool) (step : List String × List Item → Claim → List String × List Item)
    (hstep : ∀ acc c, step acc c =
      if acc.1.contains c.ident then acc
      else if cond c then (acc.1 ++ [c.ident], acc.2 ++ [itemOf c]) else acc) :
    ∀ (l : List Claim) (st : List String × List Item),
      (∀ c ∈ l, c ∈ claims ∧ c.role = "tests" ∧ (cond c = true → P c)) →
      TestState claims placed P st →
      TestState claims placed P (l.foldl step st) ∧
      (∀ x ∈ st.1, x ∈ (l.foldl step st).1) ∧
      (∀ c ∈ l, cond c = true → c.ident ∈ (l.foldl step st).1)
  | [], st, _, hs => ⟨hs, fun _ h => h, fun _ h => absurd h (List.not_mem_nil _)⟩
  | c :: rest, st, hl, hs => by
    obtain ⟨hc, hrole, hp⟩ := hl c (List.mem_cons_self _ _)
    have hs' : TestState claims placed P (step st c) ∧ (∀ x ∈ st.1, x ∈ (step st c).1) ∧
        (cond c = true → c.ident ∈ (step st c).1) := by
      rw [hstep]
      by_cases hin : c.ident ∈ st.1
      · have : st.1.contains c.ident = true := List.elem_eq_true_of_mem hin
        simp only [this, ite_true]
        exact ⟨hs, fun _ h => h, fun _ => hin⟩
      · have : st.1.contains c.ident = false := by
          cases e : st.1.contains c.ident
          · rfl
          · exact absurd (List.mem_of_elem_eq_true e) hin
        simp only [this]
        by_cases hcond : cond c = true
        · simp only [hcond, ite_true, Bool.false_eq_true, ite_false]
          obtain ⟨h1, h2, h3⟩ := hs
          refine ⟨⟨fun x hx => List.mem_append_left _ (h1 x hx), ?_, ?_⟩,
            fun x hx => List.mem_append_left _ hx, fun _ => List.mem_append_right _ (List.mem_singleton_self _)⟩
          · intro x hx
            rcases List.mem_append.mp hx with hx | hx
            · rcases h2 x hx with h | ⟨c', hc', e, hi⟩
              · exact Or.inl h
              · exact Or.inr ⟨c', hc', e, List.mem_append_left _ hi⟩
            · rw [List.mem_singleton.mp hx]
              exact Or.inr ⟨c, hc, rfl, List.mem_append_right _ (List.mem_singleton_self _)⟩
          · intro it hit
            rcases List.mem_append.mp hit with hit | hit
            · exact h3 it hit
            · refine ⟨c, hc, hrole, List.mem_singleton.mp hit, fun hpl => hin (h1 _ hpl), hp hcond⟩
        · simp only [Bool.false_eq_true, ite_false, hcond]
          exact ⟨hs, fun _ h => h, fun h => by simp_all⟩
    obtain ⟨r1, r2, r3⟩ := testFold claims placed P cond step hstep rest (step st c)
      (fun x hx => hl x (List.mem_cons_of_mem _ hx)) hs'.1
    refine ⟨r1, fun x hx => r2 x (hs'.2.1 x hx), ?_⟩
    intro c' hc' hcond
    rcases List.mem_cons.mp hc' with h | h
    · subst h
      exact r2 _ (hs'.2.2 hcond)
    · exact r3 c' h hcond

theorem testState_start (claims : List Claim) (placed : List String) (P : Claim → Prop) :
    TestState claims placed P (placed, []) :=
  ⟨fun _ h => h, fun _ h => Or.inl h, fun _ h => absurd h (List.not_mem_nil _)⟩

theorem useStep_extends (symbols : List String) (path : String) (acc : Nat × List Item) (l : Nat × String) :
    ∃ extra, (useStep symbols path acc l).2 = acc.2 ++ extra ∧ ∀ it ∈ extra, it.role = "uses" := by
  unfold useStep
  split
  · exact ⟨[useOf path l.1 l.2], rfl, fun it h => by simp [List.mem_singleton.mp h, useOf]⟩
  · exact ⟨[], by simp, fun _ h => absurd h (List.not_mem_nil _)⟩

theorem fold_extends {β : Type} (f : Nat × List Item → β → Nat × List Item)
    (hf : ∀ acc b, ∃ extra, (f acc b).2 = acc.2 ++ extra ∧ ∀ it ∈ extra, it.role = "uses") :
    ∀ (l : List β) (acc : Nat × List Item),
      ∃ extra, (l.foldl f acc).2 = acc.2 ++ extra ∧ ∀ it ∈ extra, it.role = "uses"
  | [], acc => ⟨[], by simp, fun _ h => absurd h (List.not_mem_nil _)⟩
  | b :: rest, acc => by
    obtain ⟨e1, h1, r1⟩ := hf acc b
    obtain ⟨e2, h2, r2⟩ := fold_extends f hf rest (f acc b)
    refine ⟨e1 ++ e2, ?_, ?_⟩
    · simp only [List.foldl_cons, h2, h1, List.append_assoc]
    · intro it hit
      rcases List.mem_append.mp hit with h | h
      · exact r1 it h
      · exact r2 it h

theorem usesIn_extends (symbols skip : List String) (files : List (String × String)) (out : List Item) :
    ∃ extra, usesIn symbols skip files out = out ++ extra ∧ ∀ it ∈ extra, it.role = "uses" :=
  fold_extends (useWalk symbols)
    (fun acc file => fold_extends (fun a l => useStep symbols file.1 a l)
      (fun a l => useStep_extends symbols file.1 a l) _ acc) _ (0, out)

/-- @proves REQ-CONTEXT.affected_tests -/
theorem affected_tests_listed (claims : List Claim) (id : String) (clause : Option String)
    (down : List String) (files : List (String × String)) :
    (∀ c ∈ claims, c.role = "tests" →
      (c.req ∈ down ∨ (symbolsOf claims id clause).any (fun s => names c.source s) = true) →
      c.ident ∉ placedOf claims id clause →
      ∃ c' ∈ claims, c'.ident = c.ident ∧ itemOf c' ∈ affected claims id clause down files) ∧
    (∀ it ∈ affected claims id clause down files, it.role = "uses" ∨
      ∃ c ∈ claims, c.role = "tests" ∧ it = itemOf c ∧ c.ident ∉ placedOf claims id clause ∧
        (c.req ∈ down ∨ (symbolsOf claims id clause).any (fun s => names c.source s) = true)) := by
  let placed := placedOf claims id clause
  let symbols := symbolsOf claims id clause
  let P : Claim → Prop := fun c => c.req ∈ down ∨ symbols.any (fun s => names c.source s) = true
  have below := testFold claims placed P (fun _ => true)
    (fun acc c => if acc.1.contains c.ident then acc else (acc.1 ++ [c.ident], acc.2 ++ [itemOf c]))
    (fun acc c => by simp)
    (claims.filter (fun c => c.role == "tests" && down.contains c.req)) (placed, [])
    (fun c hc => by
      have := List.mem_filter.mp hc
      simp only [Bool.and_eq_true, beq_iff_eq] at this
      exact ⟨this.1, this.2.1, fun _ => Or.inl (List.mem_of_elem_eq_true this.2.2)⟩)
    (testState_start claims placed P)
  have byName := testFold claims placed P (fun c => symbols.any (fun s => names c.source s))
    (fun acc c =>
      if acc.1.contains c.ident then acc
      else if symbols.any (fun s => names c.source s) then (acc.1 ++ [c.ident], acc.2 ++ [itemOf c])
      else acc)
    (fun _ _ => rfl)
    (claims.filter (fun c => c.role == "tests")) _
    (fun c hc => by
      have := List.mem_filter.mp hc
      simp only [beq_iff_eq] at this
      exact ⟨this.1, this.2, fun h => Or.inr h⟩)
    below.1
  have hstate : fromNames claims symbols (fromBelow claims down (placed, [])) =
      (claims.filter (fun c => c.role == "tests")).foldl
        (fun acc c =>
          if acc.1.contains c.ident then acc
          else if symbols.any (fun s => names c.source s) then (acc.1 ++ [c.ident], acc.2 ++ [itemOf c])
          else acc)
        ((claims.filter (fun c => c.role == "tests" && down.contains c.req)).foldl
          (fun acc c => if acc.1.contains c.ident then acc else (acc.1 ++ [c.ident], acc.2 ++ [itemOf c]))
          (placed, [])) := rfl
  obtain ⟨extra, hextra, huses⟩ := usesIn_extends symbols
    (((fromNames claims symbols (fromBelow claims down (placed, []))).2 ++
      (claimsOn claims id clause).tests).map (·.path)) files
    (fromNames claims symbols (fromBelow claims down (placed, []))).2
  rw [affected_eq]
  show (∀ c ∈ claims, c.role = "tests" → P c → c.ident ∉ placed →
      ∃ c' ∈ claims, c'.ident = c.ident ∧ itemOf c' ∈ usesIn symbols
        (((fromNames claims symbols (fromBelow claims down (placed, []))).2 ++
          (claimsOn claims id clause).tests).map (·.path)) files
        (fromNames claims symbols (fromBelow claims down (placed, []))).2) ∧ _
  rw [hextra, hstate]
  have s2 := byName.1.2.1
  have s3 := byName.1.2.2
  constructor
  · intro c hc hrole hP hnot
    have hin : c.ident ∈ (fromNames claims symbols (fromBelow claims down (placed, []))).1 := by
      rcases hP with hd | hn
      · apply byName.2.1
        apply below.2.2 c _ rfl
        simpa [List.mem_filter, hc, hrole] using hd
      · exact byName.2.2 c (by simp [List.mem_filter, hc, hrole]) hn
    rcases s2 _ hin with h | ⟨c', hc', e, hi⟩
    · exact absurd h hnot
    · exact ⟨c', hc', e, List.mem_append_left _ hi⟩
  · intro it hit
    rcases List.mem_append.mp hit with h | h
    · exact Or.inr (s3 it h)
    · exact Or.inl (huses it h)

inductive ReachFrom (next : String → List String) (a : String) : String → Prop where
  | refl : ReachFrom next a a
  | step {b c : String} : ReachFrom next a b → c ∈ next b → ReachFrom next a c

theorem eraseDups_loop : ∀ (as bs : List String), bs.Nodup →
    (List.eraseDups.loop as bs).Nodup ∧ ∀ x, x ∈ List.eraseDups.loop as bs ↔ x ∈ as ∨ x ∈ bs
  | [], bs, h => ⟨List.pairwise_reverse.mpr (List.Pairwise.imp (fun hne e => hne e.symm) h), fun x => by simp [List.eraseDups.loop]⟩
  | a :: as, bs, h => by
    unfold List.eraseDups.loop
    split
    · rename_i he
      have ha : a ∈ bs := List.mem_of_elem_eq_true he
      obtain ⟨n, m⟩ := eraseDups_loop as bs h
      refine ⟨n, fun x => ?_⟩
      rw [m]
      constructor
      · intro hx
        rcases hx with hx | hx
        · exact Or.inl (List.mem_cons_of_mem _ hx)
        · exact Or.inr hx
      · intro hx
        rcases hx with hx | hx
        · rcases List.mem_cons.mp hx with e | hx
          · exact Or.inr (e ▸ ha)
          · exact Or.inl hx
        · exact Or.inr hx
    · rename_i he
      have ha : a ∉ bs := fun hm => by simp at he; exact he hm
      obtain ⟨n, m⟩ := eraseDups_loop as (a :: bs) (List.nodup_cons.mpr ⟨ha, h⟩)
      refine ⟨n, fun x => ?_⟩
      rw [m]
      simp only [List.mem_cons]
      constructor
      · intro hx
        rcases hx with hx | hx | hx
        · exact Or.inl (Or.inr hx)
        · exact Or.inl (Or.inl hx)
        · exact Or.inr hx
      · intro hx
        rcases hx with (hx | hx) | hx
        · exact Or.inr (Or.inl hx)
        · exact Or.inl hx
        · exact Or.inr (Or.inr hx)

theorem eraseDups_spec (as : List String) :
    as.eraseDups.Nodup ∧ ∀ x, x ∈ as.eraseDups ↔ x ∈ as := by
  obtain ⟨n, m⟩ := eraseDups_loop as [] List.nodup_nil
  exact ⟨n, fun x => by simpa using m x⟩

theorem nodup_length_le : ∀ (l m : List String), l.Nodup → (∀ x ∈ l, x ∈ m) → l.length ≤ m.length
  | [], _, _, _ => Nat.zero_le _
  | a :: l, m, hn, hs => by
    have ha : a ∈ m := hs a (List.mem_cons_self _ _)
    have hn' := List.nodup_cons.mp hn
    have ih := nodup_length_le l (m.erase a) hn'.2 (fun x hx => by
      have hxa : x ≠ a := fun e => hn'.1 (e ▸ hx)
      exact (List.mem_erase_of_ne hxa).mpr (hs x (List.mem_cons_of_mem _ hx)))
    rw [List.length_erase_of_mem ha] at ih
    have : 0 < m.length := List.length_pos_of_mem ha
    simp only [List.length_cons]
    omega

def ReachState (next : String → List String) (id : String) (U : List String) (queue seen : List String) : Prop :=
  seen.Nodup ∧ id ∈ seen ∧ (∀ y ∈ seen, ReachFrom next id y) ∧ (∀ y ∈ seen, y = id ∨ y ∈ U) ∧
    ∃ done, seen = done ++ queue ∧ ∀ y ∈ done, ∀ z ∈ next y, z ∈ seen

theorem reach_result (next : String → List String) (id : String) (U : List String)
    (hU : ∀ y, ∀ z ∈ next y, z ∈ U) :
    ∀ (fuel : Nat) (queue seen : List String) (done : List String),
      ReachState next id U queue seen → seen = done ++ queue →
      U.length + 1 ≤ fuel + done.length →
      let r := reach next fuel queue seen
      r.Nodup ∧ id ∈ r ∧ (∀ y ∈ r, ReachFrom next id y) ∧ (∀ y ∈ r, ∀ z ∈ next y, z ∈ r)
  | 0, queue, seen, done, ⟨hn, hid, hs, hu, done', hd', hc⟩, hd, hf => by
    have hq : queue = [] := by
      have hlen := nodup_length_le seen (id :: U) hn (fun y hy => by
        rcases hu y hy with e | e
        · exact e ▸ List.mem_cons_self _ _
        · exact List.mem_cons_of_mem _ e)
      rw [hd, List.length_append] at hlen
      simp only [List.length_cons] at hlen
      cases queue with
      | nil => rfl
      | cons q qs => simp at hlen; omega
    subst hq
    have : done = done' := by simpa [hd] using hd'
    subst this
    simp only [List.append_nil] at hd
    subst hd
    exact ⟨hn, hid, hs, hc⟩
  | fuel + 1, [], seen, done, ⟨hn, hid, hs, _, done', hd', hc⟩, hd, _ => by
    have : done = done' := by simpa [hd] using hd'
    subst this
    simp only [List.append_nil] at hd
    subst hd
    exact ⟨hn, hid, hs, hc⟩
  | fuel + 1, x :: rest, seen, done, ⟨hn, hid, hs, hu, done', hd', hc⟩, hd, hf => by
    have : done = done' := by
      rw [hd] at hd'
      exact List.append_cancel_right hd'
    subst this
    show
      let r := reach next fuel (rest ++ ((next x).filter (fun y => !seen.contains y)).eraseDups)
        (seen ++ ((next x).filter (fun y => !seen.contains y)).eraseDups)
      r.Nodup ∧ id ∈ r ∧ (∀ y ∈ r, ReachFrom next id y) ∧ (∀ y ∈ r, ∀ z ∈ next y, z ∈ r)
    obtain ⟨fn, fm⟩ := eraseDups_spec ((next x).filter (fun y => !seen.contains y))
    have mem_fresh : ∀ z, z ∈ ((next x).filter (fun y => !seen.contains y)).eraseDups ↔
        z ∈ next x ∧ z ∉ seen := by
      intro z
      rw [fm z, List.mem_filter]
      constructor
      · intro ⟨h1, h2⟩
        refine ⟨h1, fun hm => ?_⟩
        simp at h2; exact h2 hm
      · intro ⟨h1, h2⟩
        refine ⟨h1, ?_⟩
        cases e : seen.contains z
        · rfl
        · exact absurd (List.mem_of_elem_eq_true e) h2
    have hx : x ∈ seen := by rw [hd]; simp
    apply reach_result next id U hU fuel _ _ (done ++ [x])
    · refine ⟨?_, List.mem_append_left _ hid, ?_, ?_, done ++ [x], by simp [hd], ?_⟩
      · rw [List.Nodup, List.pairwise_append]
        refine ⟨hn, fn, fun a ha b hb e => ?_⟩
        subst e
        exact ((mem_fresh a).mp hb).2 ha
      · intro y hy
        rcases List.mem_append.mp hy with hy | hy
        · exact hs y hy
        · exact ReachFrom.step (hs x hx) ((mem_fresh y).mp hy).1
      · intro y hy
        rcases List.mem_append.mp hy with hy | hy
        · exact hu y hy
        · exact Or.inr (hU x y ((mem_fresh y).mp hy).1)
      · intro y hy z hz
        rcases List.mem_append.mp hy with hy | hy
        · exact List.mem_append_left _ (hc y hy z hz)
        · rw [List.mem_singleton.mp hy] at hz
          by_cases hzs : z ∈ seen
          · exact List.mem_append_left _ hzs
          · exact List.mem_append_right _ ((mem_fresh z).mpr ⟨hz, hzs⟩)
    · simp [hd]
    · simp only [List.length_append, List.length_singleton]
      omega

theorem closure_spec (next : String → List String) (nodes : List Node) (id : String) (U : List String)
    (hU : ∀ y, ∀ z ∈ next y, z ∈ U) (hlen : U.length + 1 ≤ fuelOf nodes) :
    (closure next nodes id).Nodup ∧ ∀ x, x ∈ closure next nodes id ↔ x ≠ id ∧ ReachFrom next id x := by
  have start : ReachState next id U [id] [id] :=
    ⟨List.nodup_cons.mpr ⟨List.not_mem_nil _, List.nodup_nil⟩, List.mem_singleton_self _,
      fun y hy => List.mem_singleton.mp hy ▸ ReachFrom.refl, fun y hy => Or.inl (List.mem_singleton.mp hy),
      [], rfl, fun _ h => absurd h (List.not_mem_nil _)⟩
  obtain ⟨rn, rid, rs, rc⟩ := reach_result next id U hU (fuelOf nodes) [id] [id] [] start rfl
    (by simpa using hlen)
  have complete : ∀ x, ReachFrom next id x → x ∈ reach next (fuelOf nodes) [id] [id] := by
    intro x h
    induction h with
    | refl => exact rid
    | step _ hc ih => exact rc _ ih _ hc
  have perm := List.mergeSort_perm (le := fun a b : String => decide (a ≤ b))
    ((reach next (fuelOf nodes) [id] [id]).filter (· != id))
  constructor
  · exact (List.Perm.nodup_iff perm).mpr (List.Pairwise.filter _ rn)
  · intro x
    unfold closure
    rw [perm.mem_iff, List.mem_filter]
    constructor
    · intro ⟨h1, h2⟩
      exact ⟨by simpa using h2, rs x h1⟩
    · intro ⟨h1, h2⟩
      exact ⟨complete x h2, by simpa using h1⟩

theorem foldl_add_eq (l : List Nat) (a : Nat) : l.foldl (· + ·) a = a + l.foldl (· + ·) 0 := by
  induction l generalizing a with
  | nil => simp
  | cons b rest ih =>
    simp only [List.foldl_cons]
    rw [ih (a + b), ih (0 + b)]
    omega

theorem refines_total_length (nodes : List Node) :
    (nodes.bind (·.refines)).length = (nodes.map (·.refines.length)).foldl (· + ·) 0 := by
  induction nodes with
  | nil => rfl
  | cons n rest ih =>
    simp only [List.bind_cons, List.length_append, List.map_cons, List.foldl_cons, ih]
    rw [foldl_add_eq _ (0 + n.refines.length)]
    omega

/-- @proves REQ-CONTEXT.neighbourhood_is_closed -/
theorem neighbourhood_is_the_transitive_closure (nodes : List Node) (id : String) :
    (neighbourhood nodes id).1.Nodup ∧ (neighbourhood nodes id).2.Nodup ∧
    (∀ x, x ∈ (neighbourhood nodes id).1 ↔ x ≠ id ∧ ReachFrom (parentsOf nodes) id x) ∧
    (∀ x, x ∈ (neighbourhood nodes id).2 ↔ x ≠ id ∧ ReachFrom (childrenOf nodes) id x) := by
  have up := closure_spec (parentsOf nodes) nodes id (nodes.bind (·.refines))
    (fun y z hz => by
      unfold parentsOf at hz
      obtain ⟨n, hn, hz⟩ := List.mem_bind.mp hz
      exact List.mem_bind.mpr ⟨n, (List.mem_filter.mp hn).1, hz⟩)
    (by rw [refines_total_length]; unfold fuelOf; omega)
  have down := closure_spec (childrenOf nodes) nodes id (nodes.map (·.id))
    (fun y z hz => by
      unfold childrenOf at hz
      obtain ⟨n, hn, hz⟩ := List.mem_map.mp hz
      exact List.mem_map.mpr ⟨n, (List.mem_filter.mp hn).1, hz⟩)
    (by rw [List.length_map]; unfold fuelOf; omega)
  exact ⟨up.1, down.1, up.2, down.2⟩
end TraceLean.Context
