---
id: REQ-COST
title: What a sandboxed agent's usage is estimated to have cost
refines: [REQ-TRANSCRIPT, ARCH-HONEST, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  cost_from_usage: The system shall compute spend from the usage a transcript reports and a price table, and from nothing else.
  price_is_per_model: The system shall look up a price by the model the usage names, and if the table does not price that model, then the system shall report the usage as unpriced rather than price it at zero.
  cache_priced_apart: The system shall price cached input, cache writes and ordinary input separately.
  estimate_is_labelled: The system shall render a figure derived from a transcript as an estimate, and never as an amount billed.
---

# What a sandboxed agent's usage is estimated to have cost

The sandbox station shows what a sandboxed tool's usage came to: a reading of
the tool's own log against a price table this project keeps by hand. A model
the table has never heard of is normal, and pricing it at zero would give a
total that looks right and is wrong, so unpriced models are named beside the
amount. Cached input is the cheapest part of a bill and cache writes the
dearest.

Amounts are whole millionths of a unit of currency and prices are per million
tokens, so the arithmetic is over naturals and two implementations cannot
disagree in the last digit.
