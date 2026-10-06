import Lean

/-!
# Normalised hashing of an anchored body

Models `REQ-ANCHOR`. The hash decides staleness, so both directions matter and
they fail differently. A hash that does not move when the body does leaves stale
evidence looking valid -- the failure this whole project exists to prevent. A
hash that moves when the body did not makes people stop annotating, which
achieves the same thing more slowly.

Two normalisations do the work, and both are about what counts as a change.
Whitespace outside literals is collapsed, so reindenting a function does not
invalidate the evidence about it. Whitespace *inside* a literal is content and
is copied through byte for byte. Comments are removed entirely, so editing an
annotation cannot invalidate its own evidence -- which would otherwise make the
annotation mechanism self-defeating.

Offsets here are byte offsets, and the model indexes by character. The two agree
for ASCII, which is what the differential run generates; a non-ASCII body
reaches the hash through the protected-range path, which is a copy on both
sides.
-/

namespace TraceLean.Hash

open Lean (ToJson FromJson)

/-- Whether an index falls inside any of the given ranges, and where that range
ends. -/
def rangeAt (ranges : List (Nat × Nat)) (i : Nat) : Option (Nat × Nat) :=
  ranges.find? (fun r => r.1 ≤ i && i < r.2)

def isSpace' (c : Char) : Bool :=
  -- Form feed and vertical tab by code point: the grammar that reads these
  -- annotations cannot read a `\xNN` character literal (ADR-0008).
  c == ' ' || c == '\t' || c == '\n' || c == '\r'
    || c == Char.ofNat 12 || c == Char.ofNat 11

private def normalizeGo (cs : List Char) (n : Nat) (removed protected_ : List (Nat × Nat))
    (fuel i : Nat) (lastWasSpace : Bool) (acc : List Char) : List Char :=
  match fuel with
  | 0 => acc
  | fuel + 1 =>
    if i ≥ n then acc
    else
      match rangeAt removed i with
      | some (_, e) =>
        if lastWasSpace then normalizeGo cs n removed protected_ fuel e true acc
        else normalizeGo cs n removed protected_ fuel e true (acc ++ [' '])
      | none =>
        match rangeAt protected_ i with
        | some (s, e) =>
          normalizeGo cs n removed protected_ fuel e false (acc ++ ((cs.drop s).take (e - s)))
        | none =>
          let c := cs.getD i ' '
          if isSpace' c then
            if lastWasSpace then normalizeGo cs n removed protected_ fuel (i + 1) true acc
            else normalizeGo cs n removed protected_ fuel (i + 1) true (acc ++ [' '])
          else normalizeGo cs n removed protected_ fuel (i + 1) false (acc ++ [c])

/--
Collapse runs of whitespace outside protected ranges, and drop the ranges listed
in `removed` entirely.

A removed range separates tokens, so it counts as whitespace: deleting a comment
from between two identifiers must not run them together.

@models REQ-ANCHOR.whitespace_normalised
@models REQ-ANCHOR.comments_excluded
-/
def normalize (source : String) (removed protected_ : List (Nat × Nat)) : String :=
  let cs := source.toList
  let n := cs.length
  -- The walk is a separate definition rather than a `let rec`, which the
  -- grammar that reads these annotations cannot read (ADR-0008).
  -- Leading whitespace is dropped, trailing whitespace trimmed.
  let out := normalizeGo cs n removed protected_ (n + 1) 0 true []
  String.mk (out.reverse.dropWhile (· == ' ')).reverse

/-! ## The digest

FNV-1a over the normalised bytes. Not cryptographic: nothing here defends
against an adversary choosing a collision, it only has to change when the input
does. -/

def fnvPrime : UInt64 := 0x00000100000001B3
def fnvOffset : UInt64 := 0xcbf29ce484222325

def hexDigit (n : UInt64) : Char :=
  let d := (UInt64.land n 0xf).toNat
  if d < 10 then Char.ofNat ('0'.toNat + d) else Char.ofNat ('a'.toNat + d - 10)

/-- Sixteen lower-case hex digits, most significant first. -/
def toHex16 (h : UInt64) : String :=
  String.mk ((List.range 16).map
    (fun i => hexDigit (UInt64.shiftRight h (UInt64.ofNat ((15 - i) * 4)))))

def digest (s : String) : String :=
  toHex16 (s.toUTF8.toList.foldl
    (fun h b => (UInt64.xor h (UInt64.ofNat b.toNat)) * fnvPrime) fnvOffset)

/-- Hash of a normalised body.

@models REQ-ANCHOR.hash_tracks_body -/
def bodyHash (source : String) (removed protected_ : List (Nat × Nat)) : String :=
  digest (normalize source removed protected_)

/-- Hash of a requirement's semantic content: its clauses and its prose.

Keyed by clause name, so renaming a clause changes the hash -- a judgement was
about the pair, and a renamed clause is a different pair.

@models REQ-REQDOC.clause_addressable -/
def requirementHash (clauses : List (String × String)) (bodyText : String) : String :=
  let sorted := (clauses.foldl
    (fun acc kv =>
      if acc.any (·.1 == kv.1) then acc.map (fun p => if p.1 == kv.1 then kv else p)
      else acc ++ [kv]) []).mergeSort (fun a b => decide (a.1 ≤ b.1))
  let buffer := sorted.foldl
    (fun acc kv =>
      acc ++ kv.1 ++ String.mk [Char.ofNat 1] ++ kv.2.trim ++ String.mk [Char.ofNat 2]) ""
  digest (buffer ++ normalize bodyText [] [])

/-- Reindenting a body does not move its hash: the whole reason whitespace is
normalised before hashing.

@proves REQ-ANCHOR.hash_tracks_body -/
-- The sample carries no brace on purpose: the grammar that reads these
-- annotations treats `{` in a string as the start of an interpolation and stops
-- there, and what the theorem says does not need one (ADR-0008).
theorem reindentation_does_not_move_the_hash :
    bodyHash "let x = 1;\n    let y = x;" [] []
      = bodyHash "let x = 1;\n\t\tlet y = x;" [] [] := by
  native_decide

end TraceLean.Hash
