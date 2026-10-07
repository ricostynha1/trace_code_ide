/-
  Properties of the model in `Checkout.lean`.

  Read the level these earn carefully: a theorem here is evidence about the
  *model*, never about the implementation. TraceLean renders that as a chain —
  "L4 model · L3 code" — rather than collapsing it into one green badge, because
  a proved model wired to an untested implementation is not a proved system.
-/
import Checkout

namespace Checkout

/-- @proves REQ-DISCOUNT.tiers

    A discount never exceeds the order it discounts, so the subtraction in
    `price` can never wrap. On `Nat` an underflow would silently clamp to zero
    rather than fail, which is exactly the kind of bug worth ruling out once and
    for all instead of sampling for. -/
theorem discount_le_subtotal (subtotal : Nat) :
    discountCents subtotal ≤ subtotal := by
  -- Every band is `subtotal * p / 100` for some `p ≤ 100`, and that is at most
  -- `subtotal * 100 / 100`, which is `subtotal`. Proving it once for an
  -- arbitrary `p` means adding a band later cannot invalidate the proof.
  -- `Nat.div_le_div_right` is not in Lean 4.12's core, so the bound is
  -- established by multiplication and handed to `omega`, which can close a
  -- goal about division by a literal once it has one.
  have key : ∀ p : Nat, p ≤ 100 → subtotal * p / 100 ≤ subtotal := by
    intro p hp
    have : subtotal * p ≤ subtotal * 100 := Nat.mul_le_mul_left subtotal hp
    omega
  unfold discountCents
  split
  · exact key 15 (by omega)
  · split
    · exact key 10 (by omega)
    · exact Nat.zero_le _

/-- @proves REQ-SHIPPING.free

    Free shipping means free: above the threshold, a non-remote order pays
    nothing to ship. Stated as an implication rather than checked at a few
    sample subtotals. -/
theorem free_above_threshold (afterDiscount : Nat) (h : afterDiscount ≥ 10000) :
    shippingCents afterDiscount false = 0 := by
  unfold shippingCents
  simp [h]

/-- @proves REQ-SHIPPING.remote

    The remote surcharge applies on top of free shipping too — the requirement
    says "including on otherwise-free shipping", and this is that sentence
    written so it cannot drift. -/
theorem remote_surcharge_survives_free_shipping (afterDiscount : Nat)
    (h : afterDiscount ≥ 10000) :
    shippingCents afterDiscount true = 400 := by
  unfold shippingCents
  simp [h]

/-- @proves REQ-SHIPPING.flat

    Below the threshold, shipping is the flat rate. Stated for every subtotal
    below the line rather than sampled at a few, and written because the
    strength obligation showed the free-shipping theorems constrained nothing
    down here: any function at all agreed with them below 10,000. -/
theorem flat_below_threshold (afterDiscount : Nat) (h : afterDiscount < 10000) :
    shippingCents afterDiscount false = 599 := by
  unfold shippingCents
  simp
  omega

/-- @proves REQ-SHIPPING.remote

    And the remote surcharge applies below the threshold too, on top of the
    flat rate. With this the four theorems about `shippingCents` determine it
    completely -- see `Strength.lean`. -/
theorem remote_below_threshold (afterDiscount : Nat) (h : afterDiscount < 10000) :
    shippingCents afterDiscount true = 999 := by
  unfold shippingCents
  simp
  omega

end Checkout
