# Action plan after the first review

What the 2026-10-09 review and pinning pass found, what was decided, and how to
fix each. In order of priority. Findings per clause: [review findings](../investigations/review-findings-2026-10-09.md);
the two harder problems explained: [structural clauses and model choice](../investigations/structural-clauses-and-model-choice.md).

**Status (2026-10-10):** implemented and green: §1, §2, §4, §5, the roll-up
reachable set (§3, Rust, Lean, DRT), coverage verdicts with waivers (§10 code), 26 pins.
§7 several-models cleanup (0 left); §6 mostly (below).
Open: §6 remainder, §7 unmodeled clauses (17: SHOW ×6, UNDO ×2, ACT, LINECOV.uncovered_shown, STALE.requirement_reopens_all, TRANSCRIPT, and the demo's THERMO/TABLE), §8 grammar, and re-earning stale evidence (`--stale`).

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

- a checker finding `SharedRefinement` (a requirement reachable from one
  ancestor by two paths) is **not added**: diamonds are everywhere in this
  project's own graph (`REQ-ACT` reaches `ARCH-CORE-SHELL` through both
  `REQ-MYTH` and `REQ-VIEW`), the roll-up now counts each once, so it would be
  noise. Revisit only if the project decides refinement must be a tree;
- the same for the other places one thing can be claimed twice: two `@pins` for
  one clause, two specs, two function models (see §7), a clause key declared
  twice in one document (`id_unique` covers ids, not keys).

Open question for us: should a requirement be allowed to refine two parents at
all? Today yes, and the roll-up does not assume a tree.

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
- **Nest them under their clause**, so a clause and what narrows it sit
  together (your layout, kept valid YAML so GitHub and YAML tools still read
  the frontmatter):

  ```yaml
  clauses:
    weakest_link: The minimum over bonds.        # no narrowings: unchanged
    min_not_mean:
      text: The aggregate shall be the minimum of its parts' levels.
      empty: With no parts, the aggregate shall be L1.
      order: Parts shall be listed in ascending id.
  ```

  A clause without narrowings keeps its one-line form; with narrowings it is a
  block whose `text:` is the base clause and whose other keys are narrowings
  (`[A-Za-z0-9_]`, `text` reserved). Annotations still name only the clause.
  The parser reads two levels instead of one (ADR-0001's freeze ended at
  stage 2); a new ADR records the change.

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

**Every structural check needs a positive and a negative test — decided.** A
check that only passes on today's tree also passes when the check is broken or
looks for the wrong thing. So:

- each check is a function from a set of files to its violations
  (`fn violations(files) -> Vec<(path, line, why)>`), so one implementation
  serves both tests;
- **positive**: over the real tree, no violations;
- **negative**: over a small made-up tree that breaks the rule (a file outside
  `cmd/` calling `Workspace::set`), it reports exactly that file and line —
  and, where the rule has an allowed place, a file there is *not* reported;
- a structural clause counts only with both: a new requirement clause (in
  REQ-CHECK, next to `structural_is_not_exempt`) says a structural test must
  show its check rejects a violating tree, and the checker reports a
  structural clause whose tests are positive-only.

**Done:** `crates/core/tests/structural.rs` holds the six clauses' checks with
positive and negative tests, and `REQ-CHECK.structural_rejects` states the rule.
Known gaps the checks name: the editor's `in_cells` refit assigns the layout
outside `arrange`, and `perform` names three prompt actions before `dispatch`.
**Open:** the checker does not yet report a structural clause whose tests are
positive-only; and of the 14 tests in `architecture.rs` the needle checks
(model/network, process spawn, clock), the shell check and the axiom check have
a negative case, as do the scratch-name, visibility and process-spawn checks and
a sensitivity check for `moving_a_file…`. Still without one: `every_finding_type…`,
`the_judge_exports…`, `every_unsure_answer…`, `exemptions_are_rare…`,
`nothing_a_binding_names…`. In general the same rule is worth applying to
any test whose pass is "nothing found" — a search that finds nothing must be
shown able to find something — but structural checks come first.

## 7. One model per clause — decided; explained in [structural clauses and model choice](../investigations/structural-clauses-and-model-choice.md)

79 of 219 modelled clauses carry several `@models`, because the role is used
for five different relationships. `@models` comes to mean exactly *the
function that computes what the clause talks about*, and the checker allows
at most one per clause:

| Annotated today | Becomes |
|---|---|
| the function answering the clause | the one `@models` |
| a `Prop` spec (from pinning) | `@specifies`, at most one per clause |
| an operation that must keep the property | nothing: the clause is an invariant, modelled by a theorem over the operations (`@proves`) |
| a type the clause mentions | nothing: a type is vocabulary |
| every member of a family, or pipeline helpers | a universal claim is structural or a theorem; only a pipeline's entry point is annotated |

A clause that still seems to need two functions says two things: split it.
Also at most one `@pins` per clause; `--judge` shows the model and the spec;
fix the scanner binding a constructor's doc comment to the next declaration
(PROV.base_is_honest). Cleanup: sort the 79 clauses into these cases.
*Mechanism done (ADR-0014):* `@specifies`, the three warning kinds, pinning
by role, `--judge` showing all, the constructor/field binding in both
languages. Cleanup done: no clause has several models, and the three kinds
are now errors that block.

**Split, extend or fix.** When a model covers only part of a clause: *split*
when the parts can be met independently (they have their own implementation
and evidence — e.g. "support 8 image formats" is 8 clauses, or 8 child
requirements if each has several obligations); *extend* the model when one
mechanism serves every input (e.g. one seeded generator for every schema);
*fix* the code when it is simply wrong. Splitting is preferred for simpler
requirements and models; do the splits after §5, so evidence re-opens once.
The per-clause choice is in the review findings.

## 8. The Lean grammar — fix by extending it

`tree-sitter-lean4` 0.3.0 rejects `by_cases`, `obtain`, `cases … with |`,
`match` in tactics, `first |`, `by` inside a term, typed `fun` binders,
`mutual`, and `simp … at h` before `have`. A rejected file anchors to the whole
file and caps at L1 (ADR-0008). The provers spent most of their time writing
around it.

Check upstream for a newer grammar; otherwise vendor `tree-sitter-lean4` into
the repo (`grammar.js` → generated `parser.c`, a path dependency) and add
`mutual`, `where` and `termination_by`, which do affect declaration
boundaries. Keep a corpus test of this project's own files so a grammar change
cannot silently lose a declaration. ADR-0008's rewrites can then be undone.

Upstream, as checked 2026-10-09 (not yet compared against this tree's files):
`wvhulle/tree-sitter-lean` (the crate's source) has commits to 2026-05-10,
e.g. doc comments on constructors and `show`/`suffices`; `Julian/tree-sitter-lean`
regenerated its parser 2026-10-04. The tree-sitter CLI (0.25.10) is installed,
and the crate already builds `parser.c` from `grammar.js` when it is absent.

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

## 10. Differential coverage is complete or waived — decided

A differential test only counts if its random inputs reached every case that
matters. Today a binding names *situations* and a floor for each ("deletes a
file: at least 20 cases"); 31 of 106 bindings name none and stay at L1, and a
list somebody wrote can always miss a case.

Rule: the target is 100%, waived only exceptionally, with a reason.

- **Every argument class reached.** Derive the classes from the types, as
  `--drt` already does (negative/zero/positive, empty/not, each `Option` and
  enum case, each struct field's classes). All must be reached; a binding's
  hand-named situations are added on top, never instead.
- **Every line of the implementation run by the differential cases.** Measure
  it with the per-test line coverage (`trace::lines`) over the DRT run, per
  implementing item. Below 100% is not L3.
- **A waiver** names the classes or lines it excuses and why
  (`@waive(lines=…, reason=…)`, like `@exempt`), is shown in the requirement
  view, and is checked by the reviewer like any judgement. An unused waiver
  (the lines are now reached) is reported, so waivers cannot pile up.
- Floors as counts (`atLeast: 20`) stay only as "how often", never as a
  substitute for "reached at all"; the default becomes at least one.

Requirement changes: REQ-DRT-COVER gains clauses for the class and line rules
and the waiver; `coverage::verdict` and its Lean model change to match.

## 11. Carried over from the removed progress and gap documents

- `menu_entries` in `crates/editor` builds a mode's menu from the keymap
  itself instead of through `keymap::which_key`, so it omits the `Escape` row
  the bar has: two which-keys (`REQ-MYTH.whichkey_is_a_query`; same root as
  SHOW.menu_from_keymap in the review findings).
- Not built from the first TraceLean: Lean infoview and type-check on save
  (both start a process, so the user must start them), LSP hover, a coverage
  treemap and trend.
- `cargo test` runs suites one at a time; `tools/differential-all.sh` is the
  fast way to run every differential suite.
- Only a person can check two things, and nothing will: that
  `.tracelean/prices.json` holds today's prices, and that a glyph suits the
  name it stands for.
