/**
 * The project view: what relates to what.
 *
 * Four columns, left to right, and the horizontal position *is* the claim:
 * a requirement clause is formalized by a model, realized by an
 * implementation, and backed by evidence. Reading the picture left to right is
 * reading the argument the project makes.
 *
 * Every edge here is an annotation somebody wrote. Nothing is inferred, so
 * there is no such thing as a plausible-looking line that turns out to be a
 * guess — the previous view drew reference edges found by matching names in
 * text, which look identical to real ones and are not.
 *
 * Not a force-directed layout: a simulation puts the project wherever the
 * physics lands it, so the same project looks different every time and position
 * means nothing. Not a treemap either — nested boxes answer "how much is
 * untraced", which is a question about proportion, and they answer "what
 * relates to what" very badly.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  COLUMNS,
  COLUMN_TITLE,
  edgePath,
  layoutRoleGraph,
  type Column,
  type Placed,
} from "./roleLayout";

export type Level = "L1" | "L2" | "L3" | "L4";

export const LEVEL_NAME: Record<Level, string> = {
  L1: "claimed",
  L2: "judged",
  L3: "tested",
  L4: "proved",
};

export const LEVEL_MEANING: Record<Level, string> = {
  L1: "L1 claimed — an annotation says these are linked. Nothing has checked it.",
  L2: "L2 judged — an LLM judge agreed the model says what the requirement says, with an executed witness.",
  L3: "L3 tested — differential testing found no disagreement between the model and the code, over a run that cleared its coverage floor.",
  L4: "L4 proved — a machine-checked proof about the model.",
};

const LEVEL_COLOR: Record<Level, string> = {
  L1: "#c05a3a",
  L2: "#c8952f",
  L3: "#4f8f4f",
  L4: "#3f7fbf",
};

/**
 * Edges are deliberately one neutral colour.
 *
 * They used to be coloured by role, and two of those colours were the *same
 * hex* as two assurance levels — `implements` green as L3, `proves` blue as L4.
 * A reader had no way to tell whether a green thing meant "tested" or "this is
 * an implements edge", which is worse than no colour at all. The role is
 * already unambiguous from the columns an edge connects, and the node carries
 * its role badge, so nothing is lost by spending the colour budget entirely on
 * assurance.
 */
const EDGE_COLOR = "#5a6070";
const EDGE_HIGHLIGHT = "#8d94a6";

/**
 * One status palette, used by every dot on every node.
 *
 * Green, amber, red mean the same three things everywhere — working, under way,
 * missing or failing — while *what* they are about depends on the node. On a
 * model the dot is about whether the proofs pin it down; on an implementation,
 * whether its tests pass; on a harness, whether the model and the code agree.
 * Keeping the colours constant and letting the subject change is what makes the
 * picture scannable: red always means "look here", whatever kind of node it is
 * sitting on.
 *
 * `declared` is the fourth state and deliberately not red: a decision somebody
 * made and justified is not a gap.
 */
const STATUS = {
  ok: "#4f8f4f",
  progress: "#c8952f",
  missing: "#c05a3a",
  declared: "#7f7f8c",
} as const;

/** Spec strength, on the model it is a property of. */
const STRENGTH_COLOR: Record<string, string> = {
  pinned: STATUS.ok,
  attempted: STATUS.progress,
  // Red on purpose: nobody has asked whether these proofs determine anything,
  // so the check does not exist. `open` is not a pass.
  open: STATUS.missing,
  nondeterministic: STATUS.declared,
};

/**
 * The word drawn on a model node.
 *
 * Every model gets one, including the ones nobody has asked about: a node with
 * no badge reads as "this question does not apply here", when the truth is
 * "nobody has answered it". That is the same distinction the evidence ladder
 * makes between untraced and L1, and it is worth the pixels.
 */
const STRENGTH_WORD: Record<string, string> = {
  pinned: "pinned",
  attempted: "unproven",
  open: "unasked",
  nondeterministic: "random",
};

const STRENGTH_MEANING: Record<string, string> = {
  pinned:
    "The proved properties determine this model: anything satisfying them is this function.",
  attempted: "A uniqueness obligation is written for this model but not finished.",
  open: "Nobody has asked whether the proved properties determine this model.",
  nondeterministic: "Declared not determined by its inputs, with a reason.",
};

/** Coverage reads as a traffic light, but only where it was measured. */
function coverageColor(fraction: number): string {
  if (fraction >= 0.9) return STATUS.ok;
  if (fraction >= 0.6) return STATUS.progress;
  return STATUS.missing;
}

/** The dot on an implementation: did the tests annotated against it pass? */
function testStatus(tests: TestState | null): { color: string; word: string } {
  if (!tests || tests.claimed === 0) {
    return { color: STATUS.missing, word: "untested" };
  }
  if (tests.passing === null) {
    return { color: STATUS.progress, word: `${tests.claimed} unrun` };
  }
  if (tests.passing < tests.claimed) {
    return {
      color: STATUS.missing,
      word: `${tests.claimed - tests.passing} failing`,
    };
  }
  return { color: STATUS.ok, word: `${tests.passing}/${tests.claimed} pass` };
}

function harnessColor(h: HarnessState): string {
  if (!h.bound) return STATUS.missing;
  if (h.divergences > 0) return STATUS.missing;
  if (h.never_run) return STATUS.progress;
  if (h.stale) return STATUS.progress;
  return STATUS.ok;
}

function harnessWord(h: HarnessState): string {
  if (!h.bound) return "unbound";
  if (h.divergences > 0) return `${h.divergences} diverged`;
  if (h.never_run) return "never run";
  if (h.stale) return "stale";
  return "agreeing";
}

const ROLE_MARK: Record<string, string> = {
  models: "M",
  implements: "I",
  tests: "T",
  drt: "D",
  proves: "P",
  pins: "≡",
};

export interface SpanCoverage {
  covered: number;
  executable: number;
}

export interface TestState {
  claimed: number;
  passing: number | null;
}

export interface HarnessState {
  bound: boolean;
  cases: number;
  divergences: number;
  coverage_covered: number;
  coverage_total: number;
  stale: boolean;
  never_run: boolean;
}

interface NodeFinding {
  kind: string;
  message: string;
  blocking: boolean;
}

export interface RoleNode {
  id: string;
  column: Column;
  label: string;
  sublabel: string;
  file: string;
  start_line: number;
  end_line: number;
  roles: string[];
  assurance: Level | null;
  strength: string | null;
  coverage: SpanCoverage | null;
  tests: TestState | null;
  harness: HarnessState | null;
  stale: boolean;
  exempt: boolean;
  partial: boolean;
  findings: NodeFinding[];
}

export interface RoleEdge {
  from: string;
  to: string;
  role: string;
  stale: boolean;
}

export interface RoleGraphData {
  nodes: RoleNode[];
  edges: RoleEdge[];
  unlinked_clauses: string[];
}

/**
 * Order nodes so an edge is usually a short hop.
 *
 * Requirements first, in their own order; then, for each column, the nodes in
 * the order their requirement appears. Without this, a node's vertical position
 * is whatever the map iteration produced and the picture is a tangle of long
 * diagonals that cross for no reason.
 */
export function orderNodes(data: RoleGraphData): RoleNode[] {
  const byId = new Map(data.nodes.map((n) => [n.id, n]));
  const requirements = data.nodes
    .filter((n) => n.column === "requirement")
    .sort((a, b) => a.id.localeCompare(b.id));

  const rank = new Map<string, number>();
  requirements.forEach((req, i) => rank.set(req.id, i));
  for (const edge of data.edges) {
    const source = rank.get(edge.from);
    if (source === undefined) continue;
    const existing = rank.get(edge.to);
    // A declaration serving several clauses sits beside the first of them.
    if (existing === undefined || source < existing) rank.set(edge.to, source);
  }

  const ordered: RoleNode[] = [];
  for (const column of COLUMNS) {
    const inColumn = data.nodes
      .filter((n) => n.column === column)
      .sort((a, b) => {
        const ra = rank.get(a.id) ?? Number.MAX_SAFE_INTEGER;
        const rb = rank.get(b.id) ?? Number.MAX_SAFE_INTEGER;
        return ra - rb || a.label.localeCompare(b.label);
      });
    ordered.push(...inColumn.filter((n) => byId.has(n.id)));
  }
  return ordered;
}

function nodeColor(node: RoleNode): string {
  if (node.exempt) return "#3a3f4b";
  // A harness is coloured by what it is doing, not by the level it earned: a
  // binding that has never run has no level at all, and grey would read as
  // "untraced" when the truth is "bound, unexercised".
  if (node.harness) return harnessColor(node.harness);
  return node.assurance ? LEVEL_COLOR[node.assurance] : "#3a3f4b";
}

function NodeDetail({
  node,
  onOpen,
  onRequirement,
}: {
  node: RoleNode;
  onOpen: (file: string, line?: number) => void;
  onRequirement: (req: string) => void;
}) {
  const isRequirement = node.column === "requirement";
  return (
    <div className="graph-details">
      <div className="graph-details-head">
        <button
          className="graph-open"
          onClick={() => onOpen(node.file, isRequirement ? undefined : node.start_line)}
        >
          {isRequirement ? node.id : `${node.file}:${node.start_line + 1}`}
        </button>
        {isRequirement && (
          <button className="graph-open" onClick={() => onRequirement(node.id)}>
            show in traceability
          </button>
        )}
      </div>
      <div className="graph-details-body">
        {node.sublabel && <div className="graph-details-sub">{node.sublabel}</div>}
        {node.assurance && (
          <div title={LEVEL_MEANING[node.assurance]}>
            assurance <strong>{node.assurance} {LEVEL_NAME[node.assurance]}</strong> — the
            weakest bond in the chain, never the average
          </div>
        )}
        {node.column === "model" && (
          <div title={STRENGTH_MEANING[node.strength ?? "open"]}>
            spec strength{" "}
            <strong>{STRENGTH_WORD[node.strength ?? "open"] ?? "unasked"}</strong> —{" "}
            {STRENGTH_MEANING[node.strength ?? "open"]}
          </div>
        )}
        {node.coverage && (
          <div>
            line coverage{" "}
            <strong>
              {node.coverage.covered}/{node.coverage.executable}
            </strong>{" "}
            executable lines
            {node.coverage.executable === 0 && " (nothing executable here)"}
          </div>
        )}
        {node.tests && (
          <div>
            {node.tests.claimed} test{node.tests.claimed === 1 ? "" : "s"} annotated against
            this code
            {node.tests.passing !== null && `, ${node.tests.passing} passing`}
          </div>
        )}
        {node.harness && (
          <div>
            differential testing: <strong>{harnessWord(node.harness)}</strong>
            {node.harness.bound
              ? ` — ${node.harness.cases} cases, ${node.harness.divergences} divergence(s), input coverage ${node.harness.coverage_covered}/${node.harness.coverage_total}`
              : " — no binding in .tracelean/drt.json, so nothing checks this model against the code"}
          </div>
        )}
        {node.roles.length > 0 && <div>roles: {node.roles.join(", ")}</div>}
        {node.partial && <div>claims only part of its clause</div>}
        {node.exempt && <div>deliberately outside the model</div>}
        {node.stale && (
          <div className="graph-stale">
            the annotated body changed after the evidence was taken
          </div>
        )}
        {node.findings.map((f, i) => (
          <div key={i} className={f.blocking ? "graph-finding blocking" : "graph-finding"}>
            {f.kind}: {f.message}
          </div>
        ))}
      </div>
    </div>
  );
}

export function ProjectGraph({
  visible,
  onClose,
  onFileSelect,
  highlightRequirement,
  onRequirementSelect,
}: {
  visible: boolean;
  onClose: () => void;
  onFileSelect: (file: string, line?: number) => void;
  /** Requirement to highlight, driven from the trace panel. */
  highlightRequirement?: string | null;
  onRequirementSelect?: (req: string) => void;
}) {
  const [data, setData] = useState<RoleGraphData | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [width, setWidth] = useState(720);
  const box = useRef<HTMLDivElement | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await invoke<RoleGraphData>("trace_role_graph", {}));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (visible) void load();
  }, [visible, load]);

  useEffect(() => {
    if (!visible || !box.current) return;
    const element = box.current;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    setWidth(element.clientWidth);
    observer.observe(element);
    return () => observer.disconnect();
  }, [visible]);

  const ordered = useMemo(() => (data ? orderNodes(data) : []), [data]);
  const layout = useMemo(
    // Taller than the default: a code node carries a name, a file, a coverage
    // bar and a test count, and four rows do not fit in 34 pixels.
    () => layoutRoleGraph(ordered, { width, nodeHeight: 46 }),
    [ordered, width]
  );
  const nodeIndex = useMemo(
    () => new Map(ordered.map((n) => [n.id, n])),
    [ordered]
  );

  /** Which nodes the highlighted requirement reaches, transitively. */
  const lit = useMemo(() => {
    if (!data) return null;
    const roots = data.nodes
      .filter(
        (n) =>
          n.column === "requirement" &&
          (selected === n.id ||
            (!!highlightRequirement &&
              (n.id === highlightRequirement ||
                n.id.startsWith(`${highlightRequirement}.`))))
      )
      .map((n) => n.id);
    if (roots.length === 0) return null;
    const reached = new Set(roots);
    // Breadth-first rather than one hop: an implementation reached through a
    // model is still part of what this requirement rests on.
    let frontier = roots;
    while (frontier.length) {
      const next: string[] = [];
      for (const edge of data.edges) {
        if (frontier.includes(edge.from) && !reached.has(edge.to)) {
          reached.add(edge.to);
          next.push(edge.to);
        }
      }
      frontier = next;
    }
    return reached;
  }, [data, highlightRequirement, selected]);

  if (!visible) return null;

  const detail = selected ? nodeIndex.get(selected) : undefined;

  return (
    <div className="project-graph">
      <div className="panel-header">
        <span className="panel-title">Project</span>
        <button onClick={() => void load()} title="Rescan">
          rescan
        </button>
        <button onClick={onClose}>x</button>
      </div>

      {error ? (
        <div className="graph-error">{error}</div>
      ) : !data ? (
        <div className="graph-empty">scanning…</div>
      ) : ordered.length === 0 ? (
        <div className="graph-empty">
          Nothing is annotated yet, so there is nothing to relate.
        </div>
      ) : (
        <>
          <div className="graph-canvas" ref={box}>
            <svg width={width} height={layout.height}>
              {COLUMNS.map((column) => (
                <text
                  key={column}
                  className="graph-column-title"
                  x={layout.columnX[column]}
                  y={12}
                  fill="#888"
                  fontSize={10}
                >
                  {COLUMN_TITLE[column]}
                </text>
              ))}

              {data.edges.map((edge, i) => {
                const from = layout.placed.get(edge.from);
                const to = layout.placed.get(edge.to);
                if (!from || !to) return null;
                const dim = lit ? !(lit.has(edge.from) && lit.has(edge.to)) : false;
                return (
                  <path
                    key={i}
                    d={edgePath(from as Placed, to as Placed)}
                    fill="none"
                    stroke={dim ? EDGE_COLOR : EDGE_HIGHLIGHT}
                    strokeWidth={dim ? 1 : 1.6}
                    strokeOpacity={dim ? 0.18 : 0.9}
                    strokeDasharray={edge.stale ? "3 3" : undefined}
                  >
                    <title>
                      {`@${edge.role}  ${edge.from} → ${edge.to}${
                        edge.stale ? "\n(stale: the body changed after the evidence)" : ""
                      }`}
                    </title>
                  </path>
                );
              })}

              {ordered.map((node) => {
                const place = layout.placed.get(node.id);
                if (!place) return null;
                const dim = lit ? !lit.has(node.id) : false;
                return (
                  <g
                    key={node.id}
                    opacity={dim ? 0.25 : 1}
                    onClick={() => setSelected(node.id === selected ? null : node.id)}
                    style={{ cursor: "pointer" }}
                  >
                    <rect
                      x={place.x}
                      y={place.y}
                      width={place.w}
                      height={place.h}
                      rx={3}
                      fill="#23262d"
                      stroke={node.id === selected ? "#ddd" : nodeColor(node)}
                      strokeWidth={node.id === selected ? 2 : 1.4}
                    />
                    <rect
                      x={place.x}
                      y={place.y}
                      width={4}
                      height={place.h}
                      fill={nodeColor(node)}
                    />
                    <text
                      x={place.x + 10}
                      y={place.y + 14}
                      fill="#ddd"
                      fontSize={11}
                      className="graph-label"
                    >
                      {node.label}
                    </text>
                    <text
                      x={place.x + 10}
                      y={place.y + 26}
                      fill="#888"
                      fontSize={9}
                      className="graph-label"
                    >
                      {node.sublabel}
                    </text>
                    <text
                      x={place.x + place.w - 6}
                      y={place.y + 14}
                      fill="#999"
                      fontSize={9}
                      textAnchor="end"
                    >
                      {node.roles.map((r) => ROLE_MARK[r] ?? "?").join(" ")}
                    </text>

                    {/* Spec strength is a property of the model, so it is drawn
                        on the model rather than only on the requirement that
                        points at it. */}
                    {node.column === "model" && (
                      <>
                        <circle
                          cx={place.x + place.w - 12}
                          cy={place.y + place.h - 10}
                          r={4}
                          fill={STRENGTH_COLOR[node.strength ?? "open"] ?? "#8a8f99"}
                        />
                        <text
                          x={place.x + place.w - 20}
                          y={place.y + place.h - 6}
                          fill="#9aa0aa"
                          fontSize={8}
                          textAnchor="end"
                        >
                          {STRENGTH_WORD[node.strength ?? "open"] ?? "unasked"}
                        </text>
                      </>
                    )}

                    {/* Coverage is a property of the code: how much of this
                        declaration the tests actually executed. */}
                    {node.coverage && node.coverage.executable > 0 && (
                      <>
                        <rect
                          x={place.x + 10}
                          y={place.y + place.h - 9}
                          width={place.w - 80}
                          height={3}
                          fill="#2c2f36"
                        />
                        <rect
                          x={place.x + 10}
                          y={place.y + place.h - 9}
                          width={
                            (place.w - 80) *
                            (node.coverage.covered / node.coverage.executable)
                          }
                          height={3}
                          fill={coverageColor(
                            node.coverage.covered / node.coverage.executable
                          )}
                        />
                        <text
                          x={place.x + place.w - 62}
                          y={place.y + place.h - 5}
                          fill="#9aa0aa"
                          fontSize={8}
                        >
                          {Math.round(
                            (node.coverage.covered / node.coverage.executable) * 100
                          )}
                          %
                        </text>
                      </>
                    )}

                    {/* Tests are a property of the implementation they
                        exercise, counted on it rather than scattered across a
                        column of their own. */}
                    {node.column === "implementation" && (
                      <>
                        <circle
                          cx={place.x + place.w - 12}
                          cy={place.y + place.h - 10}
                          r={4}
                          fill={testStatus(node.tests).color}
                        />
                        <text
                          x={place.x + place.w - 20}
                          y={place.y + place.h - 6}
                          fill="#9aa0aa"
                          fontSize={8}
                          textAnchor="end"
                        >
                          {testStatus(node.tests).word}
                        </text>
                      </>
                    )}

                    {/* The harness had no node at all before, which is why the
                        state of the thing binding model to code was invisible
                        in this picture. */}
                    {node.harness && (
                      <>
                        <circle
                          cx={place.x + place.w - 12}
                          cy={place.y + place.h - 10}
                          r={4}
                          fill={harnessColor(node.harness)}
                        />
                        <text
                          x={place.x + place.w - 20}
                          y={place.y + place.h - 6}
                          fill="#9aa0aa"
                          fontSize={8}
                          textAnchor="end"
                        >
                          {harnessWord(node.harness)}
                        </text>
                      </>
                    )}

                    <title>
                      {`${node.id}${
                        node.assurance ? `\n${LEVEL_MEANING[node.assurance]}` : ""
                      }${node.stale ? "\nstale: the body changed after the evidence" : ""}`}
                    </title>
                  </g>
                );
              })}
            </svg>
          </div>

          <div className="graph-legend">
            {(["L1", "L2", "L3", "L4"] as const).map((level) => (
              <span key={level} title={LEVEL_MEANING[level]}>
                <i style={{ background: LEVEL_COLOR[level] }} />
                {LEVEL_NAME[level]}
              </span>
            ))}
            <span className="graph-legend-note">node border = assurance</span>
            <span title="Green, amber and red mean the same three things on every node — working, under way, missing or failing — while what they are about depends on the node.">
              <i style={{ background: STATUS.ok }} />
              <i style={{ background: STATUS.progress }} />
              <i style={{ background: STATUS.missing }} />
              dot = status
            </span>
            <span title="Whether the properties proved about this model determine it: pinned (proved), unproven (obligation written, unfinished), unasked (nobody has asked — not a pass), random (declared not determined by its inputs, with a reason).">
              on a model: pinned / unproven / unasked / random
            </span>
            <span title="Whether the tests annotated against this code passed. Untested is red: a claim with nothing behind it.">
              on code: tests passing, plus a line-coverage bar
            </span>
            <span title="Whether differential testing found the model and the code to agree: agreeing, never run, stale, N diverged, or unbound — nothing checks this model against the code at all.">
              on a harness: agreeing / never run / diverged / unbound
            </span>
            <span title="Line coverage of the declaration, from the language's own tool. Absent where nothing measured the file — which is not the same as zero.">
              bar under a code node = coverage
            </span>
            <span title="A dashed edge means the annotated body changed after the evidence was recorded. Edges are one colour on purpose: the role is already unambiguous from the columns an edge joins, and two role colours used to be identical to two assurance colours.">
              dashed edge = stale
            </span>
          </div>

          {data.unlinked_clauses.length > 0 && (
            <div className="graph-omitted">
              {data.unlinked_clauses.length} clause
              {data.unlinked_clauses.length === 1 ? "" : "s"} with no annotation at all, so
              nothing to draw: {data.unlinked_clauses.join(", ")}
            </div>
          )}

          {detail && (
            <NodeDetail
              node={detail}
              onOpen={onFileSelect}
              onRequirement={(req) => onRequirementSelect?.(req)}
            />
          )}
        </>
      )}
    </div>
  );
}
