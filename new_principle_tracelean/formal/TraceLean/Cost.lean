import Lean
import TraceLean.Transcript

namespace TraceLean.Cost

open Lean (ToJson FromJson)

open TraceLean.Transcript (Usage)

structure Price where
  model : String
  input : Nat
  cached : Nat
  cacheWrite : Nat
  output : Nat
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

structure Spend where
  amount : Nat
  unpriced : List String
  deriving Repr, DecidableEq, Inhabited, ToJson, FromJson

def priceOf : List Price → String → Option Price
  | [], _ => none
  | price :: rest, model => if price.model == model then some price else priceOf rest model

/-- @models REQ-COST.cache_priced_apart -/
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

/-- @models REQ-COST.cost_from_usage
@models REQ-COST.price_is_per_model -/
def spendOf (table : List Price) (usage : List Usage) : Spend :=
  usage.foldl (accumulate table) { amount := 0, unpriced := [] }

private def padded (n : Nat) : String :=
  let digits := toString n
  let zeros := 6 - digits.length
  String.mk (List.replicate zeros '0') ++ digits

/-- @models REQ-COST.estimate_is_labelled -/
def estimateLine (spend : Spend) : String :=
  let money := toString (spend.amount / 1000000) ++ "." ++ padded (spend.amount % 1000000)
  let tail :=
    if spend.unpriced.isEmpty then ""
    else " (unpriced: " ++ String.intercalate ", " spend.unpriced ++ ")"
  "estimated " ++ money ++ tail

/-- @proves REQ-COST.cache_priced_apart -/
theorem each_kind_at_its_own_rate (price : Price) (model : String) (n : Nat) :
    amountOf price { model := model, input := n, cached := 0, cacheWrite := 0, output := 0 }
      = n * price.input / 1000000 ∧
    amountOf price { model := model, input := 0, cached := n, cacheWrite := 0, output := 0 }
      = n * price.cached / 1000000 ∧
    amountOf price { model := model, input := 0, cached := 0, cacheWrite := n, output := 0 }
      = n * price.cacheWrite / 1000000 := by
  simp [amountOf]

/-- @proves REQ-COST.price_is_per_model -/
theorem priced_by_its_model_or_named (table : List Price) (usage : Usage) :
    (∀ price, priceOf table usage.model = some price →
      spendOf table [usage] = { amount := amountOf price usage, unpriced := [] }) ∧
    (priceOf table usage.model = none →
      spendOf table [usage] = { amount := 0, unpriced := [usage.model] }) := by
  constructor
  · intro price h
    simp [spendOf, accumulate, h]
  · intro h
    simp [spendOf, accumulate, h, noting]

/-- @proves REQ-COST.cost_from_usage -/
theorem built_record_by_record (table : List Price) (usage : List Usage) (last : Usage) :
    spendOf table (usage ++ [last]) = accumulate table (spendOf table usage) last := by
  simp [spendOf, List.foldl_append]

/-- @proves REQ-COST.estimate_is_labelled -/
theorem every_figure_is_labelled_an_estimate (spend : Spend) :
    ∃ rest : String, estimateLine spend = "estimated " ++ rest :=
  ⟨_, rfl⟩

end TraceLean.Cost
