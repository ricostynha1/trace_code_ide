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

/-- @proves REQ-CONTEXT.part_named -/
theorem labels_round_trip : allParts.all (fun p => partNamed (label p) == some p) = true := by
  native_decide

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

/-- @proves REQ-CONTEXT.neighbourhood_is_closed -/
theorem the_neighbourhood_is_transitive_and_once :
    neighbourhood graph "B" = (["A"], ["C", "D"]) ∧
    neighbourhood graph "D" = (["A", "B", "C"], []) ∧
    neighbourhood graph "X" = (["Y"], ["Y"]) := by
  native_decide

def claims : List Claim :=
  [⟨"implements", "REQ-A", some "one", "src/m.rs::convert", "src/m.rs", 3, some "m::convert", "fn convert()"⟩,
   ⟨"tests", "REQ-A", some "one", "tests/a.rs::t2", "tests/a.rs", 9, some "t2", "fn t2()"⟩,
   ⟨"models", "REQ-A", some "one", "M.lean::convert", "M.lean", 5, some "convert", "def convert"⟩,
   ⟨"tests", "REQ-B", none, "tests/b.rs::t1", "tests/b.rs", 2, some "t1", "fn t1()"⟩,
   ⟨"tests", "REQ-C", none, "tests/c.rs::t3", "tests/c.rs", 7, some "t3", "fn t3() convert()"⟩]

/-- @proves REQ-CONTEXT.claims_with_source -/
theorem claims_come_with_their_source :
    (claimsOn claims "REQ-A" none).code.map (fun i => (i.path, i.line, i.source))
      = [("src/m.rs", 3, "fn convert()")] ∧
    (claimsOn claims "REQ-A" none).tests.map (·.path) = ["tests/a.rs"] ∧
    (claimsOn claims "REQ-A" none).models.map (·.source) = ["def convert"] := by
  native_decide

/-- @proves REQ-CONTEXT.affected_tests -/
theorem affected_are_below_or_naming :
    (affected claims "REQ-A" none ["REQ-B"] []).map (·.path) = ["tests/b.rs", "tests/c.rs"] := by
  native_decide

/-- @proves REQ-CONTEXT.from_the_shell -/
theorem the_shell_chooses_parts_by_label :
    shellParts none = .chosen defaultParts ∧
    shellParts (some "all") = .chosen allParts ∧
    shellParts (some "affected-tests, code") = .chosen [.code, .affected] ∧
    shellParts (some "code,bogus") = .unknown "bogus" := by
  native_decide

end TraceLean.Context
