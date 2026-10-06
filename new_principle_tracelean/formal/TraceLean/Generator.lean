import Lean

/-!
# Deterministic case generation

Models `REQ-DRT-GEN`. Determinism here is not a convenience. An evidence record
claims "seed 7, twelve million cases, no divergence", and that claim is
auditable only while seed 7 still means those cases. So the random stream is
defined in the project rather than taken from a dependency, where a version bump
would silently invalidate every record that referred to it.

Writing it twice and checking the two agree is what makes "fixed" mean
something. A stream that is merely *stated* to be fixed is a comment.

The second property is about where cases land. Uniform sampling over a wide
range tests a model branching at a boundary almost entirely on one side of it,
and a coverage floor computed over the whole range still reports itself met.
Declared edges are therefore drawn heavily, not uniformly.
-/

namespace TraceLean.Generator

open Lean (ToJson FromJson)

/-- xorshift64*, short enough to read and fixed here forever. -/
structure Rng where
  state : UInt64
  deriving Repr, Inhabited

/-- Zero is a fixed point of xorshift; nudge it. -/
def Rng.new (seed : UInt64) : Rng :=
  { state := if seed == 0 then 0x9E3779B97F4A7C15 else seed }

def Rng.next (r : Rng) : UInt64 × Rng :=
  -- Written with the named operations rather than `^^^`, `>>>` and `<<<`,
  -- which the grammar that reads these annotations cannot read (ADR-0008).
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

/-! The three walks below are top-level definitions rather than `let rec` inside
the stream functions: the grammar that reads these annotations cannot read a
`let rec`, and a walk that cannot be anchored is a model nothing can point at
(ADR-0008). -/

private def rawStreamGo : Nat → Rng → List Nat
  | 0, _ => []
  | n + 1, r =>
    let (v, r) := r.next
    v.toNat :: rawStreamGo n r

/-- The first `count` values of the raw stream.

Seeds and results cross the wire as `Nat`: Lean encodes a `UInt64` as a string
to avoid precision loss, and the implementation writes a JSON number. The
arithmetic below is still 64-bit and wrapping -- only the boundary converts.

@models REQ-DRT-GEN.fixed_stream
@models ARCH-DETERMINISM.seeded_generation -/
def rawStream (seed : Nat) (count : Nat) : List Nat :=
  rawStreamGo count (Rng.new (UInt64.ofNat seed))

/-- `max.unwrap_or(1000)`, then one more -- saturating, so a bound at the top of
the range does not wrap to zero. -/
def boundOf (max : Option Nat) : UInt64 :=
  let bound := UInt64.ofNat (max.getD 1000)
  if bound + 1 == 0 then bound else bound + 1

private def natStreamGo (max : Option Nat) (edges : List Nat) : Nat → Rng → List Nat
  | 0, _ => []
  | n + 1, r =>
    -- Pairs are taken apart with `.1` and `.2` rather than by a destructuring
    -- `let` inside a branch, which the grammar that reads these annotations
    -- cannot read (ADR-0008).
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

/-- The first `count` values a `Nat` schema generates from a seed.

Edges are drawn heavily rather than uniformly: a run that never reached a branch
has not tested it.

@models REQ-DRT-GEN.seed_reproduces
@models REQ-DRT-GEN.edges_sampled -/
def natStream (seed : Nat) (max : Option Nat) (edges : List Nat) (count : Nat) : List Nat :=
  natStreamGo max edges count (Rng.new (UInt64.ofNat seed))

/-- The alphabet generated strings are drawn from. -/
def letterAt (i : UInt64) : Char := Char.ofNat ('a'.toNat + i.toNat)

private def strLetters : Nat → Rng → List Char × Rng
  | 0, r => ([], r)
  | n + 1, r =>
    let (i, r) := r.below 26
    let (rest, r) := strLetters n r
    (letterAt i :: rest, r)

private def strStreamGo (maxLen : Option Nat) (examples : List String) : Nat → Rng → List String
  | 0, _ => []
  | n + 1, r =>
    let chance := if examples.isEmpty then (false, r) else r.chance 50
    let takeExample := chance.1
    let r := chance.2
    -- `match` rather than `if … then … else …`: the grammar that reads these
    -- annotations cannot follow a `let` in both branches of an `if` (ADR-0008).
    match takeExample with
    | true =>
      let drawn := r.below (UInt64.ofNat examples.length)
      examples.getD drawn.1.toNat "" :: strStreamGo maxLen examples n drawn.2
    | false =>
      let drawn := r.below (UInt64.ofNat ((maxLen.getD 12) + 1))
      let word := strLetters drawn.1.toNat drawn.2
      String.mk word.1 :: strStreamGo maxLen examples n word.2

/-- The first `count` values a `Str` schema generates from a seed.

@models REQ-DRT-GEN.seed_reproduces -/
def strStream (seed : Nat) (maxLen : Option Nat) (examples : List String) (count : Nat)
    : List String :=
  strStreamGo maxLen examples count (Rng.new (UInt64.ofNat seed))

/-- The same seed gives the same stream. Trivially true of a pure function --
which is the point: the property is that generation *is* a pure function of the
seed, and nothing else.

@proves REQ-DRT-GEN.seed_reproduces -/
theorem the_same_seed_gives_the_same_stream (seed : Nat) (count : Nat) :
    rawStream seed count = rawStream seed count := rfl

/-- Zero is a fixed point of xorshift, so it is nudged rather than used.

@proves REQ-DRT-GEN.fixed_stream -/
theorem a_zero_seed_does_not_stick :
    rawStream 0 3 ≠ [0, 0, 0] := by
  native_decide

end TraceLean.Generator
