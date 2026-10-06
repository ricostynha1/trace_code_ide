---
id: REQ-COST
title: What a sandboxed agent's usage is estimated to have cost
refines: [REQ-TRANSCRIPT, ARCH-HONEST, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  cost_from_usage: Spend shall be computed from the usage a transcript reports and a price table, and from nothing else.
  price_is_per_model: A price shall be looked up by the model the usage names, and usage naming a model the table does not price shall be reported as unpriced rather than priced at zero.
  cache_priced_apart: Cached input, cache writes and ordinary input shall be priced separately.
  estimate_is_labelled: A figure derived from a transcript shall be rendered as an estimate and shall never be rendered as an amount billed.
---

# What a sandboxed agent's usage cost

The sandbox station shows what a tool changed, what it said, and what its usage
came to. The third is a reading of somebody else's log against a table we
maintain, and that is the whole of what this requirement is about.

`cost_from_usage` keeps it a function. Usage records and a price table in, an
amount out — so it is modelled, differentially tested and graded like the rest,
and nothing in it reaches for a clock, a network or a billing API.

`price_is_per_model` is the case that decides whether the figure can be
trusted. A table is a thing that goes out of date; a model it has never heard of
is the normal consequence, not an exception. Pricing that at zero produces a
total that is confidently wrong and looks right, which is the failure
[ARCH-HONEST](../arch/ARCH-HONEST.md) exists to prevent. So the unpriced models
are named alongside the amount, and the amount says what it does not include.

`cache_priced_apart` is not a detail: cached input is the cheapest thing on the
bill and cache writes are the dearest, often by an order of magnitude in each
direction. A total that added them at one rate would be wrong by more than
rounding, and wrong in whichever direction the workload happened to lean.

`estimate_is_labelled` is the reason this is allowed to exist at all. The
figure is derived from a log the tool wrote about itself, priced against a table
this project keeps by hand. Rendering it as a bill would be a claim nobody here
can back. It is rendered as an estimate, and when anything was unpriced it says
so in the same line.

Amounts are whole millionths of a unit of currency, and prices are per million
tokens, so the arithmetic is over naturals. A floating-point total would make
two implementations disagree in the last digit for reasons that have nothing to
do with pricing, which is exactly the noise
[ARCH-DETERMINISM](../arch/ARCH-DETERMINISM.md) rules out.
