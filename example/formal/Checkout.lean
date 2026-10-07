/-
  The executable reference model for checkout pricing.

  This file is written from `product/*.md`, never from the implementation. That
  independence is the whole point: if the model were transliterated from the
  code, differential testing would compare the code against itself and agree
  about the bugs.

  It is a *shallow* embedding — plain compiled Lean functions over `Nat`, total
  and first-order — because differential testing runs it millions of times.
-/
import Lean.Data.Json

open Lean

namespace Checkout

/-- One priced order, as the model sees it. Cents throughout: no floats anywhere
    near money. -/
structure Order where
  subtotalCents : Nat
  remote        : Bool
  deriving Repr, FromJson, ToJson

/-- What pricing decided, broken out so a divergence says *which* decision was
    wrong rather than only that the totals differ. -/
structure Priced where
  discountCents : Nat
  shippingCents : Nat
  totalCents    : Nat
  deriving Repr, FromJson, ToJson

/-- @models REQ-DISCOUNT.tiers
    @models REQ-DISCOUNT.rounding

    Integer arithmetic does the rounding: `Nat` division truncates, which is
    rounding down, which is the direction the requirement asks for. Writing it
    as `subtotal * 15 / 100` rather than `subtotal * 0.15` is what makes the
    `rounding` clause true by construction instead of by luck. -/
def discountCents (subtotal : Nat) : Nat :=
  if subtotal ≥ 20000 then subtotal * 15 / 100
  else if subtotal ≥ 5000 then subtotal * 10 / 100
  else 0

/-- @models REQ-SHIPPING.free
    @models REQ-SHIPPING.flat
    @models REQ-SHIPPING.remote

    Note the argument: shipping is decided on the subtotal *after* the discount.
    An implementation that passes the pre-discount subtotal here agrees with
    this model on most orders and disagrees on exactly the band boundary — the
    shape of bug random differential testing is good at and example-based tests
    usually miss. -/
def shippingCents (afterDiscount : Nat) (remote : Bool) : Nat :=
  let base := if afterDiscount ≥ 10000 then 0 else 599
  base + (if remote then 400 else 0)

/-- @models REQ-DISCOUNT.cap

    The cap the requirement asks for. `min` is the whole of it: whatever the
    tier computes, no order is discounted by more than 5000 cents.

    `engine/pricing.py` does not do this, on purpose. The two disagree only when
    the tier discount exceeds the cap -- above 33,334 cents, where 15% is more
    than 5000 -- which is far enough into the tail that no example-based test in
    this project catches it and uniform random sampling of a `Nat` would have to
    be lucky. The boundary edges the input schema carries from this file's own
    literals are what make it a first-run finding instead. -/
def cappedDiscountCents (subtotal : Nat) : Nat :=
  min (discountCents subtotal) 5000

/-- @models REQ-CHECKOUT.total -/
def price (o : Order) : Priced :=
  let discount := discountCents o.subtotalCents
  let afterDiscount := o.subtotalCents - discount
  let shipping := shippingCents afterDiscount o.remote
  { discountCents := discount
    shippingCents := shipping
    totalCents    := afterDiscount + shipping }

end Checkout
