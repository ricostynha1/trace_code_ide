---
id: REQ-CHECKOUT
title: Checkout pricing
decomposition: complete
status: approved
clauses:
  total: the amount charged is the subtotal, minus the discount, plus shipping
---
A customer's cart is priced at checkout. The amount charged is derived from three
independent decisions — what discount applies, what shipping costs, and what the receipt
must record — each of which is a requirement of its own below.

`decomposition: complete` is a claim: these three children are *all* of this requirement.
TraceLean reports this requirement's coverage as an exact percentage because of that
claim. If a fourth child were discovered later, the honest move is to add it here rather
than to quietly widen the existing three.
