---
id: REQ-RECEIPT
title: Receipt
refines: [REQ-CHECKOUT]
decomposition: open
status: draft
clauses:
  lines: every cart line appears on the receipt, with its own price
  audit: the receipt is appended to the audit log
---
What the customer is shown after paying.

`decomposition: open` says more children may exist and have not been written down yet.
TraceLean renders any percentage computed over this requirement as a lower bound — "≥ 60%"
rather than "60%" — and refuses to show it as finished, because the denominator is not
claimed to be complete.

The `audit` clause is `@exempt`: appending to a log is a side effect, and the Lean model is
pure. An exemption stays in the denominator and carries a written reason, so it reads as a
decision someone made rather than as a clause that quietly vanished.
