
## 7 — LSP integration, and Myth × LSP synergies

### User stories
- As a **Dev**, I want real diagnostics, go-to-definition, hover, and symbol search on my
  code, so that TraceLean is a real IDE and not just an editor with a tree.
- As a **Dev**, I want LSP-driven actions (rename, code actions) to be undoable exactly
  like every other edit, so that the undo tree stays the single history of everything.
- As a **Dev**, I want to *discover* what's available at the cursor via the keyboard, so
  that IDE power isn't hidden behind menus I have to hunt through with a mouse.

### Crate choice (corrected from the existing plan)
`tower-lsp` is for building language *servers*; TraceLean needs to be the **client**.
Use **`async-lsp`** — it supports the client direction, composes via `tower::Layer`, and
handles notifications *synchronously in order* (tower-lsp dispatches them async, which its
own docs call "semantically incorrect" — a real problem for tracking live diagnostics). No
LSP crate is in `Cargo.toml` yet; this is greenfield.

### Myth × LSP synergies (the part worth being concrete about)
Myth's model is **content → parser → nodes with captures → a binding map attaching named
actions to captures**, rendered by the frontend with which-key discoverability
(`core/src/myth/surface.rs`, `actions.rs`, `keymap.rs`). LSP is, structurally, *another
producer of captured nodes and location-scoped actions* — so the fit is real, not
hand-wavy:

1. **Code actions ↔ Myth actions — the strongest synergy.** LSP `textDocument/codeAction`
   returns "named operations valid at this location." That is *exactly* Myth's
   capture→actions→which-key shape. An LSP code-action list can populate a Myth binding
   map at the cursor's capture, giving keyboard-discoverable "quick fixes here" for free —
   arguably a *better* use of the ActionRegistry than the file tree's fixed
   open/rename/delete set it powers today.
- 2. **Diagnostics ↔ SurfaceNode captures.** `SurfaceNode` already carries capture + range +
     JSON meta and its doc comment already anticipates "highlight captures for code." A
     diagnostic is just a node with an `error`/`warning` capture and a message in its meta —
     it renders through the *same* surface pipeline as syntax highlighting, no parallel UI.
3. **Symbol search / outline / references ↔ FileTreeSurface's list-nav.** These are the
   same list-of-nodes-you-navigate-and-act-on pattern `FileTreeSurface` already implements;
   swap paths for symbols and the navigation + which-key bindings carry over.
4. **Rename / code-action edits ↔ Command/undo-tree.** LSP `WorkspaceEdit` results should
   be lowered into the existing invertible `Command`s and pushed through the undo tree — so
   an LSP rename is undoable identically to a hand edit. This is general architecture reuse
   (available to any feature), not Myth-specific, but it's what keeps "one history of
   everything" true.

**The one place the synergy does *not* hold (scope this separately):** `FileTreeSurface`
re-parses static content once per view build. Live diagnostics must update on *every
keystroke* in an editing buffer — that's incremental re-parse, a capability the surface
model doesn't have yet. Treat live squiggles as a distinct, harder sub-problem gated on
extending `Surface`; don't let it block go-to-def / symbol search / code actions, which
need no new capability. (Note the overlap with Bug -1's layer 3: both want incremental
tree-sitter re-parse — doing Bug -1 layer 3 first would de-risk this.)

### Suggested implementation priority
1. **Transport + a single server (rust-analyzer) first** — spawn, initialize, capability
   negotiation. Pure infrastructure, shares subprocess/JSON-RPC patterns with the ACP
   client. Nothing user-visible yet.
2. **Go-to-definition + hover + symbol search next** — the request/response features that
   need *no* new surface capability, so they land on the existing Myth pipeline directly
   and prove the integration.
3. **Code actions third** — the highest-synergy feature, wiring LSP actions into the Myth
   ActionRegistry + which-key.
4. **Live diagnostics last**, gated on incremental re-parse (and ideally after Bug -1's
   layer 3 has already introduced incremental tree-sitter). Don't block 1–3 on it.

---

## Master implementation priority (all items, in order, with why)

Ordered by *(user pain now) × (how unblocked it is) ÷ (risk/effort)* — ship correctness
and cheap wins before large greenfield subsystems.

1. **Bug -1 layer 1 (cursor teleport correctness).** A data-loss-adjacent bug hitting the
   Dev on every fast-typed large file, right now. Correctness, self-contained, testable.
   Nothing else matters if the editor itself scatters your keystrokes.
2. **Bug 0-C.2 + C.1 (shell result message + duplicate-call guard).** Cheap, and directly
   stops the budget-burning agent loop the user actually hit. C.2 is nearly free and may
   fix it alone.
3. **Bug 1-B minimal hint + Bug 1-A embeddings auto-build.** Both small/self-contained;
   the hint kills a daily footgun today, the auto-build (free, local) makes semantic
   search real and unblocks the later tool split.
4. **Bug 0-B (stale diff) + Bug 0-A (checkpoint sync).** Higher-effort review-mode
   correctness; B is partly de-risked by having done the C guard first. A is a simple
   papercut with a Ctrl+S workaround, so it rides along last of this group.
5. **`find` tool split (Bug 1 Alternative 1).** The real fix for tool complexity, but it's
   a visible surface change needing user sign-off and depends on embeddings (step 3) being
   in place first.
6. **OpenRouter gated live test (6.5).** Small, unblocks cheap end-to-end provider
   validation that every later agent change benefits from. Slots in whenever there's a gap.
7. **ACP `AcpPanel.tsx` + openCode smoke test (6.8-i).** High value, fully unblocked, low
   risk — pure UI over working backend. Then make the two-loops decision (6.8-ii) with the
   user before any refactor.
8. **Bug -1 layers 2–3 (latency + incremental markdown parse).** Perf polish; do once the
   correctness fix (step 1) is proven, and pair layer 3 with the LSP work that also wants
   incremental re-parse.
9. **LSP (item 7).** Last — the largest greenfield subsystem, best built once Myth's
   surface/action model is exercised and incremental re-parse (step 8) exists to lean on.

Rationale for the shape: steps 1–4 are correctness and cheap-signal fixes to things
users hit *today*; 5–7 are self-contained features with clear value and low risk; 8–9 are
the big/uncertain investments that benefit from everything above being stable first —
mirroring how the previous batch (Bugs 2–4 last session) sequenced correctness before
new subsystems.
