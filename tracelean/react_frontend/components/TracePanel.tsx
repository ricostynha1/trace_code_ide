import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { hierarchyFromPaths, layoutTreemap } from "./treemap";

/**
 * The traceability panel.
 *
 * Two rules from the design drive everything visible here:
 *
 *  - Assurance is shown as a *chain* (`L4 model · L1 code`), never as one
 *    badge. A proof about a model says nothing about code that nothing binds
 *    to it, and a single badge would hide exactly that.
 *  - A percentage is only exact when the requirement claims its children are
 *    complete. Otherwise it renders as "≥ x%", because the denominator is
 *    unknown.
 */

type Level = "L1" | "L2" | "L3" | "L4";
type Decomposition = "complete" | "open";
type Severity = "error" | "warn" | "info";

interface RollUp {
  req_id: string;
  coverage: number;
  coverage_is_lower_bound: boolean;
  assurance: Level;
  leaf_clauses: number;
  exempt_clauses: number;
  stale_clauses: number;
  error_count: number;
  warn_count: number;
  reachable: string[];
}

interface TreeNode {
  req_id: string;
  title: string;
  depth: number;
  rollup: RollUp;
  decomposition: Decomposition;
  children: TreeNode[];
  duplicate: boolean;
}

interface Assurance {
  requirement_model: Level | null;
  model_impl: Level | null;
  model_proof: Level | null;
  stale: string[];
}

interface Link {
  role: string;
  req_id: string;
  clause: string | null;
  anchor: { file: string; kind: { kind: string; symbol_path?: string }; start_line: number };
  line: number;
}

/**
 * Whether the proved properties are known to *determine* the model.
 *
 * A theorem can hold of many different functions: `discount ≤ subtotal` is
 * satisfied by the constant-zero discount. Coverage answers "is there evidence",
 * strength answers "does the evidence pin anything down", and the second
 * question has no answer at all unless somebody asks it — which is why `open`
 * is a state of its own rather than a quiet pass.
 */
type Strength =
  | { kind: "pinned" }
  | { kind: "attempted"; unproved: string[] }
  | { kind: "open" }
  | { kind: "nondeterministic"; reason: string | null };

const STRENGTH_LABEL: Record<Strength["kind"], string> = {
  pinned: "pinned",
  attempted: "unfinished",
  open: "unasked",
  nondeterministic: "not determined",
};

const STRENGTH_MEANING: Record<Strength["kind"], string> = {
  pinned:
    "The proved properties determine the model: anything satisfying them is that function. Checked by the Lean kernel.",
  attempted:
    "Somebody wrote the uniqueness obligation but has not finished it — a `sorry` remains.",
  open:
    "Nobody has asked whether the proved properties determine the model. Not a failure, and not a pass.",
  nondeterministic:
    "Declared not determined by its inputs — randomness, a clock, concurrency — with a reason.",
};

const STRENGTH_COLOR: Record<Strength["kind"], string> = {
  pinned: "#3f7fbf",
  attempted: "#c8952f",
  open: "#666",
  nondeterministic: "#777",
};

interface ClauseView {
  key: string | null;
  text: string;
  assurance: Assurance;
  weakest: Level;
  exempt: boolean;
  partial: boolean;
  strength: Strength;
  links: Link[];
}

/** Shown only where it can mean something: a clause with a proof. */
function StrengthTag({
  clause,
  verified,
}: {
  clause: ClauseView;
  verified?: Strength;
}) {
  // The index knows a `@pins` theorem was written; only the kernel knows it was
  // finished. Until somebody asks, "unfinished" is the honest reading of a
  // claim -- so an unverified attempt is never shown as pinned.
  const strength = verified ?? clause.strength;
  const proved = clause.links.some((l) => l.role === "proves");
  if (!proved && strength.kind === "open") return null;
  const reason = strength.kind === "nondeterministic" ? strength.reason : null;
  const unproved = strength.kind === "attempted" ? strength.unproved : [];
  return (
    <span
      className="trace-tag strength"
      style={{ borderColor: STRENGTH_COLOR[strength.kind] }}
      title={
        STRENGTH_MEANING[strength.kind] +
        (verified ? "" : "\n\nNot yet checked against the Lean kernel — press `spec` to ask.") +
        (reason ? `\n\nReason: ${reason}` : "") +
        (unproved.length ? `\n\nUnfinished at ${unproved.join(", ")}` : "")
      }
    >
      spec {STRENGTH_LABEL[strength.kind]}
      {verified ? " ✓" : ""}
    </span>
  );
}

interface Finding {
  kind: string;
  severity: Severity;
  message: string;
  file: string;
  line: number;
  req_id: string | null;
  clause: string | null;
  /** On the policy's `block_on` list, i.e. this one fails the CI gate. */
  blocking: boolean;
}

interface RequirementView {
  requirement: { id: string; title: string; file: string; decomposition: Decomposition };
  clauses: ClauseView[];
  coverage: number;
  coverage_is_lower_bound: boolean;
  findings: Finding[];
  children: string[];
}

interface MapEntry {
  file: string;
  symbol: string | null;
  lines: number;
  requirements: string[];
  weakest: Level | null;
  stale: boolean;
}

interface CoverageMap {
  entries: MapEntry[];
  traced_lines: number;
  total_lines: number;
}

/**
 * What each assurance level actually means. "L1..L4" is the internal
 * vocabulary; a bare level number on screen tells the reader the scale has
 * four positions and nothing about what any of them establishes, so the name
 * is what is drawn and the level rides along as a prefix.
 */
const LEVEL_NAME: Record<Level, string> = {
  L1: "claimed",
  L2: "judged",
  L3: "tested",
  L4: "proved",
};

const LEVEL_MEANING: Record<Level, string> = {
  L1: "L1 claimed — an annotation says these are linked. Nothing has checked it.",
  L2: "L2 judged — an LLM judge agreed the model says what the requirement says, with an executed witness.",
  L3: "L3 tested — differential testing found no disagreement between the model and the code, over a run that cleared its coverage floor.",
  L4: "L4 proved — a machine-checked proof about the model.",
};

const LEVEL_COLORS: Record<Level, string> = {
  L1: "#888",
  L2: "#d19a66",
  L3: "#4ec9b0",
  L4: "#569cd6",
};

const SEVERITY_COLORS: Record<Severity, string> = {
  error: "#f14c4c",
  warn: "#d19a66",
  info: "#888",
};

/**
 * Zoom altitudes: the same evidence, asked at a different granularity. The
 * buttons are named rather than numbered -- a row of bare digits told the
 * reader the control had five positions and nothing about what any of them
 * would show. Three is also the honest count: levels 4 and 5 existed in the
 * numbering but rendered exactly what 3 rendered.
 */
const ZOOM_LEVELS: Array<{ label: string; hint: string }> = [
  { label: "Capabilities", hint: "Top-level capabilities only, collapsed" },
  { label: "Requirements", hint: "Every requirement and clause, expanded" },
  { label: "Evidence", hint: "Expanded, plus the evidence for the selected requirement" },
];
const MAX_ZOOM = ZOOM_LEVELS.length;

function formatCoverage(value: number, lowerBound: boolean): string {
  const pct = Math.round(value * 100);
  return lowerBound ? `≥ ${pct}%` : `${pct}%`;
}

/** The chain, never collapsed into one number. */
function AssuranceChain({ assurance }: { assurance: Assurance }) {
  const parts: Array<[string, Level | null]> = [
    ["req↔model", assurance.requirement_model],
    ["model↔code", assurance.model_impl],
    ["proof", assurance.model_proof],
  ];
  return (
    <span className="trace-chain">
      {parts.map(([label, level], i) => (
        <span key={label}>
          {i > 0 && <span className="trace-chain-sep"> · </span>}
          <span
            className="trace-chain-part"
            style={{ color: level ? LEVEL_COLORS[level] : "#555" }}
            title={
              level
                ? `${label}: ${LEVEL_MEANING[level]}`
                : `${label}: nothing has been checked`
            }
          >
            {level ? `${level} ${LEVEL_NAME[level]}` : "—"} {label}
          </span>
        </span>
      ))}
      {assurance.stale.length > 0 && (
        <span className="trace-stale" title="changed since it was last verified">
          {" "}⟳ stale
        </span>
      )}
    </span>
  );
}

function CoverageBar({ value, lowerBound }: { value: number; lowerBound: boolean }) {
  return (
    <span className="trace-bar" title={lowerBound ? "the denominator is not known to be complete" : undefined}>
      <span className="trace-bar-track">
        <span
          className="trace-bar-fill"
          style={{ width: `${Math.round(value * 100)}%` }}
        />
      </span>
      <span className="trace-bar-label">{formatCoverage(value, lowerBound)}</span>
    </span>
  );
}

function TreeRow({
  node,
  selected,
  onSelect,
}: {
  node: TreeNode;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState(node.depth < 1);
  const r = node.rollup;

  return (
    <div className="trace-tree-node">
      <div
        className={`trace-row${selected === node.req_id ? " selected" : ""}${
          node.duplicate ? " duplicate" : ""
        }`}
        style={{ paddingLeft: `${node.depth * 14 + 4}px` }}
        onClick={() => onSelect(node.req_id)}
      >
        <button
          className="trace-twisty"
          onClick={(e) => {
            e.stopPropagation();
            setExpanded(!expanded);
          }}
          disabled={node.children.length === 0}
        >
          {node.children.length === 0 ? "·" : expanded ? "▾" : "▸"}
        </button>
        <span className="trace-id">{node.req_id}</span>
        <span className="trace-title">{node.title}</span>
        <CoverageBar value={r.coverage} lowerBound={r.coverage_is_lower_bound} />
        <span className="trace-level" style={{ color: LEVEL_COLORS[r.assurance] }}>
          {r.assurance}
        </span>
        {r.stale_clauses > 0 && (
          <span className="trace-stale" title={`${r.stale_clauses} stale clause(s)`}>
            ⟳{r.stale_clauses}
          </span>
        )}
        {r.error_count > 0 && (
          <span className="trace-count error" title={`${r.error_count} error(s)`}>
            {r.error_count}
          </span>
        )}
        {node.decomposition === "open" && (
          <span className="trace-open" title="children are not claimed to be exhaustive">
            open
          </span>
        )}
        {node.duplicate && (
          <span className="trace-dup" title="already shown under another parent">
            ↗
          </span>
        )}
      </div>
      {expanded &&
        node.children.map((child) => (
          <TreeRow key={`${child.req_id}-${child.depth}`} node={child} selected={selected} onSelect={onSelect} />
        ))}
    </div>
  );
}

function ClauseRow({
  clause,
  verifiedStrength,
  onOpen,
  onJudge,
  onJudgePrompt,
  onDrt,
}: {
  clause: ClauseView;
  /** The kernel's answer, once somebody has asked for it. */
  verifiedStrength?: Strength;
  onOpen: (file: string, line: number) => void;
  onJudge: (clause: string | null) => void;
  onJudgePrompt: (clause: string | null) => void;
  onDrt: (clause: string | null) => void;
}) {
  const missing = useMemo(() => {
    const roles = new Set(clause.links.map((l) => l.role));
    const gaps: string[] = [];
    if (!roles.has("models")) gaps.push("no @models");
    if (!roles.has("implements")) gaps.push("no @implements");
    if (roles.has("models") && roles.has("implements") && !roles.has("drt"))
      gaps.push("no @drt harness binding them");
    if (!roles.has("tests")) gaps.push("no @tests");
    if (clause.assurance.stale.length > 0) gaps.push("evidence is stale");
    return gaps;
  }, [clause]);

  return (
    <div className="trace-clause">
      <div className="trace-clause-head">
        <span className="trace-clause-key">{clause.key ?? "(requirement)"}</span>
        {clause.exempt && (
          <span className="trace-tag exempt" title="deliberately outside the model">
            exempt
          </span>
        )}
        {clause.partial && (
          <span className="trace-tag partial" title="claims only part of the clause">
            partial
          </span>
        )}
        <StrengthTag clause={clause} verified={verifiedStrength} />
        <AssuranceChain assurance={clause.assurance} />
      </div>
      <div className="trace-clause-text">{clause.text}</div>

      {clause.links.length > 0 && (
        <div className="trace-links">
          {clause.links.map((link, i) => (
            <button
              key={i}
              className="trace-link"
              onClick={() => onOpen(link.anchor.file, link.anchor.start_line)}
              title={`${link.anchor.file}:${link.anchor.start_line + 1}`}
            >
              @{link.role} {link.anchor.kind.symbol_path ?? link.anchor.file}
            </button>
          ))}
        </div>
      )}

      {!clause.exempt && missing.length > 0 && (
        <div className="trace-gap" title="why this is not green">
          {missing.join(" · ")}
        </div>
      )}

      <div className="trace-clause-actions">
        {/* The copy path is primary: it costs nothing, and an agent on the
            other end can open the surrounding files, which an API call cannot.
            The verdict is worth the same either way — what sets the standard of
            evidence is that the reply is parsed and its witness executed. */}
        <button onClick={() => onJudgePrompt(clause.key)}>copy judge prompt</button>
        <button className="secondary" onClick={() => onJudge(clause.key)} title="Call the configured provider (costs money)">
          judge via API
        </button>
        <button onClick={() => onDrt(clause.key)}>differential test</button>
      </div>
    </div>
  );
}

/**
 * The coverage map: the project laid out by what each part of it serves.
 *
 * The grey is the point. A map showing only annotated code would flatter the
 * project; every source line appears here, sized honestly, so "how much of this
 * is traced at all" is answerable at a glance.
 *
 * Areas are meaningful because `layoutTreemap` conserves them (see
 * `treemap.ts`, and the tests that pin it). Nesting follows the directory tree,
 * which is also why nothing has to be truncated: a directory is one rectangle
 * until it is large enough to open.
 */
function CoverageTreemap({
  map,
  onSelect,
  onOpen,
}: {
  map: CoverageMap;
  onSelect: (req: string) => void;
  onOpen: (file: string) => void;
}) {
  const width = 320;
  const height = 200;

  const tiles = useMemo(() => {
    const roots = hierarchyFromPaths(
      map.entries.map((entry) => ({
        // The symbol becomes a path segment so annotated declarations nest
        // inside their file, the way they do in the code.
        path: entry.symbol ? `${entry.file}/${entry.symbol}` : entry.file,
        value: Math.max(entry.lines, 1),
        data: entry,
      }))
    );
    return layoutTreemap(roots, { x: 0, y: 0, w: width, h: height }, { headerHeight: 0 });
  }, [map]);

  const percent = Math.round((map.traced_lines / Math.max(map.total_lines, 1)) * 100);

  return (
    <div className="trace-map">
      <div className="trace-map-head">
        Coverage map — {percent}% of {map.total_lines} lines carry an annotation
      </div>
      <svg width={width} height={height} role="img" aria-label="coverage map">
        {tiles.map((tile, i) => {
          const entry = tile.node.data;
          const { x, y, w, h } = tile.rect;
          if (w <= 0.5 || h <= 0.5) return null;

          // A container has no entry of its own; it is drawn as a frame so the
          // directory structure reads, without claiming a coverage colour.
          if (!entry) {
            return (
              <rect
                key={i}
                x={x}
                y={y}
                width={w}
                height={h}
                fill="none"
                stroke="#3e4451"
                strokeWidth={tile.depth === 0 ? 1 : 0.5}
              >
                <title>{tile.node.name}</title>
              </rect>
            );
          }

          const untraced = entry.requirements.length === 0;
          const fill = untraced
            ? "#2a2a2a"
            : entry.stale
            ? "#555"
            : entry.weakest
            ? LEVEL_COLORS[entry.weakest]
            : "#4b5263";
          return (
            <rect
              key={i}
              x={x}
              y={y}
              width={Math.max(w - 0.5, 0.5)}
              height={Math.max(h - 0.5, 0.5)}
              fill={fill}
              stroke="#1e1e1e"
              strokeWidth={0.5}
              className="trace-map-tile"
              onClick={() =>
                entry.requirements[0] ? onSelect(entry.requirements[0]) : onOpen(entry.file)
              }
            >
              <title>
                {entry.file}
                {entry.symbol ? ` :: ${entry.symbol}` : ""} — {entry.lines} lines —{" "}
                {untraced
                  ? "untraced"
                  : `${entry.requirements.join(", ")}${
                      entry.weakest ? ` (${entry.weakest})` : " (no evidence yet)"
                    }`}
                {entry.stale ? " — evidence is stale" : ""}
              </title>
            </rect>
          );
        })}
      </svg>
      <div className="trace-map-legend">
        <span><i style={{ background: "#2a2a2a" }} />untraced</span>
        <span><i style={{ background: "#4b5263" }} />annotated</span>
        <span><i style={{ background: LEVEL_COLORS.L2 }} />L2</span>
        <span><i style={{ background: LEVEL_COLORS.L3 }} />L3</span>
        <span><i style={{ background: LEVEL_COLORS.L4 }} />L4</span>
        <span><i style={{ background: "#555" }} />stale</span>
      </div>
    </div>
  );
}


interface HistoryPoint {
  commit: string;
  at: string;
  subject: string;
  coverage: number;
  coverage_is_lower_bound: boolean;
  assurance_counts: Record<string, number>;
  requirements: number;
  weakest: string | null;
}

/**
 * Coverage over recent commits, as a plain SVG line.
 *
 * Two things it refuses to smooth over: a point whose coverage is only a lower
 * bound is drawn hollow, and commits that could not be indexed are reported as
 * a count rather than interpolated across. A progress chart that quietly
 * invents the missing half is worse than no chart.
 */
function ProgressChart({
  points,
  problems,
}: {
  points: HistoryPoint[];
  problems: string[];
}) {
  if (points.length < 2) return null;
  const w = 220;
  const h = 36;
  const step = w / (points.length - 1);
  const y = (c: number) => h - 2 - c * (h - 4);
  const path = points.map((p, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${y(p.coverage).toFixed(1)}`).join(" ");
  const last = points[points.length - 1];
  const first = points[0];
  const delta = last.coverage - first.coverage;

  return (
    <div className="trace-history" title={`${points.length} commits, oldest ${first.at.slice(0, 10)}`}>
      <svg width={w} height={h} className="trace-history-svg">
        <path d={path} fill="none" stroke="#61afef" strokeWidth="1.5" />
        {points.map((p, i) => (
          <circle
            key={p.commit}
            cx={i * step}
            cy={y(p.coverage)}
            r={2}
            fill={p.coverage_is_lower_bound ? "none" : "#61afef"}
            stroke="#61afef"
            strokeWidth="1"
          >
            <title>
              {`${p.at.slice(0, 10)} ${p.subject}\n${p.coverage_is_lower_bound ? "≥ " : ""}${Math.round(p.coverage * 100)}% coverage, weakest ${p.weakest ?? "—"}`}
            </title>
          </circle>
        ))}
      </svg>
      <span className={`trace-history-delta${delta < 0 ? " down" : ""}`}>
        {delta >= 0 ? "+" : ""}
        {Math.round(delta * 100)} pts
      </span>
      {problems.length > 0 && (
        <span className="trace-history-gaps" title={problems.join("\n")}>
          {problems.length} commit{problems.length === 1 ? "" : "s"} not indexed
        </span>
      )}
    </div>
  );
}


/**
 * What the findings filter can narrow to.
 *
 * `errors` deliberately means *blocking* rather than "severity is error": the
 * `blocking` flag comes from the same `block_on` policy list the CI gate reads,
 * so the panel and the build can never disagree about what an error is.
 */
type FindingFilter = "all" | "errors" | "warnings" | "unbound" | "stale";

const FILTERS: Array<{ key: FindingFilter; label: string; match: (f: Finding) => boolean }> = [
  { key: "all", label: "all", match: () => true },
  { key: "errors", label: "errors", match: (f) => f.blocking || f.severity === "error" },
  { key: "warnings", label: "warnings", match: (f) => f.severity === "warn" },
  { key: "unbound", label: "unbound", match: (f) => f.kind === "unbound" },
  { key: "stale", label: "stale", match: (f) => f.kind === "stale" },
];

/** Human sentence for a finding kind, for the list's group headers. */
const KIND_LABEL: Record<string, string> = {
  dangling: "points at a requirement or clause that does not exist",
  "dangling-refines": "refines a requirement that does not exist",
  "refines-cycle": "refinement cycle",
  "duplicate-id": "two documents declare this id",
  stale: "evidence exists but an input changed",
  unmodeled: "no Lean model",
  unimplemented: "modeled, but nothing implements it",
  unbound: "model and implementation exist, but nothing checks they agree",
  untested: "implemented, but no test",
  "judge-drift": "the judge found drift",
  "judge-unreliable": "the judge's claims about the model were falsified",
  divergence: "differential testing found a mismatch",
  contested: "two links exclusively claim the same clause",
  "unsound-exemption": "exemption without a reason, approver, or still in date",
  "unsound-qualifier": "a qualifier with nothing to qualify",
  "unknown-role": "not a role this system knows",
  "policy-unmet": "below the level the policy requires",
};

/**
 * The findings, flat and sorted by severity.
 *
 * This is the view you want when fixing, as opposed to when surveying: the tree
 * answers "where does this project stand", the list answers "what do I do next".
 */
function FindingsList({
  findings,
  onSelect,
  onOpen,
}: {
  findings: Finding[];
  onSelect: (req: string) => void;
  onOpen: (file: string, line?: number) => void;
}) {
  const order: Record<Severity, number> = { error: 0, warn: 1, info: 2 };
  const sorted = [...findings].sort(
    (a, b) =>
      Number(b.blocking) - Number(a.blocking) ||
      order[a.severity] - order[b.severity] ||
      (a.req_id ?? "").localeCompare(b.req_id ?? "") ||
      a.kind.localeCompare(b.kind)
  );

  if (sorted.length === 0) return null;

  return (
    <div className="trace-findings-list">
      {sorted.map((f, i) => (
        <div
          key={i}
          className={`trace-finding-row ${f.severity}${f.blocking ? " blocking" : ""}`}
          onClick={() => (f.req_id ? onSelect(f.req_id) : onOpen(f.file, f.line))}
          title={KIND_LABEL[f.kind] ?? f.kind}
        >
          <span className="trace-finding-kind" style={{ color: SEVERITY_COLORS[f.severity] }}>
            {f.blocking ? "\u25C6" : "\u25C7"} {f.kind}
          </span>
          <span className="trace-finding-msg">{f.message}</span>
          <button
            className="trace-finding-goto"
            onClick={(e) => {
              e.stopPropagation();
              onOpen(f.file, f.line);
            }}
            title={`${f.file}:${f.line + 1}`}
          >
            open
          </button>
        </div>
      ))}
    </div>
  );
}

export function TracePanel({
  visible,
  onClose,
  onFileSelect,
  onRequirementFocus,
}: {
  visible: boolean;
  onClose: () => void;
  onFileSelect: (path: string, line?: number) => void;
  /** Told which requirement is selected, so the project graph can light up
      every piece of code that serves it. */
  onRequirementFocus?: (req: string | null) => void;
}) {
  const [tree, setTree] = useState<TreeNode[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<RequirementView | null>(null);
  const [map, setMap] = useState<CoverageMap | null>(null);
  const [zoom, setZoom] = useState(1);
  const [showMap, setShowMap] = useState(false);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<{ points: HistoryPoint[]; problems: string[] } | null>(
    null
  );
  const [showHistory, setShowHistory] = useState(false);
  const [findings, setFindings] = useState<Finding[]>([]);
  const [pasteFor, setPasteFor] = useState<{ req: string; clause: string | null } | null>(null);
  const [pasteText, setPasteText] = useState("");
  const [filter, setFilter] = useState<FindingFilter>("all");
  const [strengthByClause, setStrengthByClause] = useState<Record<string, Strength>>({});

  /** Ask the Lean kernel whether the written obligations are finished. */
  const checkStrength = useCallback(async () => {
    setBusy(true);
    setMessage("Elaborating the spec-strength obligations…");
    try {
      const rows = await invoke<Array<[string, string | null, Strength]>>(
        "trace_strength_check"
      );
      const next: Record<string, Strength> = {};
      for (const [req, clause, state] of rows) next[`${req}.${clause ?? ""}`] = state;
      setStrengthByClause(next);
      const pinned = rows.filter(([, , s]) => s.kind === "pinned").length;
      setMessage(
        `${pinned} of ${rows.length} clause(s) have proofs that determine the model.`
      );
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      // Depth follows the zoom: Z1 shows capabilities, deeper zooms expand.
      const depth = zoom <= 1 ? 0 : undefined;
      setTree(await invoke<TreeNode[]>("trace_tree", { depth }));
      setFindings(await invoke<Finding[]>("trace_findings"));
      // The editor's gutter chips come from the same index.
      window.dispatchEvent(new CustomEvent("tracelean-trace-refresh"));
    } catch (e) {
      setMessage(String(e));
    }
  }, [zoom]);

  useEffect(() => {
    if (visible) refresh();
  }, [visible, refresh]);

  useEffect(() => {
    if (!selected) {
      setDetail(null);
      return;
    }
    invoke<RequirementView>("trace_requirement", { reqId: selected })
      .then(setDetail)
      .catch((e) => setMessage(String(e)));
  }, [selected]);

  // History walks git and indexes each commit, so it is opt-in rather than
  // part of every panel open.
  useEffect(() => {
    if (!showHistory || history) return;
    invoke<{ points: HistoryPoint[]; problems: string[] }>("trace_history", { limit: 30 })
      .then(setHistory)
      .catch((e) => setMessage(String(e)));
  }, [showHistory, history]);

  // The project graph highlights whatever is selected here, so the two views
  // answer the same question from opposite ends: "what serves this
  // requirement" and "what is this code for".
  useEffect(() => {
    onRequirementFocus?.(selected);
  }, [selected, onRequirementFocus]);

  useEffect(() => {
    if (!showMap || map) return;
    invoke<CoverageMap>("trace_coverage_map").then(setMap).catch((e) => setMessage(String(e)));
  }, [showMap, map]);

  /**
   * Copy a self-contained judge prompt, and open the box that takes the reply.
   *
   * The round trip is deliberate rather than a cost-saving hack: the judgement
   * is about whether English and Lean still agree, and an agent that can open
   * the surrounding files does that better than a one-shot API call. The reply
   * is fed back through `judge_apply_reply`, which parses it and executes its
   * witness exactly as the API path does.
   */
  const copyJudgePrompt = async (clause: string | null) => {
    if (!selected) return;
    try {
      const prompt = await invoke<{
        text: string;
        version: string;
        model_runner_available: boolean;
      }>("judge_prompt", { reqId: selected, clause });
      await navigator.clipboard?.writeText(prompt.text);
      setPasteFor({ req: selected, clause });
      setPasteText("");
      setMessage(
        `Judge prompt copied (${prompt.version}). Paste it into an agent, then paste the JSON reply below.` +
          (prompt.model_runner_available
            ? ""
            : " No compiled model runner, so a drift verdict will come back unchecked — build the model first if you want it to count.")
      );
    } catch (e) {
      setMessage(String(e));
    }
  };

  const applyPastedVerdict = async () => {
    if (!pasteFor || !pasteText.trim()) return;
    setBusy(true);
    try {
      const result = await invoke<Record<string, unknown>>("judge_apply_reply", {
        reqId: pasteFor.req,
        clause: pasteFor.clause,
        reply: pasteText,
      });
      setMessage(
        `${result.verdict}` +
          (result.degraded ? " (unchecked: no model runner)" : "") +
          (result.witness_confirmed === false
            ? " — the judge's claim about the model was falsified, so the verdict was discarded"
            : "") +
          ` — ${result.explanation ?? ""}`
      );
      setPasteFor(null);
      setPasteText("");
      await refresh();
    } catch (e) {
      // A malformed reply is rejected here exactly as an API one would be.
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runJudge = async (clause: string | null) => {
    if (!selected) return;
    setBusy(true);
    setMessage(`Judging ${selected}${clause ? "." + clause : ""}…`);
    try {
      const result = await invoke<Record<string, unknown>>("judge_clause", {
        reqId: selected,
        clause,
      });
      const confirmed = result.witness_confirmed;
      setMessage(
        `${result.verdict}` +
          (result.degraded ? " (unchecked: no model runner)" : "") +
          (confirmed === false ? " — the judge's claim about the model was falsified" : "") +
          ` — ${result.explanation ?? ""}`
      );
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runDrt = async (clause: string | null) => {
    if (!selected) return;
    setBusy(true);
    setMessage(`Differential testing ${selected}${clause ? "." + clause : ""}…`);
    try {
      const result = await invoke<{
        cases_run: number;
        divergences: unknown[];
        coverage_floor_met: boolean;
      }>("drt_run", { reqId: selected, clause });
      setMessage(
        result.divergences.length > 0
          ? `${result.divergences.length} divergence(s) over ${result.cases_run} cases — triage before trusting either side`
          : result.coverage_floor_met
          ? `${result.cases_run} cases, no divergences`
          : `${result.cases_run} cases, no divergences — but coverage was too narrow to earn L3`
      );
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Trace-mode keybindings in the editor act on the requirement at the cursor
  // and route here, so `t d` differential-tests what you are looking at rather
  // than what happens to be selected in this panel.
  useEffect(() => {
    const handler = (e: Event) => {
      const eff = (e as CustomEvent).detail as
        | { kind?: string; req_id?: string; clause?: string | null; level?: number; delta?: number }
        | undefined;
      if (!eff) return;
      if (eff.req_id) setSelected(eff.req_id);
      switch (eff.kind) {
        case "zoom":
          // The action vocabulary still speaks of levels 1-5; anything above
          // the last named altitude means "as deep as it goes".
          if (typeof eff.level === "number") setZoom(Math.min(MAX_ZOOM, Math.max(1, eff.level)));
          else if (typeof eff.delta === "number")
            setZoom((z) => Math.min(MAX_ZOOM, Math.max(1, z + (eff.delta as number))));
          break;
        case "coverage_map":
          setShowMap(true);
          break;
        case "show_evidence":
          // Evidence lives in the requirement detail, which the last altitude reveals.
          setZoom(MAX_ZOOM);
          break;
        case "explain_gap":
          setZoom(MAX_ZOOM);
          setMessage(
            eff.req_id
              ? `Showing why ${eff.req_id}${eff.clause ? "." + eff.clause : ""} is not green — see the clause rows below.`
              : ""
          );
          break;
        case "run_judge":
          void runJudge(eff.clause ?? null);
          break;
        case "run_drt":
        case "replay_witness":
          void runDrt(eff.clause ?? null);
          break;
      }
    };
    window.addEventListener("tracelean-trace-action", handler);
    return () => window.removeEventListener("tracelean-trace-action", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected]);

  const counts = useMemo(() => {
    const out: Record<FindingFilter, number> = {
      all: tree.length,
      errors: 0,
      warnings: 0,
      unbound: 0,
      stale: 0,
    };
    for (const f of findings) {
      for (const spec of FILTERS) {
        if (spec.key !== "all" && spec.match(f)) out[spec.key] += 1;
      }
    }
    return out;
  }, [findings, tree.length]);

  const active = FILTERS.find((f) => f.key === filter) ?? FILTERS[0];
  const matching = useMemo(
    () => (filter === "all" ? [] : findings.filter(active.match)),
    [filter, findings, active]
  );

  /**
   * Requirements to keep under the active filter, plus their ancestors.
   *
   * Ancestors are kept so a filtered tree is still a tree — hiding a parent
   * because it has no finding of its own would orphan the child that does, and
   * a filter that appears to delete structure reads as a broken filter.
   */
  const keep = useMemo(() => {
    if (filter === "all") return null;
    const hit = new Set(matching.map((f) => f.req_id).filter(Boolean) as string[]);
    const keepers = new Set<string>();
    const walk = (node: TreeNode, ancestors: string[]): boolean => {
      const childHit = node.children.map((c) => walk(c, [...ancestors, node.req_id])).some(Boolean);
      const self = hit.has(node.req_id);
      if (self || childHit) {
        keepers.add(node.req_id);
        ancestors.forEach((a) => keepers.add(a));
      }
      return self || childHit;
    };
    tree.forEach((node) => walk(node, []));
    return keepers;
  }, [filter, matching, tree]);

  const visibleTree = useMemo(() => {
    if (!keep) return tree;
    const prune = (node: TreeNode): TreeNode | null => {
      if (!keep.has(node.req_id)) return null;
      return { ...node, children: node.children.map(prune).filter(Boolean) as TreeNode[] };
    };
    return tree.map(prune).filter(Boolean) as TreeNode[];
  }, [tree, keep]);

  if (!visible) return null;

  return (
    <div className="requirements-panel trace-panel">
      <div className="panel-header">
        <span className="panel-title">Traceability</span>
        <div className="trace-zoom">
          {ZOOM_LEVELS.map(({ label, hint }, i) => (
            <button
              key={label}
              className={`trace-zoom-btn${zoom === i + 1 ? " active" : ""}`}
              onClick={() => setZoom(i + 1)}
              title={hint}
            >
              {label}
            </button>
          ))}
        </div>
        <button onClick={() => setShowMap(!showMap)}>{showMap ? "list" : "map"}</button>
        <button
          onClick={() => setShowHistory((v) => !v)}
          title="Coverage over recent commits"
        >
          {showHistory ? "hide trend" : "trend"}
        </button>
        <button
          onClick={() => void checkStrength()}
          disabled={busy}
          title="Ask Lean whether the proved properties determine the model, or merely constrain it"
        >
          spec
        </button>
        <button onClick={refresh}>rescan</button>
        <button onClick={onClose}>×</button>
      </div>

      <div className="trace-filters">
        {FILTERS.map((spec) => (
          <button
            key={spec.key}
            className={`trace-filter${filter === spec.key ? " active" : ""}`}
            onClick={() => setFilter(spec.key)}
            title={
              spec.key === "errors"
                ? "Findings the CI gate blocks on \u2014 the same list `block_on` uses"
                : undefined
            }
          >
            {spec.label}
            <span className="trace-filter-count">{counts[spec.key]}</span>
          </button>
        ))}
      </div>

      {filter !== "all" && (
        <FindingsList
          findings={matching}
          onSelect={setSelected}
          onOpen={(file, line) => onFileSelect(file, line)}
        />
      )}

      {showHistory &&
        (history ? (
          <ProgressChart points={history.points} problems={history.problems} />
        ) : (
          <div className="trace-busy">reading git history…</div>
        ))}

      {message && <div className="req-message">{message}</div>}

      {pasteFor && (
        <div className="trace-paste">
          <div className="trace-paste-head">
            Paste the judge's JSON reply for {pasteFor.req}
            {pasteFor.clause ? `.${pasteFor.clause}` : ""}
          </div>
          <textarea
            className="trace-paste-box"
            value={pasteText}
            spellCheck={false}
            placeholder={'{"verdict": "agrees", "explanation": "...", "confidence": "high"}'}
            onChange={(e) => setPasteText(e.target.value)}
          />
          <div className="trace-paste-actions">
            <button onClick={applyPastedVerdict} disabled={!pasteText.trim() || busy}>
              record verdict
            </button>
            <button className="secondary" onClick={() => setPasteFor(null)}>
              cancel
            </button>
          </div>
        </div>
      )}
      {busy && <div className="trace-busy">working…</div>}

      {showMap && map && (
        <CoverageTreemap map={map} onSelect={setSelected} onOpen={(file) => onFileSelect(file)} />
      )}

      <div className="trace-tree">
        {tree.length === 0 ? (
          <div className="req-empty">
            No requirements found. Any markdown file with an <code>id:</code> in its
            frontmatter is a requirement — no particular folder required.
          </div>
        ) : visibleTree.length === 0 ? (
          // Naming the active filter matters: an empty result rendered as "no
          // requirements" is the thing that makes a filter feel broken.
          <div className="req-empty">
            Nothing matches <b>{active.label}</b>. {counts.all} requirement
            {counts.all === 1 ? "" : "s"} in total —{" "}
            <button className="trace-inline-btn" onClick={() => setFilter("all")}>
              show all
            </button>
          </div>
        ) : (
          visibleTree.map((node) => (
            <TreeRow key={node.req_id} node={node} selected={selected} onSelect={setSelected} />
          ))
        )}
      </div>

      {detail && zoom >= MAX_ZOOM && (
        <div className="trace-detail">
          <div className="trace-detail-head">
            <button
              className="trace-open-file"
              onClick={() => onFileSelect(detail.requirement.file)}
            >
              {detail.requirement.id} — {detail.requirement.title}
            </button>
            <CoverageBar
              value={detail.coverage}
              lowerBound={detail.coverage_is_lower_bound}
            />
          </div>

          {detail.clauses.map((clause) => (
            <ClauseRow
              key={clause.key ?? "(requirement)"}
              clause={clause}
              verifiedStrength={
                strengthByClause[`${detail.requirement.id}.${clause.key ?? ""}`]
              }
              onOpen={(file, line) => onFileSelect(file, line)}
              onJudge={runJudge}
              onJudgePrompt={copyJudgePrompt}
              onDrt={runDrt}
            />
          ))}

          {detail.findings.length > 0 && (
            <div className="trace-findings">
              {detail.findings.map((f, i) => (
                <div
                  key={i}
                  className="trace-finding"
                  style={{ color: SEVERITY_COLORS[f.severity] }}
                  onClick={() => onFileSelect(f.file, f.line)}
                >
                  <span className="trace-finding-kind">{f.kind}</span> {f.message}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
