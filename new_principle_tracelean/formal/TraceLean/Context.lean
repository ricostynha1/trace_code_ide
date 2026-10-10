import Lean

/-!
# What an agent needs to see to change a requirement

Models `REQ-CONTEXT`: the neighbourhood of a requirement in the refinement
graph, and which parts of its context a person's choice puts in the copied
text.
-/

open Lean

namespace TraceLean.Context

/-- A requirement as the refinement graph sees it: its name and its parents. -/
structure Node where
  id : String
  refines : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What `id` refines, one step up. A name declared twice refines what both
declarations say. -/
def parentsOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (·.id == id)).bind (·.refines)

/-- What refines `id`, one step down. -/
def childrenOf (nodes : List Node) (id : String) : List String :=
  (nodes.filter (fun n => n.refines.contains id)).map (·.id)

/-- Every name `next` reaches from the queue, added to `seen` once each. The
fuel is spent one queued name at a time, and no name is queued twice. -/
def reach (next : String → List String) : Nat → List String → List String → List String
  | 0, _, seen => seen
  | _ + 1, [], seen => seen
  | fuel + 1, x :: rest, seen =>
    let fresh := ((next x).filter (fun y => !seen.contains y)).eraseDups
    reach next fuel (rest ++ fresh) (seen ++ fresh)

/-- Enough fuel to queue every name the graph mentions. -/
def fuelOf (nodes : List Node) : Nat :=
  nodes.length + (nodes.map (·.refines.length)).foldl (· + ·) 0 + 1

def closure (next : String → List String) (nodes : List Node) (id : String) : List String :=
  ((reach next (fuelOf nodes) [id] [id]).filter (· != id)).mergeSort (fun a b => decide (a ≤ b))

/-- Everything `id` refines, transitively, each once, in name order.

@models REQ-CONTEXT.neighbourhood_is_closed -/
def ancestors (nodes : List Node) (id : String) : List String :=
  closure (parentsOf nodes) nodes id

/-- Everything that refines `id`, transitively, each once, in name order. -/
def descendants (nodes : List Node) (id : String) : List String :=
  closure (childrenOf nodes) nodes id

/-- One part of a context. -/
inductive Part where
  | requirement | refines | refinedBy | code | tests | models | affected
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The parts, in the one order they are shown and copied in. -/
def allParts : List Part :=
  [.requirement, .refines, .refinedBy, .code, .tests, .models, .affected]

/-- The parts the copied text holds: those included that have something in
them, in the fixed order — whatever order they were chosen in.

@models REQ-CONTEXT.person_chooses -/
def partsShown (included : List Part) (filled : List Part) : List Part :=
  allParts.filter (fun p => included.contains p && filled.contains p)

/-- What the view calls a part. -/
def label : Part → String
  | .requirement => "requirement"
  | .refines => "refines"
  | .refinedBy => "refined by"
  | .code => "code"
  | .tests => "tests"
  | .models => "models"
  | .affected => "affected tests"

/-- The part a label names, if it names one.

@models REQ-CONTEXT.part_named -/
def partNamed (name : String) : Option Part :=
  allParts.find? (fun p => label p == name)

/-- What a part holds when nobody has chosen: everything but what refines the
target. -/
def defaultParts : List Part :=
  allParts.filter (fun p => p != .refinedBy)

/-- What a shell's `--parts` chose: the parts, in their fixed order and each
once, or the first name that is no part. -/
inductive ShellParts where
  | chosen (parts : List Part)
  | unknown (name : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Space, tab, carriage return and line feed: what is trimmed from a name. -/
def blank (c : Char) : Bool :=
  c == ' ' || c == '\t' || c == '\r' || c == '\n'

/-- A name as a shell wrote it: trimmed, and a dash for a space. -/
def shellName (raw : String) : String :=
  let trimmed := ((raw.toList.dropWhile blank).reverse.dropWhile blank).reverse
  String.mk (trimmed.map (fun c => if c == '-' then ' ' else c))

/-- The parts a shell asked for: a comma-separated list of labels, or `all`, or
nothing for the default. The first label that names no part is refused.

@models REQ-CONTEXT.from_the_shell -/
def shellParts (parts : Option String) : ShellParts :=
  match parts with
  | none => ShellParts.chosen defaultParts
  | some "all" => ShellParts.chosen allParts
  | some list =>
    let names := (list.splitOn ",").map shellName
    match names.find? (fun n => (partNamed n).isNone) with
    | some bad => ShellParts.unknown bad
    | none => ShellParts.chosen (allParts.filter (fun p => names.any (fun n => partNamed n == some p)))

/-- A part is shown only if it was included.

@proves REQ-CONTEXT.person_chooses -/
theorem shown_were_included (included filled : List Part) (p : Part)
    (h : List.Mem p (partsShown included filled)) : List.Mem p included := by
  have chosen := (Bool.and_eq_true _ _).mp (List.mem_filter.mp h).2
  exact List.elem_iff.mp chosen.1

/-- Every label names its own part back.

@proves REQ-CONTEXT.part_named -/
theorem labels_round_trip : allParts.all (fun p => partNamed (label p) == some p) = true := by
  native_decide

/-! ## What claims a target, and what a change may break -/

/-- One annotation, flattened, with the source of the item it sits on. -/
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

/-- A claimed item, or a line that names one. -/
structure Item where
  role : String
  claims : String
  path : String
  line : Nat
  symbol : Option String
  source : String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- What claims a target, by kind. -/
structure Claimed where
  code : List Item
  tests : List Item
  models : List Item
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- Whether a claim is on the target: its requirement, and its clause when one
was asked for. -/
def onTarget (c : Claim) (id : String) (clause : Option String) : Bool :=
  c.req == id && (clause.isNone || c.clause == clause)

/-- The clause a claim makes, as `REQ-X.clause`. -/
def clauseLabel (c : Claim) : String :=
  match c.clause with
  | some k => c.req ++ "." ++ k
  | none => c.req

/-- A claim as an item. -/
def itemOf (c : Claim) : Item :=
  { role := c.role, claims := clauseLabel c, path := c.path, line := c.line,
    symbol := c.symbol, source := c.source }

/-- An item added to the list by anchor: merged into the first item of its
anchor when that one has the same role, else placed last. -/
def addClaim : List (String × Item) → String → Item → List (String × Item)
  | [], ident, found => [(ident, found)]
  | entry :: rest, ident, found =>
    if entry.1 == ident then
      if entry.2.role == found.role then
        (entry.1, { entry.2 with claims := entry.2.claims ++ ", " ++ found.claims }) :: rest
      else entry :: (rest ++ [(ident, found)])
    else entry :: addClaim rest ident found

/-- The claims on the target with one of `roles`, an item an anchor. -/
def ofRoles (claims : List Claim) (id : String) (clause : Option String) (roles : List String)
    : List Item :=
  let chosen := claims.filter (fun c => onTarget c id clause && roles.contains c.role)
  (chosen.foldl (fun out c => addClaim out c.ident (itemOf c)) []).map (fun e => e.2)

/-- Every claim on `id` (and `clause`, when one was asked for), by kind.

@models REQ-CONTEXT.claims_with_source -/
def claimsOn (claims : List Claim) (id : String) (clause : Option String) : Claimed :=
  { code := ofRoles claims id clause ["implements"],
    tests := ofRoles claims id clause ["tests"],
    models := ofRoles claims id clause ["models", "proves", "drt", "pins"] }

/-- A letter, a digit or an underscore, ASCII only. -/
def isWord (c : Char) : Bool :=
  c.isAlphanum || c == '_'

/-- Whether the characters either side of a match at `place` are not word
characters. -/
def boundaryOk (text word : List Char) (place : Nat) : Bool :=
  let before := if place == 0 then none else text.get? (place - 1)
  let after := text.get? (place + word.length)
  !((before.map isWord).getD false) && !((after.map isWord).getD false)

/-- Whether a word starts at `place` in `text`, bounded by non-word characters;
matches are taken left to right without overlapping. -/
def scanAt (text word : List Char) : Nat → Nat → Bool
  | 0, _ => false
  | fuel + 1, place =>
    if place > text.length then false
    else if word.isPrefixOf (text.drop place) then
      (if boundaryOk text word place then true
       else scanAt text word fuel (place + max word.length 1))
    else scanAt text word fuel (place + 1)

/-- Whether `text` names `word` as a whole word. -/
def names (text word : String) : Bool :=
  scanAt text.toList word.toList (text.length + 2) 0

/-- A path that holds tests by where it is or what it is called. -/
def isTestPath (path : String) : Bool :=
  (path.splitOn "/").any (fun part =>
    part == "tests" || part == "test" || (part.splitOn "_test").length > 1
      || part.startsWith "test_")

/-- A text's lines: split at line feeds, no line after a final one, and a
carriage return before a line feed dropped. -/
def linesOf (text : String) : List String :=
  let pieces0 := text.splitOn "\n"
  let ended := pieces0.getLast? == some ""
  let pieces := if ended then pieces0.dropLast else pieces0
  let last := pieces.length
  pieces.enum.map (fun e =>
    if e.1 + 1 < last || ended then
      (if e.2.endsWith "\r" then e.2.dropRight 1 else e.2)
    else e.2)

/-- The most lines named outside any claim that an affected list carries. -/
def mostUses : Nat := 20

/-- A name trimmed of blanks. -/
def trimBlank (raw : String) : String :=
  String.mk ((raw.toList.dropWhile blank).reverse.dropWhile blank).reverse

/-- The tests claiming what refines the target, placed once. -/
def fromBelow (claims : List Claim) (down : List String)
    (state : List String × List Item) : List String × List Item :=
  (claims.filter (fun c => c.role == "tests" && down.contains c.req)).foldl
    (fun acc c =>
      if acc.1.contains c.ident then acc else (acc.1 ++ [c.ident], acc.2 ++ [itemOf c]))
    state

/-- The tests whose source names an item that implements the target. -/
def fromNames (claims : List Claim) (symbols : List String)
    (state : List String × List Item) : List String × List Item :=
  (claims.filter (fun c => c.role == "tests")).foldl
    (fun acc c =>
      if acc.1.contains c.ident then acc
      else if symbols.any (fun s => names c.source s) then (acc.1 ++ [c.ident], acc.2 ++ [itemOf c])
      else acc)
    state

/-- A line of a test file as a use of an implementing item. -/
def useOf (path : String) (line : Nat) (text : String) : Item :=
  { role := "uses", claims := "", path := path, line := line + 1, symbol := none,
    source := trimBlank text }

/-- One line: counted, and listed, when it names a symbol and there is room. -/
def useStep (symbols : List String) (path : String) (acc : Nat × List Item)
    (numbered : Nat × String) : Nat × List Item :=
  if acc.1 < mostUses && symbols.any (fun s => names numbered.2 s) then
    (acc.1 + 1, acc.2 ++ [useOf path numbered.1 numbered.2])
  else acc

/-- One file's lines, in order. -/
def useWalk (symbols : List String) (acc : Nat × List Item) (file : String × String)
    : Nat × List Item :=
  (linesOf file.2).enum.foldl (fun a l => useStep symbols file.1 a l) acc

/-- Lines of test files nobody claimed that name an implementing item, at most
`mostUses` of them in all. -/
def usesIn (symbols : List String) (skip : List String) (files : List (String × String))
    (out : List Item) : List Item :=
  ((files.filter (fun f => isTestPath f.1 && !skip.contains f.1)).foldl
    (useWalk symbols) (0, out)).2

/-- The tests a change to the target may break, past those that claim it.

@models REQ-CONTEXT.affected_tests -/
def affected (claims : List Claim) (id : String) (clause : Option String) (down : List String)
    (files : List (String × String)) : List Item :=
  let claimed := claimsOn claims id clause
  let placed := (claims.filter (fun c => c.role == "tests" && onTarget c id clause)).map (·.ident)
  let symbols := (claimed.code.filterMap (·.symbol)).map (fun s => ((s.splitOn "::").getLast?).getD s)
  let state := fromNames claims symbols (fromBelow claims down (placed, []))
  let claimedFiles := (state.2 ++ claimed.tests).map (·.path)
  usesIn symbols claimedFiles files state.2

end TraceLean.Context
