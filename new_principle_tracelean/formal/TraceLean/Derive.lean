import Lean

/-!
# Deriving a schema from two signatures

Models `REQ-DRT-SCHEMA.derived_from_both`. Each argument's Lean type is paired
with the Rust type in the same position; a pair both sides plainly agree on
gives the generator its shape, and anything else is a named mismatch rather
than a guessed generator.
-/

namespace TraceLean.Derive

open Lean (Json ToJson FromJson toJson)

/-- A type, as far as pairing needs to know it. -/
inductive Ty where
  | int (bits : Nat)
  | nat (bits : Nat)
  | bool
  | str
  | float
  | list (inner : Ty)
  | option (inner : Ty)
  | named (name : String)
  | other (text : String)
  deriving Repr, Inhabited, ToJson, FromJson

/-- The generator shapes a pairing produces, encoded as the binding file
spells a schema. -/
inductive Schema where
  | nat (max : Nat) (edges : List Nat)
  | int (min max : Int)
  | bool
  | str (maxLen : Nat) (examples : List String)
  | option (inner : Schema)
  | list (inner : Schema) (maxLen : Nat)
  | struct (fields : List (String × Schema))
  deriving Inhabited

partial def Schema.encode : Schema → Json
  | .nat max edges => Json.mkObj [("type", "nat"), ("max", toJson max), ("edges", toJson edges)]
  | .int min max => Json.mkObj [("type", "int"), ("min", toJson min), ("max", toJson max)]
  | .bool => Json.mkObj [("type", "bool")]
  | .str maxLen examples =>
    Json.mkObj [("type", "str"), ("max_len", toJson maxLen), ("examples", toJson examples)]
  | .option inner => Json.mkObj [("type", "option"), ("inner", inner.encode)]
  | .list inner maxLen =>
    Json.mkObj [("type", "list"), ("inner", inner.encode), ("max_len", toJson maxLen)]
  | .struct fields =>
    Json.mkObj [("type", "struct"), ("fields", Json.mkObj (fields.map (fun f => (f.1, f.2.encode))))]

/-- The schema a pair gives, or why there is none. -/
inductive Paired where
  | ok (schema : Schema)
  | err (why : String)
  deriving Inhabited

instance : ToJson Paired where
  toJson
    | .ok schema => Json.mkObj [("Ok", schema.encode)]
    | .err why => Json.mkObj [("Err", toJson why)]

/-- A structure's fields as JSON spells them, in order. -/
abbrev Structs := List (String × List (String × Ty))

/-- How a type reads in a report. -/
def spelled : Ty → String
  | .int 0 => "Int"
  | .int bits => "i" ++ toString bits
  | .nat 0 => "Nat"
  | .nat bits => "u" ++ toString bits
  | .bool => "Bool"
  | .str => "String"
  | .float => "Float"
  | .list inner => "List (" ++ spelled inner ++ ")"
  | .option inner => "Option (" ++ spelled inner ++ ")"
  | .named name => name
  | .other text => text

def intBound (bits : Nat) : Int := if bits == 8 then 10 else 100

def natBound (bits : Nat) : Nat := if bits == 8 then 15 else 1000

def isFloat : Ty → Bool
  | .float => true
  | _ => false

/-- The fields of a structure, by name. -/
def fieldsOf (structs : Structs) (name : String) : Option (List (String × Ty)) :=
  (structs.find? (fun s => s.1 == name)).map (fun s => s.2)

/-- The schema for a Lean type against a Rust type. `fuel` bounds the walk
through structures, which may refer to each other.

@models REQ-DRT-SCHEMA.derived_from_both -/
def pair (lean rust : Ty) (leanStructs rustStructs : Structs) (fuel : Nat) : Paired :=
  match fuel with
  | 0 => .err ("`" ++ spelled lean ++ "` nests too deeply to generate")
  | n + 1 =>
    let mismatch := Paired.err ("Lean `" ++ spelled lean ++ "` against Rust `" ++ spelled rust ++ "`")
    if isFloat lean || isFloat rust then
      .err "Float has no exact comparison; test it with a binding of your own"
    else
    match lean, rust with
    | .int 0, .int bits => .ok (.int (-(intBound bits)) (intBound bits))
    | .nat 0, .nat bits => .ok (.nat (natBound bits) [0, 1])
    | .nat 0, .int bits => .ok (.nat (natBound bits) [0, 1])
    | .bool, .bool => .ok .bool
    | .str, .str => .ok (.str 8 [""])
    | .list a, .list b =>
      match pair a b leanStructs rustStructs n with
      | .ok inner => .ok (.list inner 5)
      | e => e
    | .option a, .option b =>
      match pair a b leanStructs rustStructs n with
      | .ok inner => .ok (.option inner)
      | e => e
    | .named a, .named b =>
      match fieldsOf leanStructs a, fieldsOf rustStructs b with
      | some theirs, some ours =>
        if theirs.length != ours.length then
          .err ("`" ++ a ++ "` has " ++ toString theirs.length ++ " fields and `" ++ b ++ "` "
            ++ toString ours.length)
        else
          let step := fun (acc : Except String (List (String × Schema))) (f : String × Ty) =>
            match acc with
            | .error e => .error e
            | .ok done =>
              match ours.find? (fun o => o.1 == f.1) with
              | none => .error ("`" ++ b ++ "` has no field `" ++ f.1 ++ "` as JSON spells it")
              | some o =>
                match pair f.2 o.2 leanStructs rustStructs n with
                | .ok s => .ok (done ++ [(f.1, s)])
                | .err why => .error (a ++ "." ++ f.1 ++ ": " ++ why)
          match theirs.foldl step (.ok []) with
          | .ok fields => .ok (.struct fields)
          | .error why => .err why
      | _, _ => .err ("no structure `" ++ a ++ "` in the model, or `" ++ b ++ "` in the code, to pair")
    | _, _ => mismatch

/-- Lean's unbounded integers against any Rust integer give a bounded range
around zero.

@proves REQ-DRT-SCHEMA.derived_from_both -/
theorem an_int_pairs_with_i64 :
    (match pair (.int 0) (.int 64) [] [] 1 with
     | .ok (.int lo hi) => lo == -100 && hi == 100
     | _ => false) = true := by
  rfl

/-- A float on either side is refused, never approximated.

@proves REQ-DRT-SCHEMA.outside_is_error -/
theorem a_float_is_refused (t : Ty) (s r : Structs) (n : Nat) :
    (match pair .float t s r (n + 1) with
     | .err _ => true
     | .ok _ => false) = true := by
  simp [pair, isFloat]

end TraceLean.Derive
