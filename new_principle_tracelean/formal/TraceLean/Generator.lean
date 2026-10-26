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
    (rawStream seed (n + m)).take n = rawStream seed n :=
  raw_stream_go_prefix n m _

def stateAfter : Nat → Rng → Rng
  | 0, r => r
  | n + 1, r => stateAfter n r.next.2

theorem raw_stream_go_orbit (n : Nat) :
    ∀ r : Rng, rawStreamGo n r = (List.range n).map (fun i => (stateAfter i r).next.1.toNat) := by
  induction n with
  | zero => intro _; simp [rawStreamGo]
  | succ k ih =>
    intro r
    rw [List.range_succ_eq_map]
    simp [rawStreamGo, ih, stateAfter]

/-- @proves REQ-DRT-GEN.fixed_stream -/
theorem the_stream_is_the_xorshift_orbit_of_the_seed (seed count : Nat) :
    rawStream seed count =
      (List.range count).map (fun i =>
        (stateAfter i (Rng.new (UInt64.ofNat seed))).next.1.toNat) :=
  raw_stream_go_orbit count _

def natAt (max : Option Nat) (edges : List Nat) (r : Rng) : Nat :=
  match (r.chance 40).1 with
  | true => edges.getD ((r.chance 40).2.below (UInt64.ofNat edges.length)).1.toNat 0
  | false => ((r.chance 40).2.below (boundOf max)).1.toNat

theorem state_after_add (n : Nat) : ∀ (b : Nat) (r : Rng),
    stateAfter (n + b) r = stateAfter n (stateAfter b r) := by
  intro b
  induction b with
  | zero => intro r; rfl
  | succ b ih =>
    intro r
    show stateAfter (n + b + 1) r = stateAfter n (stateAfter b r.next.2)
    rw [← ih]
    rfl

theorem nat_stream_go_head (max : Option Nat) (edges : List Nat) (notEmpty : edges.isEmpty = false)
    (k : Nat) (r : Rng) :
    natStreamGo max edges (k + 1) r = natAt max edges r :: natStreamGo max edges k (stateAfter 2 r) := by
  simp only [natStreamGo, notEmpty, Bool.false_eq_true, if_false]
  cases h : (r.chance 40).1 <;> simp only [natAt, h] <;> rfl

theorem nat_stream_go_orbit (max : Option Nat) (edges : List Nat) (nonempty : edges ≠ [])
    (n : Nat) : ∀ r : Rng,
    natStreamGo max edges n r = (List.range n).map (fun i => natAt max edges (stateAfter (2 * i) r)) := by
  have notEmpty : edges.isEmpty = false := by
    cases edges with
    | nil => exact absurd rfl nonempty
    | cons _ _ => rfl
  induction n with
  | zero => intro _; simp [natStreamGo]
  | succ k ih =>
    intro r
    rw [List.range_succ_eq_map, nat_stream_go_head max edges notEmpty, ih]
    simp only [List.map_cons, List.map_map, Nat.mul_zero, List.cons.injEq]
    refine ⟨rfl, ?_⟩
    apply List.map_congr_left
    intro i _
    simp only [Function.comp, Nat.succ_eq_add_one, Nat.mul_add, Nat.mul_one, state_after_add]

theorem drawn_edge_is_an_edge (edges : List Nat) (nonempty : edges ≠ []) (r : Rng) :
    edges.getD (r.below (UInt64.ofNat edges.length)).1.toNat 0 ∈ edges := by
  have pos : 0 < edges.length := by
    cases edges with
    | nil => exact absurd rfl nonempty
    | cons _ _ => simp
  have small : (UInt64.ofNat edges.length).toNat ≤ edges.length := Nat.mod_le _ _
  have lt : (r.below (UInt64.ofNat edges.length)).1.toNat < edges.length := by
    simp only [Rng.below]
    split
    · simpa using pos
    · rename_i nz
      have nz' : (UInt64.ofNat edges.length).toNat ≠ 0 := by
        intro z
        apply nz
        have : UInt64.ofNat edges.length = 0 := UInt64.toNat.inj (by simpa using z)
        simp [this]
      rw [UInt64.toNat_mod]
      exact Nat.lt_of_lt_of_le (Nat.mod_lt _ (Nat.pos_of_ne_zero nz')) small
  rw [List.getD_eq_getElem?_getD, List.getElem?_eq_getElem lt]
  exact List.getElem_mem _ _ _

/-- @proves REQ-DRT-GEN.edges_sampled -/
theorem a_boundary_is_drawn_whenever_the_coin_says_so (seed : Nat) (max : Option Nat)
    (edges : List Nat) (count i : Nat) (nonempty : edges ≠ []) (inRange : i < count)
    (coin : ((stateAfter (2 * i) (Rng.new (UInt64.ofNat seed))).chance 40).1 = true) :
    ∃ e ∈ edges, (natStream seed max edges count).get? i = some e := by
  unfold natStream
  rw [nat_stream_go_orbit max edges nonempty, List.get?_eq_getElem?, List.getElem?_map,
    List.getElem?_range inRange]
  refine ⟨_, ?_, rfl⟩
  simp only [natAt, coin]
  exact drawn_edge_is_an_edge edges nonempty _

def drawEach (f : Rng → Json × Rng) (acc : List Json × Rng) (_ : Nat) : List Json × Rng :=
  (acc.1 ++ [(f acc.2).1], (f acc.2).2)

theorem draw_each_extends (f : Rng → Json × Rng) (l : List Nat) :
    ∀ acc : List Json × Rng, ∃ t, (l.foldl (drawEach f) acc).1 = acc.1 ++ t := by
  induction l with
  | nil => intro acc; exact ⟨[], by simp⟩
  | cons x xs ih =>
    intro acc
    obtain ⟨t, ht⟩ := ih (drawEach f acc x)
    refine ⟨(f acc.2).1 :: t, ?_⟩
    simp only [List.foldl]
    rw [ht]
    simp [drawEach]

theorem draw_each_length (f : Rng → Json × Rng) (l : List Nat) :
    ∀ acc : List Json × Rng, (l.foldl (drawEach f) acc).1.length = acc.1.length + l.length := by
  induction l with
  | nil => intro acc; simp
  | cons x xs ih =>
    intro acc
    simp only [List.foldl, ih, drawEach, List.length_append, List.length_cons, List.length_nil]
    omega

theorem draw_each_ignores (f : Rng → Json × Rng) (l₁ l₂ : List Nat) (same : l₁.length = l₂.length) :
    ∀ acc : List Json × Rng, l₁.foldl (drawEach f) acc = l₂.foldl (drawEach f) acc := by
  induction l₁ generalizing l₂ with
  | nil => cases l₂ with
    | nil => intro acc; rfl
    | cons _ _ => simp at same
  | cons x xs ih => cases l₂ with
    | nil => simp at same
    | cons y ys =>
      intro acc
      simp only [List.foldl]
      exact ih ys (by simpa using same) _

theorem value_stream_prefix (seed : Nat) (schema : Schema) (n m : Nat) :
    (valueStream seed schema (n + m)).take n = valueStream seed schema n := by
  show ((List.range (n + m)).foldl (drawEach (valueOf schema)) ([], _)).1.take n =
    ((List.range n).foldl (drawEach (valueOf schema)) ([], _)).1
  rw [draw_each_ignores (valueOf schema) (List.range (n + m)) (List.range n ++ List.range m)
    (by simp), List.foldl_append]
  obtain ⟨t, ht⟩ := draw_each_extends (valueOf schema) (List.range m)
    ((List.range n).foldl (drawEach (valueOf schema)) ([], Rng.new (UInt64.ofNat seed)))
  rw [ht]
  apply List.take_left'
  simp [draw_each_length]

/-- @proves REQ-DRT-GEN.seed_reproduces -/
theorem a_seed_fixes_every_case_of_a_shape (seed shape n m : Nat) :
    (shapeStream seed shape (n + m)).take n = shapeStream seed shape n ∧
    (shapeStream seed shape n).length = n := by
  unfold shapeStream
  split
  · rename_i h
    simp [shapes] at h
  · refine ⟨value_stream_prefix _ _ n m, ?_⟩
    show ((List.range n).foldl (drawEach (valueOf _)) ([], _)).1.length = n
    simp [draw_each_length]

end TraceLean.Generator
