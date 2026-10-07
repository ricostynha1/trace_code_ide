---
id: REQ-SHIPPING
title: Shipping cost
refines: [REQ-CHECKOUT]
decomposition: complete
status: approved
clauses:
  free: an order whose discounted subtotal reaches 10000 cents ships free
  flat: any other order pays a flat 599 cents
  remote: a remote delivery area adds 400 cents, including on otherwise-free shipping
---
Shipping is decided *after* the discount, so a discount can drop an order out of the
free-shipping band. That ordering is the interesting part of this requirement, and the
kind of thing a differential test finds when an implementation gets it backwards.

The `remote` clause is implemented `@partial`: the code recognises a small hard-coded list
of remote postcodes rather than consulting the carrier's table. A partial implementation
caps what the clause can be worth, so it can never be counted as fully backed.
