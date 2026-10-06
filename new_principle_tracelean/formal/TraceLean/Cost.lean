import Lean
import TraceLean.Transcript

/-!
# What a sandboxed agent's usage is estimated to have cost

Models `REQ-COST`. Usage records the tool wrote about itself, priced against a
table this project keeps by hand. That is all it is, and the requirement is
mostly about saying so.

Arithmetic over naturals: a price is per million tokens, an amount is whole
millionths of a unit of currency. A floating-point total would make two
implementations disagree in the last digit for reasons that have nothing to do
with pricing.

Written in the subset of Lean the annotation grammar reads (ADR-0008).
-/

namespace TraceLean.Cost

open Lean (ToJson FromJson)

open TraceLean.Transcript (Usage)

/-- What a million tokens of each kind costs, in millionths of a unit.

Four rates and not one. Cached input is the cheapest thing on the bill and a
cache write the dearest, often by an order of magnitude in each direction, so a
total that added them at one rate would be wrong by more than rounding -- and
wrong in whichever direction the workload happened to lean. -/
structure Price where
  model : String
  input : Nat
  cached : Nat
  cacheWrite : Nat
  output : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- An estimate: what the priced usage came to, and what could not be priced. -/
structure Spend where
  amount : Nat
  /-- The models the table did not price, named once each, in the order met. -/
  unpriced : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

/-- The price for a model, if the table has one.

@models REQ-COST.price_is_per_model -/
def priceOf : List Price → String → Option Price
  | [], _ => none
  | price :: rest, model => if price.model == model then some price else priceOf rest model

/-- What one usage record costs, at one price.

Integer division, truncating. The unit is a millionth, so the discarded part is
less than a millionth of a unit per line -- below anything this figure claims to
resolve, and deterministic, which the alternative is not.

@models REQ-COST.cache_priced_apart -/
def amountOf (price : Price) (usage : Usage) : Nat :=
  (usage.input * price.input
    + usage.cached * price.cached
    + usage.cacheWrite * price.cacheWrite
    + usage.output * price.output) / 1000000

private def noting (seen : List String) (model : String) : List String :=
  if seen.contains model then seen else seen ++ [model]

private def accumulate (table : List Price) : Spend → Usage → Spend :=
  fun spend usage =>
    match priceOf table usage.model with
    | some price => { spend with amount := spend.amount + amountOf price usage }
    | none => { spend with unpriced := noting spend.unpriced usage.model }

/--
What a run of usage records is estimated to have cost.

A model the table does not price is named rather than counted as free. A total
that quietly priced an unknown model at zero would be confidently wrong and look
right, which is the failure `ARCH-HONEST` exists to prevent.

@models REQ-COST.cost_from_usage
@models REQ-COST.price_is_per_model
-/
def spendOf (table : List Price) (usage : List Usage) : Spend :=
  usage.foldl (accumulate table) { amount := 0, unpriced := [] }

private def padded (n : Nat) : String :=
  let digits := toString n
  let zeros := 6 - digits.length
  String.mk (List.replicate zeros '0') ++ digits

/--
The estimate as a person reads it.

It says `estimated`, and when anything went unpriced it says that too, in the
same line. A figure derived from somebody else's log and a table kept by hand is
not a bill, and rendering it as one would be a claim nobody here can back.

@models REQ-COST.estimate_is_labelled
-/
def estimateLine (spend : Spend) : String :=
  let money := toString (spend.amount / 1000000) ++ "." ++ padded (spend.amount % 1000000)
  let tail :=
    if spend.unpriced.isEmpty then ""
    else " (unpriced: " ++ String.intercalate ", " spend.unpriced ++ ")"
  "estimated " ++ money ++ tail

/-- An estimate reads as an estimate: priced, unpriced, empty and whole.

Concrete rather than universal, and deliberately so. A universal proof about
this string is a proof about `String.startsWith`, which is a fact about Lean's
string representation and not about what the clause says. These are the four
shapes the line can take -- nothing priced, something priced, one model
unpriced, several -- and each is evaluated rather than asserted.

@proves REQ-COST.estimate_is_labelled -/
theorem an_estimate_says_so :
    ([{ amount := 0, unpriced := [] },
      { amount := 1234567, unpriced := [] },
      { amount := 999999, unpriced := ["sonnet"] },
      { amount := 0, unpriced := ["opus", "haiku"] }] : List Spend).all
        (fun spend => (estimateLine spend).startsWith "estimated ") = true := by
  native_decide

end TraceLean.Cost
