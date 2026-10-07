---
id: REQ-DISCOUNT
title: Order discount
refines: [REQ-CHECKOUT]
decomposition: complete
status: approved
clauses:
  tiers: an order of 5000 cents or more is discounted 10%; 20000 cents or more, 15%; below 5000 cents there is no discount
  rounding: the discount is rounded down to a whole cent, so the customer is never charged a fraction
  coupon: a coupon code adds its own percentage on top of the tier discount, capped at 25% in total
  cap: no single order is discounted by more than 5000 cents, whatever the tier says
---
Discounts are a percentage of the cart subtotal, decided by size bands. The bands are
half-open and ascending: the 15% band wins whenever it applies.

The `cap` clause is deliberately **broken** in this example project: the model applies the
cap and `engine/pricing.py` does not. Nothing about the two files looks wrong — each is
readable, and each is self-consistent — and every example-based test in `checks/` passes,
because the disagreement only shows above a subtotal of 33,334 cents. Differential testing
finds it on the first run. That is the whole argument for having an executable model, and
an example where everything agrees cannot make it.

The `coupon` clause is deliberately left unbuilt in this example project. It exists so the
panel has something real to be honest about — `REQ-DISCOUNT` cannot show as finished while
one of its clauses has no implementation, and the "why is this not green?" answer names
exactly that clause.
