# Action plan after the first review

What the 2026-10-09 review and pinning pass found, what was decided, and how to
fix each. In order of priority. Findings per clause: [review findings](../investigations/review-findings-2026-10-09.md);
the two harder problems explained: [structural clauses and model choice](../investigations/structural-clauses-and-model-choice.md).

## 1. Agent judgements count when a person delegated them — decided

An agent's verdict counts as L2 if a named person authorised that agent to
judge. Today nothing records the authorisation, and `--by ""` is accepted.

- `--verdict … --by claude-review --delegated-by <person>`; the record keeps
  both names. A verdict whose `--by` is not a person on the project's list
  (`.tracelean/judges.json`: who may judge, who they delegate to) needs
  `--delegated-by`; an empty `--by` is refused (fixes JUDGE.human_decides).
- The level chain shows `L2 (delegated by X)`; `--stale` can list delegated
  verdicts a person has not looked at yet.
- The 2026-10-09 verdicts are signed `claude-review (simulated human)`; once
  the field exists, the person who asked for them adds `--delegated-by` in one
  pass, or re-judges what they disagree with.

## 2. Drift: change the requirement or the code — open, to evaluate later

[review findings](../investigations/review-findings-2026-10-09.md) gives an opinion for each of the 75 clauses. Nothing
is changed until each requirement's owner decides.

Related gap: a **drift verdict is not recorded** — it only withdraws an
agreement. So when the model changes after a drift, nothing re-opens, and the
UI cannot show "judged: drift". Record drift and unmodelable as records at L1
with the same input hashes; they then go stale like any other record. Nine
clauses were pinned after being judged drift (marked † in the review findings) and need a look.

## 3. Nothing may be counted twice or lost in the roll-up — decided: fix

The roll-up walks the refinement graph with one `seen` set shared by sibling
branches (`trace/rollup.rs`, `walk`). Where D refines both B and C, D is
counted under B only, so C reports L4 while D is L1. Change it in Rust and
in Lean (`Rollup.lean`):

- Each requirement's figure is computed over **its own set of descendants**,
  each counted once — the set, not the walk. Compute it as the reachable set
  from that node (with a cycle guard), then fold the minimum and the fraction.
  Order of visiting can then not matter.
- **Prove it** in Lean: the result equals the minimum over the reachable set,
  and is unchanged by reordering the input (this is `deterministic_order`, now
  a theorem, not a drift). DRT the Rust against it on generated graphs that
  include diamonds and cycles.

**Catching replication** — so it cannot come back silently:

- a checker finding `SharedRefinement`: a requirement reachable from one
  ancestor by two paths. Warning by default; promote to an error if the project
  decides refinement must be a tree;
- the same for the other places one thing can be claimed twice: two `@pins` for
  one clause, two specs, two function models (see §7), a clause key declared
  twice in one document (`id_unique` covers ids, not keys).

Open question for us: should a requirement be allowed to refine two parents at
all? If not, `SharedRefinement` is an error and the walk can assume a tree.

## 4. Strengthening weak requirements — discuss

Pinning shows a clause is weak: most clauses could not be pinned because the
text leaves something open (order, empty input, format, which of several is
reported). The provers' reports name exactly what is open per clause, which is
a ready backlog.

Your proposal: next to `refines` (sub-requirements), a field of entries that
**strengthen** a requirement. I agree, with three suggestions:

- **Call them narrowings**, not refinements — "refines" already means
  decomposition here, and the two must not be confused: decomposition splits
  *what* is required (and counts in the roll-up); a narrowing fixes *which
  answer* a clause allows (and does not add to the denominator).
- **Attach them to a clause**, since a pin is per clause:

  ```yaml
  clauses:
    min_not_mean: The aggregate shall be the minimum of its parts' levels.
  narrowings:
    min_not_mean:
      empty: With no parts, the aggregate shall be L1.
  ```

- **Hash them with the clause** (§5), so adding a narrowing re-opens the
  clause's judgement and pin. The spec may use only the clause and its
  narrowings — the reviewer checks that — so a spec that needs a convention
  (e.g. ascending order) forces someone to write it down as a narrowing first.

Then `--pins` can say, for an unpinned clause, "open: the text does not fix X",
and the requirement view shows narrowings indented under their clause.

## 5. Hash per clause — decided

Evidence is per clause but the hash is per requirement
(`REQDOC.clause_addressable`), so rewording one clause re-opens every clause.
Change `requirement_hash` to a clause hash: the clause's key, its text and its
narrowings. The prose around the clauses is covered by document review
(`doclink`), not by evidence. Records already carry `("requirement", hash)`;
only what is hashed changes, so existing records go stale once, then settle.

## 6. Structural clauses — explained in [structural clauses and model choice](../investigations/structural-clauses-and-model-choice.md)

Six clauses are about the shape of the code. Mark them `@structural(reason=…)`,
remove their Lean models, and give each an architecture test that reads the
tree (e.g. nothing outside `cmd/` calls `Workspace::set`).

## 7. Say which model is which — explained in [structural clauses and model choice](../investigations/structural-clauses-and-model-choice.md)

- New role `@specifies REQ-X.c` for the `Prop` spec; `@models` for functions.
- A clause with more than one function model, spec or `@pins` is a finding
  unless one is marked `@models(primary)`.
- `--judge` prints every model, not the first.
- Fix the scanner binding a constructor's doc comment to the next declaration
  (PROV.base_is_honest).

## 8. The Lean grammar — fix by extending it

`tree-sitter-lean4` 0.3.0 rejects `by_cases`, `obtain`, `cases … with |`,
`match` in tactics, `first |`, `by` inside a term, typed `fun` binders,
`mutual`, and `simp … at h` before `have`. A rejected file anchors to the whole
file and caps at L1 (ADR-0008). The provers spent most of their time writing
around it. Two steps:

1. **Now, cheap**: the scanner needs declaration *boundaries*, not proof
   *contents*. Before parsing, blank out each proof body (from `:= by` to the
   next top-level keyword) with spaces, keeping byte offsets. Every tactic
   problem disappears; anchors are unchanged. Test: every file in `formal/`
   parses cleanly with no rewrite.
2. **Then**: check upstream for a newer grammar; otherwise vendor
   `tree-sitter-lean4` into the repo (`grammar.js` → generated `parser.c`, a
   path dependency) and add `mutual`, `where` and `termination_by`, which do
   affect declaration boundaries. Keep a corpus test of this project's own
   files so a grammar change cannot silently lose a declaration. ADR-0008's
   rewrites can then be undone.

## 9. Small

- `docs/06-editor.md` is in review against REQ-SHOW and REQ-ACT.
- LSP.encoding_round_trip: pin `toCharacter` on the same prefix relation; the
  round trip then follows.
- ANCHOR.hash_tracks_body says "if and only if"; a 64-bit hash cannot — say
  "changes whenever … (up to hash collision)".
- LOCK.evidence_preserved is pinned trivially (the model is the identity); the
  real check is the Rust test of writing and reading the lock.
- Checker counts a model written by pattern matching (no named inputs) as
  taking no inputs; `shape` should count the arrows of its type instead.
