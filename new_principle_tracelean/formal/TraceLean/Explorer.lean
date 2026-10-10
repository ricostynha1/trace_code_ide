import Lean

/-!
# The project's files as a tree

Models `REQ-SHOW.listing_is_a_tree`: the rows a set of files makes, folders
before files at every level and each group in name order, an open folder
followed by what it holds and a closed one by nothing; each row shows a name
and stands for its whole path.
-/

namespace TraceLean.Explorer

open Lean (ToJson FromJson)

structure Row where
  depth : Nat
  name : String
  path : String
  folder : Bool
  isOpen : Bool
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- `s` added to a sorted list without repeats, keeping it so. -/
def insertSorted (s : String) : List String → List String
  | [] => [s]
  | x :: rest => if s == x then x :: rest else if s < x then s :: x :: rest else x :: insertSorted s rest

/-- The folders and the files directly under `pre`, each sorted and without
repeats: a folder is what comes before the first `/` after the prefix. -/
def split (files : List String) (pre : String) : List String × List String :=
  files.foldl (fun (acc : List String × List String) (file : String) =>
    if file.startsWith pre then
      match (file.drop pre.length).splitOn "/" with
      | folder :: _ :: _ => (insertSorted folder acc.1, acc.2)
      | _ => (acc.1, insertSorted (file.drop pre.length) acc.2)
    else acc) ([], [])

/-- The rows under `pre`; every level's prefix is longer than the last, so
there are no more levels than characters in the longest path. -/
def level (files opened : List String) : Nat → String → Nat → List Row
  | 0, _, _ => []
  | fuel + 1, pre, depth =>
    let parts := split files pre
    let folders := parts.1.bind fun folder =>
      let path := pre ++ folder
      let isOpen := opened.contains path
      { depth := depth, name := folder, path := path, folder := true, isOpen := isOpen } ::
        (if isOpen then level files opened fuel (path ++ "/") (depth + 1) else [])
    let leaves := parts.2.map fun leaf =>
      ({ depth := depth, name := leaf, path := pre ++ leaf, folder := false, isOpen := false } : Row)
    folders ++ leaves

/-- The rows a set of files makes with the folders in `opened` open.

@models REQ-SHOW.listing_is_a_tree -/
def rows (files opened : List String) : List Row :=
  level files opened (files.foldl (fun m f => max m f.length) 0 + 2) "" 0

end TraceLean.Explorer
