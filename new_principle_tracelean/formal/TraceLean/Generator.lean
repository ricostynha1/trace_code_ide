import Lean

namespace TraceLean.Generator

open Lean (Json ToJson FromJson toJson)

structure Rng where
  state : UInt64
  deriving Repr, Inhabited

def Rng.new (seed : UInt64) : Rng :=
  { state := if seed == 0 then 0x9E3779B97F4A7C15 else seed }

def Rng.next (r : Rng) : UInt64 × Rng :=
  let x := r.state
  let x := UInt64.xor x (UInt64.shiftRight x 12)
  let x := UInt64.xor x (UInt64.shiftLeft x 25)
  let x := UInt64.xor x (UInt64.shiftRight x 27)
  (x * 0x2545F4914F6CDD1D, { state := x })

def Rng.below (r : Rng) (n : UInt64) : UInt64 × Rng :=
  let (v, r) := r.next
  (if n == 0 then 0 else v % n, r)

def Rng.chance (r : Rng) (percent : UInt64) : Bool × Rng :=
  let (v, r) := r.below 100
  (v < percent, r)

private def rawStreamGo : Nat → Rng → List Nat
  | 0, _ => []
  | n + 1, r =>
    let (v, r) := r.next
    v.toNat :: rawStreamGo n r

/-- @models REQ-DRT-GEN.fixed_stream
@models ARCH-DETERMINISM.seeded_generation -/
def rawStream (seed : Nat) (count : Nat) : List Nat :=
  rawStreamGo count (Rng.new (UInt64.ofNat seed))

def boundOf (max : Option Nat) : UInt64 :=
  let bound := UInt64.ofNat (max.getD 1000)
  if bound + 1 == 0 then bound else bound + 1

private def natStreamGo (max : Option Nat) (edges : List Nat) : Nat → Rng → List Nat
  | 0, _ => []
  | n + 1, r =>
    let chance := if edges.isEmpty then (false, r) else r.chance 40
    let takeEdge := chance.1
    let r := chance.2
    match takeEdge with
    | true =>
      let drawn := r.below (UInt64.ofNat edges.length)
      edges.getD drawn.1.toNat 0 :: natStreamGo max edges n drawn.2
    | false =>
      let drawn := r.below (boundOf max)
      drawn.1.toNat :: natStreamGo max edges n drawn.2

/-- @models REQ-DRT-GEN.edges_sampled -/
def natStream (seed : Nat) (max : Option Nat) (edges : List Nat) (count : Nat) : List Nat :=
  natStreamGo max edges count (Rng.new (UInt64.ofNat seed))

def letterAt (i : UInt64) : Char := Char.ofNat ('a'.toNat + i.toNat)

private def strLetters : Nat → Rng → List Char × Rng
  | 0, r => ([], r)
  | n + 1, r =>
    let (i, r) := r.below 26
    let (rest, r) := strLetters n r
    (letterAt i :: rest, r)

inductive Schema where
  | nat (max : Option Nat) (edges : List Nat)
  | int (min max : Option Int)
  | bool
  | str (maxLen : Option Nat) (examples : List String)
  | option (inner : Schema)
  | list (inner : Schema) (maxLen : Option Nat)
  | struct (fields : List (String × Schema))
  | tuple (items : List Schema)
  | enum (variants : List (String × Option Schema))
  deriving Inhabited

def optField (α : Type) [FromJson α] (j : Json) (key : String) : Option α :=
  match j.getObjValAs? α key with
  | .ok v => some v
  | .error _ => none

def members (j : Json) : List (String × Json) :=
  match j with
  | .obj kvs => kvs.fold (fun acc k v => acc ++ [(k, v)]) []
  | _ => []

def allOk (xs : List (Except String α)) : Except String (List α) :=
  xs.foldr (fun x acc =>
    match x, acc with
    | .ok v, .ok vs => .ok (v :: vs)
    | .error e, _ => .error e
    | _, .error e => .error e) (.ok [])

partial def Schema.decode (j : Json) : Except String Schema :=
  let child := fun key =>
    match j.getObjVal? key with
    | .ok inner => Schema.decode inner
    | .error e => .error e
  match j.getObjValAs? String "type" with
  | .error e => .error e
  | .ok "nat" => .ok (.nat (optField Nat j "max") ((optField (List Nat) j "edges").getD []))
  | .ok "int" => .ok (.int (optField Int j "min") (optField Int j "max"))
  | .ok "bool" => .ok .bool
  | .ok "str" =>
    .ok (.str (optField Nat j "max_len") ((optField (List String) j "examples").getD []))
  | .ok "option" =>
    match child "inner" with
    | .ok inner => .ok (.option inner)
    | .error e => .error e
  | .ok "list" =>
    match child "inner" with
    | .ok inner => .ok (.list inner (optField Nat j "max_len"))
    | .error e => .error e
  | .ok "struct" =>
    let fields := (members ((j.getObjVal? "fields").toOption.getD Json.null)).map
      (fun f => (Schema.decode f.2).map (fun s => (f.1, s)))
    (allOk fields).map Schema.struct
  | .ok "tuple" =>
    let items := ((optField (List Json) j "items").getD []).map Schema.decode
    (allOk items).map Schema.tuple
  | .ok "enum" =>
    let variants := (members ((j.getObjVal? "variants").toOption.getD Json.null)).map
      (fun v =>
        match v.2 with
        | .null => .ok (v.1, none)
        | payload => (Schema.decode payload).map (fun s => (v.1, some s)))
    (allOk variants).map Schema.enum
  | .ok other => .error ("unknown variant `" ++ other ++ "`")

instance : FromJson Schema where
  fromJson? := Schema.decode

def spanOf (lo hi : Int) : UInt64 :=
  let span := UInt64.ofNat (hi - lo).natAbs
  if span + 1 == 0 then span else span + 1

partial def valueOf (schema : Schema) (r : Rng) : Json × Rng :=
  match schema with
  | .nat max edges =>
    let chance := if edges.isEmpty then (false, r) else r.chance 40
    match chance.1 with
    | true =>
      let drawn := chance.2.below (UInt64.ofNat edges.length)
      (toJson (edges.getD drawn.1.toNat 0), drawn.2)
    | false =>
      let drawn := chance.2.below (boundOf max)
      (toJson drawn.1.toNat, drawn.2)
  | .int min max =>
    let lo := min.getD (-1000)
    let drawn := r.below (spanOf lo (max.getD 1000))
    (toJson (lo + Int.ofNat drawn.1.toNat), drawn.2)
  | .bool =>
    let drawn := r.chance 50
    (Json.bool drawn.1, drawn.2)
  | .str maxLen examples =>
    let chance := if examples.isEmpty then (false, r) else r.chance 50
    match chance.1 with
    | true =>
      let drawn := chance.2.below (UInt64.ofNat examples.length)
      (Json.str (examples.getD drawn.1.toNat ""), drawn.2)
    | false =>
      let drawn := chance.2.below (UInt64.ofNat ((maxLen.getD 12) + 1))
      let word := strLetters drawn.1.toNat drawn.2
      (Json.str (String.mk word.1), word.2)
  | .option inner =>
    let drawn := r.chance 25
    match drawn.1 with
    | true => (Json.null, drawn.2)
    | false => valueOf inner drawn.2
  | .list inner maxLen =>
    let drawn := r.below (UInt64.ofNat ((maxLen.getD 6) + 1))
    let items := (List.range drawn.1.toNat).foldl (fun acc _ =>
      let v := valueOf inner acc.2
      (acc.1 ++ [v.1], v.2)) (([] : List Json), drawn.2)
    (Json.arr items.1.toArray, items.2)
  | .struct fields =>
    let done := fields.foldl (fun acc f =>
      let v := valueOf f.2 acc.2
      (acc.1 ++ [(f.1, v.1)], v.2)) (([] : List (String × Json)), r)
    (Json.mkObj done.1, done.2)
  | .tuple items =>
    let done := items.foldl (fun acc s =>
      let v := valueOf s acc.2
      (acc.1 ++ [v.1], v.2)) (([] : List Json), r)
    (Json.arr done.1.toArray, done.2)
  | .enum variants =>
    match variants with
    | [] => (Json.null, r)
    | _ =>
      let drawn := r.below (UInt64.ofNat variants.length)
      let pick := variants.getD drawn.1.toNat ("", none)
      match pick.2 with
      | none => (Json.str pick.1, drawn.2)
      | some payload =>
        let v := valueOf payload drawn.2
        (Json.mkObj [(pick.1, v.1)], v.2)

def valueStream (seed : Nat) (schema : Schema) (count : Nat) : List Json :=
  ((List.range count).foldl (fun acc _ =>
    let v := valueOf schema acc.2
    (acc.1 ++ [v.1], v.2)) (([] : List Json), Rng.new (UInt64.ofNat seed))).1

def shapes : List Schema :=
  [ Schema.nat (some 50) [7],
    Schema.int (some (-5)) (some 5),
    Schema.bool,
    Schema.str (some 4) ["one", ""],
    Schema.option (Schema.nat (some 9) []),
    Schema.list (Schema.int (some 0) (some 3)) (some 3),
    Schema.tuple [Schema.bool, Schema.nat (some 3) []],
    Schema.struct [("a", Schema.bool), ("b", Schema.str (some 2) [])],
    Schema.enum [("x", none), ("y", some (Schema.nat (some 2) []))],
    Schema.list (Schema.struct [("n", Schema.option (Schema.int (some (-1)) (some 1)))]) (some 2) ]

/-- @models REQ-DRT-GEN.seed_reproduces -/
def shapeStream (seed : Nat) (shape : Nat) (count : Nat) : List Json :=
  match shapes.length with
  | 0 => []
  | n + 1 => valueStream seed (shapes.getD (shape % (n + 1)) (.bool)) count

/-- @proves REQ-DRT-GEN.seed_reproduces -/
theorem a_nat_schema_draws_the_nat_stream :
    (valueStream 7 (.nat (some 50) [7]) 20).map Json.compress
      = (natStream 7 (some 50) [7] 20).map (fun n => (toJson n).compress) := by
  native_decide

/-- @proves REQ-DRT-GEN.fixed_stream -/
theorem a_zero_seed_does_not_stick :
    rawStream 0 3 ≠ [0, 0, 0] := by
  native_decide

theorem raw_stream_go_prefix (n m : Nat) :
    ∀ r : Rng, (rawStreamGo (n + m) r).take n = rawStreamGo n r := by
  induction n
  case zero =>
    intro r
    simp [rawStreamGo]
  case succ k ih =>
    intro r
    rw [Nat.succ_add]
    simp [rawStreamGo, ih]

/-- @proves ARCH-DETERMINISM.seeded_generation -/
theorem the_seed_alone_fixes_the_cases (seed n m : Nat) :
    (rawStream seed (n + m)).take n = rawStream seed n ∧ rawStream 1 4 ≠ rawStream 2 4 := by
  constructor
  · exact raw_stream_go_prefix n m _
  · native_decide

/-- @proves REQ-DRT-GEN.edges_sampled -/
theorem edges_are_drawn_heavily :
    ((natStream 1 (some 1000) [7] 200).filter (· == 7)).length ≥ 40 := by
  native_decide

end TraceLean.Generator
