# T6 — Lean infoview, project graph, and requirement-visibility work

Six items, from one session's feedback. They are related: five of them are the same
complaint in different places — *the traceability data is real, and the surfaces on top of
it are not yet good enough to act on.*

Everything below was verified against the code, not recalled. Where a thing is broken, the
mechanism is named.

---

## 1. A Lean infoview — proof state beside the file

**The question asked:** can the VS Code Lean experience (goal state on the right) be built
out of LSP information?

**Answer: yes for the text goal view, which is the part that matters, and the plumbing is
already in place.** `core/src/lsp/registry.rs` already answers `QueryMode::Goal` by calling
Lean's `$/lean/plainGoal`, and already calls `textDocument/waitForDiagnostics` before
reporting a Lean file clean. What is missing is a panel and a cursor-follow loop, not a
protocol capability.

One honest limit, stated up front: `$/lean/plainGoal` returns *rendered text*. VS Code's
infoview is richer than that — clickable subterms, `Try this` widgets, collapsible
hypotheses — and those come from Lean's own RPC layer (`$/lean/rpc/connect`, then
`$/lean/rpc/call` for `Lean.Widget.getInteractiveGoals`), which is a session protocol on
top of LSP rather than part of it. The plan takes the text view first and treats the
interactive one as a separate phase, because the text view is 90% of the day-to-day value
for a tenth of the work, and because building the RPC session badly would be worse than not
having it.

### Design

A new right-hand panel, `LeanInfoview.tsx`, visible when the open file is Lean.

Three stacked sections, in the order a proof is actually read:

| Section | Source | Refreshed on |
|---|---|---|
| **Goal at cursor** | `$/lean/plainGoal`, falling back to `$/lean/plainTermGoal` when the cursor is not in tactic position | cursor move (debounced), document change |
| **Messages here** | diagnostics filtered to the cursor's line | document change |
| **All messages** | every diagnostic in the file, `waitForDiagnostics` first | document change |

Backend work:

- `QueryMode::TermGoal` → `$/lean/plainTermGoal`. A tactic-position cursor returns goals
  from `plainGoal`; a term-position one returns `null` there and a real answer from
  `plainTermGoal`. Asking both and preferring the non-null is what makes the panel useful
  everywhere in the file rather than only inside `by` blocks.
- `lsp_goal_at(file, char_pos)` — one command returning `{ goals, term_goal, diagnostics_here,
  diagnostics_all, server_status }`. One round trip per cursor move, not four.
- **`server_status` is part of the payload, not an afterthought.** With no Lean toolchain
  installed the panel must say "Lean server not found — install `elan`", never render an
  empty goal list. An empty goal list means *no goals* — i.e. the proof is complete — and
  showing that for a server that never started would be the exact lie this project exists
  to prevent.

Frontend work:

- Cursor-move listener in `Editor.tsx` (CodeMirror `updateListener`, debounced ~120 ms),
  emitting the char offset on a `tracelean-cursor` event.
- The panel subscribes, calls `lsp_goal_at`, renders. Goals monospaced with `⊢` kept on its
  own line; hypotheses dimmed relative to the target.
- A pin button freezes the panel at a position, so you can edit a tactic while still seeing
  the goal it has to close.
- `Verify` mode already binds `g` to `show_goal`; that action now reveals and pins the panel
  instead of producing a transient popup.

### Tests
- `plainTermGoal` is asked only when `plainGoal` returns no goals.
- A missing server produces a status, never an empty goal list (the lie test).
- Diagnostics are partitioned by line correctly at a line boundary.
- Goal text survives non-ASCII (`⊢`, `α`, `≤`) through the transport — the UTF-16 path again.

### Phase 2, deliberately not now
Interactive goals via `$/lean/rpc/connect` + `Lean.Widget.getInteractiveGoals`, which buys
clickable terms and `Try this` code actions. Gated on the text view being in real use, and
on someone actually having a Lean toolchain on the machine to test against.

---

## 2. Delete the trace graph

**Verified state:** `core/src/trace_graph/` is 820 lines over five files. It is reachable
from the agent tools `query_trace_graph` and `query_code_element`, from `SharedApp.graph`
(`agent/context.rs`, `ai/service.rs`), from `ai/context.rs`, and from the 329-line
`TraceabilityDashboard.tsx`. Its scan half was already gutted in T1 — `link_from_annotations`
now only mirrors the real index into the old petgraph view so the dashboard has something to
query.

It is a second, worse copy of data `TraceIndex` already holds, kept alive by three call
sites. That is the definition of something to delete rather than repair.

### Work
1. Replace the two agent tools with trace-index-backed equivalents (§6 below defines their
   replacement, `project_graph`). Keep the *names* for one release if any saved agent
   transcript references them; make them thin shims that answer from `TraceIndex` and say so.
2. Drop `graph: Arc<Mutex<TraceGraph>>` from `SharedApp` and `AiContext`; the ~6 sites that
   thread it become one `TraceIndex` built on demand and cached by file-mtime set.
3. Delete `TraceabilityDashboard.tsx` and its menu toggle. `TracePanel` already shows
   strictly more, and the project graph (§6) covers what the dashboard was reaching for.
4. Delete `core/src/trace_graph/` and `tests/unit/test_trace_graph.rs`.

**Sequencing note:** do this *after* §6 exists, not before. Deleting the only structural view
and then building its replacement leaves the product worse for however long that takes.

---

## 3. Differential-testing harnesses must be a first-class object

**The complaint, restated:** binding a clause to a differential test means hand-writing
`.tracelean/drt.json` *and* an adapter file *and* a `@drt` annotation, in three places, with
no help from the tool. The example project proves the point — three of its clauses report
`Unbound` (model and implementation both exist, nothing checks they agree), and `Unbound` is
the single most important finding the checker produces.

The pieces already exist and are simply not reachable: `drt::config::scaffold_binding`,
`drt::config::scaffold_adapter`, and `drt::lean_runner::parse_declaration` (which reads a
model definition's signature).

### Design — "Bind this clause", one action

Entry points: the `Unbound` finding in the trace panel, the clause row, and `Verify` mode.
All three dispatch one flow:

1. **Find the model.** The clause's `@models` link names a Lean declaration. `parse_declaration`
   gives its argument and result types.
2. **Propose the input schema from those types**, instead of asking the user to write it.
   `Nat`/`Int`/`Bool`/`String`/`Option`/`List`/structure/inductive all map onto the existing
   whitelisted grammar. Anything outside it is reported as "cannot infer — this model takes
   `X`, which the generator has no rule for", which is an actionable sentence, not a failure.
3. **Pick the implementation side.** The clause's `@implements` link names the file, whose
   extension picks the adapter language. The user confirms the entry point.
4. **Write three things as one undoable `Command::Batch`:** the adapter file (scaffolded, with
   the call into the real function already filled in from the `@implements` anchor), the
   binding appended to `.tracelean/drt.json`, and the `@drt` annotation inserted in the
   adapter. One batch, because a binding that exists in two of the three places is worse
   than one that exists in none — and because it then undoes like any other edit.
5. **Offer to run it immediately** with a small case count, so the loop closes in one sitting
   and a mis-shaped adapter is found now rather than in CI.

A `drt_bind_preview` command returns the whole proposal — inferred schema, adapter source,
binding JSON, insertion points — for review before anything is written. Nothing is written
without the user seeing what.

### The example project
Add harnesses for `REQ-DISCOUNT.rounding`, `REQ-SHIPPING.flat` and `REQ-SHIPPING.remote`, so
`example/` demonstrates the bound state rather than only the unbound one. **Keep exactly one
clause unbound on purpose** — with a comment saying so — because a project where everything
is bound cannot show what `Unbound` looks like, and that finding is the one users most need
to recognise.

### Tests
- Schema inferred from a Lean signature matches the hand-written one in `example/`.
- An un-inferable type reports which type and does not write a partial binding.
- The three writes are one batch, and undo removes all three.
- The scaffolded adapter, run against its model, produces no divergence on the example.

---

## 4. A findings filter in the trace panel

**Asked for:** "a selector only to see the errors."

Today the panel renders the whole requirement tree and findings are visible only inside a
selected requirement's detail. On a project with thirty requirements and four problems, the
four problems are the reason you opened the panel.

### Design
A filter strip under the header with counts, so the filter also serves as the summary:

```
all (32) · errors (4) · warnings (7) · unbound (3) · stale (2) · mine
```

- Selecting a filter reduces the tree to requirements with a matching finding, keeping
  ancestors for context but dimming them, so a filtered tree is still a tree.
- Counts come from `TraceIndex::findings` grouped by the policy's severity mapping — the
  same severity the CI gate uses, so the panel and the gate can never disagree about what an
  error is.
- A flat **findings list** as an alternative to the tree, sorted by severity then requirement,
  each row jumping to the annotation. This is the view you want when fixing, as opposed to
  when surveying.
- `Trace` mode gains `e` for "errors only" and `E` to clear.

### Tests
- Counts match `findings` filtered by the same severity rule as `block_on`.
- Filtering never hides a requirement that has a matching finding beneath it.
- An empty filter result says which filter is active, rather than rendering as "no
  requirements" — the failure mode that makes a filter feel broken.

---

## 5. The judge should emit a prompt, not an API bill

**Asked for:** the judge button should produce a fully self-contained prompt to paste into a
Claude Code agent, rather than spending Bedrock credit.

This is a better default than it first sounds, and not only on cost. The agent on the other
end can *open the files*, which a one-shot API call cannot — and the thing being judged is
whether an English clause and a Lean definition agree, which is exactly the task where
reading the surrounding code helps.

The machinery is already there and provider-independent:
`judge::prompt::{SYSTEM, render_user, PromptInput}` builds the prompt,
`judge::verdict::parse` reads the reply, `judge::witness::check` executes the witness.
Only the transport is hardwired to a provider.

### Design
1. `judge_prompt(req_id, clause) -> { text, expected_reply_schema, version }` — the system
   prompt and the rendered user message concatenated into one block, with the clause text,
   the model source, and the requirement body all inlined. Self-contained: pasting it into a
   fresh agent with no repository access still works.
2. The panel's judge button becomes **Copy judge prompt**, with the provider call demoted to
   a secondary "run via API" action for anyone who wants it. A "paste verdict" box takes the
   reply back.
3. **The pasted reply goes through the identical pipeline as an API reply** — `verdict::parse`,
   then witness execution against the compiled model. This is the non-negotiable part: a
   verdict typed in by hand gets no more trust than one that arrived over HTTPS, its claims
   about Lean are still executed, and a falsified witness still discards it. Changing the
   transport must not change the standard of evidence.
4. The evidence record grows `source: api | pasted`, because "who produced this verdict" is
   part of what the record is for.

### Tests
- The rendered prompt contains the clause text, the model source and the reply schema, and
  resolves with no repository access (a fixture asserts no bare file references).
- A pasted verdict with a drift claim and no witness is rejected exactly like an API one.
- A pasted verdict whose witness is falsified is discarded and recorded as falsified.
- `source` round-trips through the lockfile.

---

## 6. The coverage map is broken — and the project graph that replaces the trace graph

### 6a. Why the map is broken (two independent bugs, both verified)

**Backend, `core/src/trace/map.rs`.** `build` pushes one `MapEntry` per *link*, not per
*anchor*. In `example/engine/pricing.py`, `shipping_cents` carries three `@implements`
annotations on one function, so the map emits three rectangles covering the same nine lines
and adds those nine lines to `covered` three times. `traced_lines` is then salvaged with
`.min(total)`, which hides the overcount in small files and leaves it in large ones. The
headline percentage is therefore wrong, and wrong in the flattering direction.

*Fix:* group links by anchor. One rectangle per anchored region, carrying the union of the
requirements that claim it, sized by the region's real line span. Coverage counts each line
once by construction.

**Frontend, `CoverageTreemap` in `TracePanel.tsx`.** It is not a treemap. Each rectangle's
width is computed independently as `sqrt(share * 1.6)` with height `share / w`, so no area
is conserved and nothing tiles. `y` accumulates past `height` without bound; the
`h: Math.min(h, height - y)` clamp then drives later rectangles to zero or negative height,
so they silently vanish. `entries.slice(0, 400)` truncates with no indication that anything
was dropped.

*Fix:* a real squarified treemap (Bruls–Huizing–van Wijk), laid out over a **directory
hierarchy** rather than a flat list, so the map reads as the project. Nesting also solves the
truncation: a directory becomes one rectangle until you zoom into it, so there is nothing to
silently drop. Where a cap is still needed, it is rendered as an explicit "+38 more" tile —
the rule from the which-key work, applied again.

### 6b. The project graph

**Asked for:** replace the trace graph with a view of the code — what relates to what — and
overlay on it the requirements each part serves, the evidence behind them, and the errors.

This is the coverage map's own idea taken seriously: the map answers *how much* of the
project is traced; the graph answers *which parts, and to what*.

**Nodes** are code, not requirements: modules, files and top-level declarations, drawn from
the tree-sitter symbol table the anchors already use.

**Edges** are structural facts we can extract without a language server, so the graph works
with no toolchain installed: containment (module → file → declaration), and call/reference
edges from the LSP when a server *is* running, clearly marked as such. Two edge kinds with
different confidence must look different; an inferred edge drawn like a certain one is the
kind of thing that makes a diagram untrustworthy.

**The overlay** is what makes it a TraceLean view rather than a generic dependency graph:

- Each node is tinted by the assurance of the requirements anchored in it, using the same
  weakest-link rule as everywhere else, and **grey when untraced** — the same grey as the
  coverage map, meaning the same thing.
- Selecting a node lists its requirements, its evidence records with the command that
  produced each, and its findings.
- Findings render *on* the node — a badge, not a separate list — because "where is the
  problem" is the question a graph is good at answering.
- A requirement can be selected from the trace panel to highlight every node serving it, and
  the reverse. This is the "zoom to whatever granularity" idea from the original design,
  applied to code instead of to the requirement hierarchy.

**Layout:** containment as nesting (a treemap-like packing, sharing the §6a implementation),
references as edges over it. This avoids a force-directed hairball, which is what the old
trace graph was heading toward and one reason it was never usable.

**Scale:** a real project has thousands of declarations. The graph therefore opens at module
level and expands on demand, and never renders more than a few hundred nodes at once. A view
that hangs on a big repository is a view nobody opens twice.

### Tests
- One anchor with three links produces one rectangle and counts its lines once.
- Coverage percentage over a fixture equals the hand-computed value.
- The treemap tiles: rectangles are disjoint and their areas sum to the container, within
  rounding.
- Nothing is dropped without being counted in a "+N more" tile.
- Every node's tint equals the minimum assurance of its requirements.
- A node with no annotation is grey, and a node whose requirement has no evidence is not.

---

## Sequencing

The order is driven by one rule: never delete a working surface before its replacement
exists, and fix what is *wrong* before adding what is *missing*.

1. **§6a — fix the coverage map.** Self-contained, it is actively misleading today, and its
   treemap implementation is the foundation §6b needs.
2. **§4 — findings filter.** Small, and it is the fastest improvement to daily use.
3. **§5 — judge prompt export.** Small, self-contained, and it stops the meter running.
4. **§1 — Lean infoview.** Independent of everything else; largest user-visible win.
5. **§3 — harness binding flow**, then bind the example project's clauses.
6. **§6b — project graph.**
7. **§2 — delete the trace graph**, only once §6b covers it.

## Risks

- **No Lean toolchain on this machine.** §1 and §3 both need one for their last mile. Both
  must degrade to a stated, visible "not configured" rather than to silence — and the tests
  must not require a toolchain to run.
- **§2 touches the agent's tool surface.** `query_trace_graph` and `query_code_element` are in
  `data/tools.json` and in saved transcripts. Replace behind the same names first, delete the
  names later.
- **The project graph is the largest item and the least specified by the request.** Build it
  behind the existing panel toggle and keep the trace panel authoritative until the graph has
  been used on a real repository.
