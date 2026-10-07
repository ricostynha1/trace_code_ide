# T1 — Traceability core: implementation plan

Scope: annotations, anchors, requirement index, graph rebuild, checker, lockfile.
Design reference: `docs/formal_traceability_and_lsp_plan.md` §2, §8, §9.1, §10.

## Deliverables
New module `tracelean/core/src/trace/`:

| File | Contents |
|---|---|
| `mod.rs` | public API, `TraceIndex` (the resolved graph) |
| `annotation.rs` | comment scanning, role parsing |
| `anchor.rs` | symbol-path resolution, normalized-body hashing |
| `requirement.rs` | frontmatter parsing, clauses, DAG edges |
| `checker.rs` | findings + policy |
| `lockfile.rs` | `.tracelean/trace.lock.json` read/write |

Plus: delete `link_by_convention` and the four filename conventions in
`trace_graph/scan.rs`; rewrite `requirements.rs` parsing onto frontmatter.

## 1. Annotation scanning (`annotation.rs`)

Input: file path + content → `Vec<RawAnnotation>`.

- `parser::Lang::from_extension` gives the grammar; walk the tree, collect nodes whose
  `kind()` contains `"comment"` (covers rust `line_comment`/`block_comment`, python
  `comment`, lean4 comments, c++ `comment`). Unknown extension → line scan fallback.
- Inside comment text: `@(?P<role>[a-z_]+)\s+(?P<id>[A-Z][A-Za-z0-9_-]*)(?:\.(?P<clause>[a-z_]+))?`
  then optional `key=value` / `key="quoted value"` attributes (`reason`, `by`, `until`, `via`).
- Roles: `models implements tests drt proves refines partial exempt`. Unknown role → finding
  `unknown-role`, never a silent skip.
- Region form: `@implements REQ-X begin` … `@end` (bare `@end` closes nearest open region in
  the same file).

```rust
pub struct RawAnnotation {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    pub attrs: HashMap<String, String>,
    pub file: PathBuf,
    pub byte: usize,        // comment start
    pub line: u32,
    pub region_end: Option<usize>,
}
```

## 2. Anchors (`anchor.rs`)

1. **Declaration** (default): first `parser::Symbol` whose `start_line >= annotation.line`,
   within a 5-line window (allows attributes/decorators between comment and declaration).
   Symbol path = enclosing named nodes joined with `::`.
2. **Region**: explicit byte range.
3. **File**: no declaration found.

```rust
pub struct Anchor {
    pub file: PathBuf,
    pub kind: AnchorKind,   // Decl { symbol_path } | Region { start, end } | File
    pub body_hash: String,
    pub start_line: u32,
    pub end_line: u32,
}
```

**Normalization before hashing:** strip comment nodes, collapse whitespace runs to one
space, trim. Reformatting and comment edits must not churn; code changes must.

## 3. Requirements (`requirement.rs`)

Every `.md` under the project root (skip `.git`, `node_modules`, `target`, `dist`) with YAML
frontmatter containing `id:`. No directory convention.

```rust
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub file: PathBuf,
    pub refines: Vec<String>,
    pub decomposition: Decomposition,      // Complete | Open (default Open)
    pub clauses: BTreeMap<String, String>,
    pub body: String,
    pub status: ReqStatus,                 // existing workflow enum, kept
}
```

Frontmatter parsing hand-rolled (no new dep): `key: value`, `key: [a, b]`, one-level nested
`clauses:`. Malformed → finding, not a panic. No `id:` → ignored (ordinary markdown).

## 4. Index (`mod.rs`)

```rust
pub struct TraceIndex {
    pub requirements: BTreeMap<String, Requirement>,
    pub links: Vec<Link>,
    pub findings: Vec<Finding>,
    pub evidence: BTreeMap<EvidenceKey, EvidenceRecord>,
}
pub struct Link {
    pub role: Role, pub req_id: String, pub clause: Option<String>,
    pub anchor: Anchor, pub attrs: HashMap<String, String>,
}
```

Build order: requirements → annotations across source files → anchors → join evidence from
lockfile → checker. Keep `trace_graph::TraceGraph` (petgraph) for queries; add `Model` /
`Harness` node kinds and role-matching edges.

## 5. Checker (`checker.rs`)

Findings per design §8: `dangling`, `broken-anchor`, `stale`, `unmodeled`, `unimplemented`,
`unbound`, `untested`, `judge-drift`, `divergence`, `contested`, `unsound-exemption`,
`unknown-role`, `bad-frontmatter`.

- `stale` = evidence exists and `evidence.anchor_hash != anchor.body_hash`.
- `unbound` = clause has `@models` and `@implements` but no `@drt`.
- `unsound-exemption` = missing `reason=`/`by=`, or `until=` in the past.
- Severity from `.tracelean/trace_policy.json`; defaults advisory except `dangling`,
  `broken-anchor`, `unsound-exemption`.

## 6. Lockfile (`lockfile.rs`)

`.tracelean/trace.lock.json`, deterministic (BTreeMaps, sorted vectors):

```json
{ "version": 1,
  "requirements": { "REQ-AUTH-03": { "clauses": ["pre","post"], "hash": "…" } },
  "links": [ { "role": "implements", "req": "REQ-AUTH-03", "clause": "post",
               "file": "src/auth.rs", "symbol": "login", "hash": "…" } ],
  "evidence": [ { "key": {…}, "level": "L3", "backend": "drt", "anchor_hash": "…",
                  "detail": {…}, "at": "…", "node": "…" } ] }
```

Evidence records are preserved across rebuilds — the scanner only ever marks them stale,
never regenerates or deletes them.

## 7. Wiring

- `service.rs`: `trace_scan`, `trace_findings`, `trace_requirement`.
- `gui_backend/src/lib.rs`: tauri commands mirroring those.
- `trace_graph::scan_project`: convention linking removed, annotation linking in; keep code
  element/test node scanning.
- `requirements.rs`: frontmatter parsing; whole-tree scan; `has_spec` becomes "has a
  `@models` link" instead of a `specs/<id>.lean` existence check.

## 8. Tests (`tracelean/tests/`)

- annotation parsing per language (rust `//`, python `#`, lean `--`, c++ `/* */`)
- role + attrs + clause parsing; unknown role
- region `begin`/`@end`; file-anchor fallback
- anchor resolution with attributes/decorators between comment and declaration
- hash stable under reformat, changes on a real edit
- frontmatter parse incl. malformed
- one test per finding class
- lockfile round-trip determinism (serialize twice → identical bytes)
- fixture project under `tests/fixtures/trace_project/` end-to-end

## 9. Order of work
1 `annotation.rs` → 2 `anchor.rs` → 3 `requirement.rs` → 4 `mod.rs` → 5 `checker.rs` →
6 `lockfile.rs` → 7 wiring → 8 delete conventions → 9 fixture end-to-end.

## Non-goals for T1
No DRT, no judge, no UI beyond keeping the existing panel compiling, no LSP.

---

# Revisions after review round 1

## R1 — `@partial` is a qualifier, not a role
`Link` gets `qualifier: Option<Qualifier>` where `Qualifier = Partial { reason } | Exempt {
reason, by, until }`. `@partial REQ-X.c reason="…"` attaches to the *nearest preceding link
in the same comment block*, or to the annotation on the same anchor. A bare `@partial`
without `reason=` is finding `unsound-qualifier` (same class as `unsound-exemption`).
`Role` therefore drops `partial`/`exempt`; they become qualifiers.

## R2 — Suppression semantics (were undefined)
- `@exempt` on a clause suppresses `unmodeled` / `unimplemented` / `untested` for that clause
  and removes it from the coverage denominator.
- `@partial` suppresses nothing; it caps the clause's contribution below 1.0 and is reported
  in its own bucket.

## R3 — Per-bond evidence, not one level
```rust
pub struct EvidenceKey { pub req_id: String, pub clause: Option<String>, pub bond: Bond }
pub enum Bond { RequirementModel, ModelImpl, ModelProof }
pub struct EvidenceRecord {
    pub level: Level,                    // L1..L4
    pub backend: String,                 // "judge" | "drt" | "lean"
    pub input_hashes: BTreeMap<String, String>, // "model"|"impl"|"adapter"|"clause" -> hash
    pub detail: EvidenceDetail,          // typed enum, not untyped JSON
    pub at: String, pub node: Option<String>,
}
pub enum EvidenceDetail {
    Judge { verdict: String, prompt_version: String, model_id: String, witness: Option<Value> },
    Drt   { seed: u64, cases: usize, coverage: Coverage, divergences: usize, lean_version: String },
    Proof { theorem: String, lean_version: String },
}
```
Assurance for a clause = weakest across bonds, rendered as a chain (`L4 model · L1 code`),
never a single badge. `Level` alone is never displayed.

## R4 — Staleness needs *all* input hashes
A record is stale if **any** entry in `input_hashes` disagrees with the current hash of that
input. This is what lets a model-side edit invalidate DRT evidence (single `anchor_hash`
could not).

## R5 — Exclusivity for `contested`
`contested` fires when two links with role `implements` target the same `(req, clause)` and
**both** carry `exclusive` (an attribute: `@implements REQ-X.post exclusive`). Without the
marker, multiple implementers are legal and silent.

## R6 — `refines` reconciliation
Frontmatter `refines:` is the source of truth for the requirement DAG. The `@refines`
annotation role is **dropped** (it would let code reshape the requirement hierarchy, which
is wrong). New findings: `dangling-refines` (target id does not exist) and `refines-cycle`.

## R7 — Grammarless files
A file whose extension has no grammar is scanned line-wise, produces annotations with
`AnchorKind::File`, and its links are capped at **L1** with finding `no-anchor-precision`
(info). They are never `broken-anchor`. Adoption on mixed repos must not look broken.

## R8 — Anchor window and normalization, specified
- Declaration search: the next symbol whose `start_line > annotation.line`, with **no fixed
  window**; the link is rejected (`broken-anchor`) only if no symbol follows in the file.
  Intervening blank lines, attributes, decorators and further comment lines are skipped by
  construction, which removes the invented 5-line rule.
- Normalization: take the anchor's byte range; **remove comment-node byte ranges** (so
  editing an annotation or a doc comment does not invalidate its own evidence — that is the
  intent, since the annotation is metadata, not behaviour); then collapse every run of ASCII
  whitespace *outside string/char literals* to a single space and trim. String literal
  ranges come from tree-sitter node kinds (`string_literal`, `string`, `raw_string_literal`,
  `char_literal`). Fallback path (no grammar): whitespace-collapse the whole file, no comment
  stripping, and mark the link `no-anchor-precision`.
- Hash: `sha256` hex, prefixed with a normalization-scheme version (`v1:`) so the scheme can
  change without silently invalidating every record.

## R9 — Policy file gains rules, not just severities
```json
{ "severity": { "untested": "warn" },
  "require": [ { "match": "REQ-SEC-*", "min_level": "L3", "bond": "ModelImpl" } ],
  "block_on": ["dangling","broken-anchor","divergence","unsound-exemption","unsound-qualifier"] }
```
Default `block_on` now includes `divergence`, per design §8.

## R10 — Regex widened
Clause pattern `[A-Za-z0-9_]+` (design says `\w+`). A clause that does not exist on the
requirement is `dangling`, never silently dropped.

## R11 — Checker/evidence split
`judge-drift` and `divergence` are derived **from lockfile evidence records**, not computed
by T1. T1 ships the finding classes and the derivation; T2/T3 populate the records.

## R12 — Lockfile is committed
`.tracelean/trace.lock.json` is checked in (add to the repo, and make sure it is not in
`.gitignore`). Determinism is a test: serialize twice → identical bytes; rebuild from a
clean checkout → identical file.

## R13 — `.tracelean/` is currently gitignored (blocks R12)
Root `.gitignore` has `.tracelean/`, so the lockfile would never be committed. Git cannot
re-include a file whose parent directory is excluded, so the rule must become:

```gitignore
.tracelean/*
!.tracelean/trace.lock.json
!.tracelean/trace_policy.json
!.tracelean/drt.json
```

Runtime state (sessions, tool_logs, tmp, web, ai_settings.json) stays ignored; the three
declarative files are versioned. Part of step 7 (wiring).

---

# Revisions after review round 2 (codebase check)

## R14 — Do not resolve anchors through `SymbolTable`
`parser.rs` `extract_symbols` walks only the root's direct children, so nested items
(methods in an `impl`, methods in a class) produce no `Symbol` at all and would silently
degrade to a file anchor. `trace/anchor.rs` therefore does **its own recursive tree-sitter
walk**, keeping a parent chain, and never consults `SymbolTable`. Consequence: `Symbol` does
not need byte offsets and is left untouched — blast radius stays inside `trace/`.

## R15 — Comment node matching is an exact set, and does not descend
`kind().contains("comment")` over-matches: tree-sitter-rust's `line_comment`/`block_comment`
contain `doc_comment` and `*_doc_comment_marker` children, so `/// @implements REQ-X` would
be counted two or three times. Match exactly `{ "line_comment", "block_comment", "comment" }`
and do not descend into a matched node. (lean4 0.3 emits a single `comment` node covering
`--`, `/- -/` and `/-- -/`; python and cpp use plain `comment`.)

## R16 — Unwrap declaration wrappers
Lean's `declaration` (wrapping `@[simp] def …` and modifiers) and Python's
`decorated_definition` must be unwrapped to their inner declaration during anchor
resolution, or the attribute/decorator case — the one the plan explicitly promises — yields
no anchor.

## R17 — Two hashes, not one
- `body_hash` — anchor body with comment ranges removed. Drives `stale`.
- `link_hash` — over `(role, req_id, clause, sorted attrs, anchor identity)`. Drives
  evidence-key validity, so retargeting `@implements REQ-A` → `REQ-B` cannot silently keep
  the old evidence.
Lockfile names both; the ambiguous single `"hash"` field is gone.

## R18 — `sha2 = "0.10"`
Already in `Cargo.lock` transitively, so adding it as a direct dependency costs no extra
compile. `DefaultHasher` is explicitly rejected: not stable across releases, which would
churn the lockfile.

## R19 — Type collisions: `trace` is self-contained
`trace_graph::types` already has `Requirement` and `ReqStatus`, and so does
`requirements.rs`. Rather than unify them mid-flight, the new `trace` module owns its own
types and does **not** reuse `trace_graph`'s graph in T1. `trace_graph` keeps working; its
convention linking is removed. Migrating the panel and deleting the duplicate types is T4
work, done once the new index is proven.

## R20 — Legacy requirement format stays readable
`tests/unit/test_requirements.rs` and `example/reqs/*.md` use the `# REQ-01: Title` +
`Status:` form. Frontmatter is preferred, but a document without frontmatter falls back to
that heading form, producing a requirement with no clauses and `decomposition: open`. This
is a *document* format fallback, not a filename convention, so it does not violate the
design — and it keeps the example project and existing tests alive.

## R21 — Decisions on the underspecified points
- **Nested regions**: a stack, LIFO. Bare `@end` closes the most recently opened region in
  the same file. An unclosed region at EOF is finding `unclosed-region`.
- **Region bounds**: body runs from the end of the line holding `begin` to the start of the
  line holding `@end`, exclusive of both comment lines.
- **Clause pattern**: `[A-Za-z0-9_]+`.
- **File anchor `body_hash`**: the whole file, normalized the same way.
- **Symbol path separator**: `::` for every language, so paths are comparable across the
  lockfile regardless of source syntax.
- **Default policy**: ship `.tracelean/trace_policy.json` on first scan if absent.
