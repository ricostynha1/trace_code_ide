import Lean

namespace TraceLean.Strength

open Lean (ToJson FromJson)

inductive Strength where
  | pinned (theoremName : String)
  | attempted (theoremName : String)
  | «open»
  | nondeterministic (reason : String)
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def insertSorted [BEq α] (le : α → α → Bool) (xs : List α) (x : α) : List α :=
  match xs with
  | [] => [x]
  | y :: rest =>
    if x == y then y :: rest
    else if le x y then x :: y :: rest
    else y :: insertSorted le rest x

def sortedUnique [BEq α] (le : α → α → Bool) (xs : List α) : List α :=
  xs.foldl (insertSorted le) []

end TraceLean.Strength
