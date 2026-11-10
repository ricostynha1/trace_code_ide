// @implements REQ-LOOK.overview_drawn
//
// The design as figures. Everything drawn here is computed from the nodes the editor
// hands over, the same ones the design buffer is produced from; nothing is invented.

export interface Tally {
  implemented: number;
  annotated: number;
  judged: number;
  tested: number;
  proved: number;
}

export interface Node {
  id: string;
  title: string;
  refines: string[];
  clauses: number;
  modelled: number;
  held: Tally;
  covered: [number, number] | null;
}

export interface Run {
  op: string;
  clauses: number;
  seed: number;
  cases: number;
  age: number | null;
}

export interface Impact {
  reqId: string;
  clause: string | null;
  file: string;
  marks: string;
}

export interface Module {
  name: string;
  files: number;
  uses: string[];
  serves: string[];
}

export interface Step {
  order: number;
  kind: string;
  file: string;
  added: number;
  removed: number;
  clauses: [string, string][];
}

export interface Design {
  nodes: Node[];
  problems: number | null;
  runs: Run[];
  impact: Impact[];
  modules?: Module[];
  steps?: Step[];
}

export interface Handlers {
  open: (id: string) => void;
  close: () => void;
  act: (action: string) => void;
  /** Puts the overview, as Markdown, on the clipboard; resolves to whether it did. */
  copy?: () => Promise<boolean>;
  card: (anchor: Element, name: string) => void;
  uncard: () => void;
}

export interface Showing {
  filter: "all" | "left" | "unimplemented" | "risk";
  root: string;
  query: string;
  as: "graph" | "treemap";
}

/** What the reader last chose; it outlives closing the overview. */
export const showing: Showing = { filter: "all", root: "", query: "", as: "graph" };

const MARKS: [keyof Tally, string, string][] = [
  ["implemented", "I", "Implemented"],
  ["annotated", "A", "Annotated"],
  ["judged", "J", "Judged"],
  ["tested", "T", "Tested"],
  ["proved", "P", "Proved"],
];

const SVG = "http://www.w3.org/2000/svg";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const made = document.createElement(tag);
  if (className) made.className = className;
  if (text) made.textContent = text;
  return made;
}

function svg(tag: string, attributes: Record<string, string | number>, text = ""): SVGElement {
  const made = document.createElementNS(SVG, tag);
  for (const [name, value] of Object.entries(attributes)) made.setAttribute(name, String(value));
  if (text) made.textContent = text;
  return made;
}

const percent = (part: number, whole: number) => (whole ? Math.round((100 * part) / whole) : 0);

/** How far a requirement is: `full`, `some` or `none` of its clauses hold the mark. */
export function state(part: number, whole: number): "full" | "some" | "none" | "empty" {
  if (!whole) return "empty";
  return part >= whole ? "full" : part > 0 ? "some" : "none";
}

/** The row each requirement is drawn on: one below the lowest requirement it refines. */
export function layers(nodes: Node[]): Map<string, number> {
  const known = new Map(nodes.map((node) => [node.id, node]));
  const depth = new Map<string, number>();
  const visit = (id: string, seen: Set<string>): number => {
    const done = depth.get(id);
    if (done !== undefined) return done;
    if (seen.has(id)) return 0;
    seen.add(id);
    const parents = (known.get(id)?.refines ?? []).filter((parent) => known.has(parent));
    const at = parents.length ? 1 + Math.max(...parents.map((parent) => visit(parent, seen))) : 0;
    depth.set(id, at);
    return at;
  };
  for (const node of nodes) visit(node.id, new Set());
  return depth;
}

/** A requirement and everything that refines it, however far down. */
export function under(nodes: Node[], root: string): Set<string> {
  const found = new Set([root]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const node of nodes) {
      if (!found.has(node.id) && node.refines.some((parent) => found.has(parent))) {
        found.add(node.id);
        grew = true;
      }
    }
  }
  return found;
}

/** How many of a requirement's clause-marks are still missing. */
export const left = (node: Node) =>
  MARKS.reduce((sum, [key]) => sum + (node.clauses - Math.min(node.clauses, node.held[key])), 0);

/**
 * The requirements the filters leave: under the chosen root, matching the words typed,
 * and of the kind asked for. `risk` names the requirements an unsaved edit touches.
 *
 * @implements REQ-FIGURE.graph_filtered
 */
export function filtered(nodes: Node[], by: Showing, risk: Set<string>): Node[] {
  const within = by.root ? under(nodes, by.root) : null;
  const words = by.query.toLowerCase().split(/\s+/).filter(Boolean);
  return nodes.filter((node) => {
    if (within && !within.has(node.id)) return false;
    const hay = `${node.id} ${node.title}`.toLowerCase();
    if (!words.every((word) => hay.includes(word))) return false;
    if (by.filter === "left") return left(node) > 0;
    if (by.filter === "unimplemented") return node.held.implemented < node.clauses;
    if (by.filter === "risk") return risk.has(node.id);
    return true;
  });
}

export interface Tile {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * A treemap: each item a rectangle whose area is its share of the whole, the rectangles
 * tiling the box with none overlapping. The list is halved by weight along the longer side.
 *
 * @implements REQ-FIGURE.treemap_areas
 */
export function treemap(items: { id: string; size: number }[], x: number, y: number, w: number, h: number): Tile[] {
  const sized = items.filter((item) => item.size > 0).sort((a, b) => b.size - a.size || a.id.localeCompare(b.id));
  if (!sized.length) return [];
  if (sized.length === 1) return [{ id: sized[0].id, x, y, w, h }];
  const total = sized.reduce((sum, item) => sum + item.size, 0);
  let cut = 1;
  let first = sized[0].size;
  while (cut < sized.length - 1 && first + sized[cut].size / 2 < total / 2) {
    first += sized[cut].size;
    cut += 1;
  }
  const share = first / total;
  const [one, two] = [sized.slice(0, cut), sized.slice(cut)];
  return w >= h
    ? [...treemap(one, x, y, w * share, h), ...treemap(two, x + w * share, y, w * (1 - share), h)]
    : [...treemap(one, x, y, w, h * share), ...treemap(two, x, y + h * share, w, h * (1 - share))];
}

/** How much of everything a requirement could hold it holds, as one of four steps. */
export function step(node: Node): 0 | 1 | 2 | 3 {
  const all = MARKS.length * node.clauses;
  if (!all) return 0;
  const share = (all - left(node)) / all;
  return share >= 1 ? 3 : share >= 0.6 ? 2 : share >= 0.3 ? 1 : 0;
}

function tiles(nodes: Node[], problems: number | null): HTMLElement {
  const row = el("div", "ov-tiles");
  const clauses = nodes.reduce((sum, node) => sum + node.clauses, 0);
  const run = nodes.reduce((sum, node) => sum + (node.covered?.[0] ?? 0), 0);
  const all = nodes.reduce((sum, node) => sum + (node.covered?.[1] ?? 0), 0);
  const done = nodes.filter((node) => MARKS.every(([key]) => node.held[key] >= node.clauses)).length;
  const tile = (value: string, label: string, kind = "") => {
    const box = el("div", `ov-tile ${kind}`);
    box.append(el("div", "ov-value", value), el("div", "ov-label", label));
    row.appendChild(box);
  };
  tile(String(nodes.length), "requirements");
  tile(String(clauses), "clauses");
  tile(`${done}/${nodes.length}`, "requirements with every mark", done === nodes.length ? "good" : "");
  tile(all ? `${percent(run, all)}%` : "–", "lines run by tests", all ? state(run, all) : "");
  tile(problems === null ? "–" : String(problems), "findings", problems ? "none" : "good");
  return row;
}

function funnel(nodes: Node[]): HTMLElement {
  const box = el("div", "ov-funnel");
  const clauses = nodes.reduce((sum, node) => sum + node.clauses, 0);
  for (const [key, letter, name] of MARKS) {
    const held = nodes.reduce((sum, node) => sum + node.held[key], 0);
    const row = el("div", "ov-bar-row");
    const track = el("div", "ov-track");
    const fill = el("div", `ov-fill mark-${letter}`);
    fill.style.width = `${percent(held, clauses)}%`;
    track.appendChild(fill);
    row.append(el("span", "ov-bar-name", `${letter}  ${name}`), track, el("span", "ov-bar-count", `${held}/${clauses} · ${percent(held, clauses)}%`));
    row.title = `${held} of ${clauses} clauses are ${name.toLowerCase()}`;
    box.appendChild(row);
  }
  return box;
}

const [WIDE, TALL, GAP_X, GAP_Y, PER_ROW] = [168, 50, 18, 46, 7];

function graph(nodes: Node[], on: Handlers, risk: Set<string>): HTMLElement {
  const holder = el("div", "ov-graph");
  const depth = layers(nodes);
  const byLayer = new Map<number, Node[]>();
  for (const node of nodes) {
    const at = depth.get(node.id) ?? 0;
    byLayer.set(at, [...(byLayer.get(at) ?? []), node]);
  }
  const placed = new Map<string, { x: number; y: number }>();
  let row = 0;
  let widest = 1;
  for (const at of [...byLayer.keys()].sort((a, b) => a - b)) {
    const middle = (node: Node) => {
      const xs = node.refines.map((parent) => placed.get(parent)?.x).filter((x): x is number => x !== undefined);
      return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : Number.MAX_SAFE_INTEGER;
    };
    const line = [...(byLayer.get(at) ?? [])].sort((a, b) => middle(a) - middle(b) || a.id.localeCompare(b.id));
    for (let start = 0; start < line.length; start += PER_ROW) {
      const part = line.slice(start, start + PER_ROW);
      widest = Math.max(widest, part.length);
      part.forEach((node, index) => placed.set(node.id, { x: index, y: row }));
      row += 1;
    }
  }
  const width = widest * (WIDE + GAP_X) + GAP_X;
  const height = row * (TALL + GAP_Y) + GAP_Y / 2;
  for (const [id, at] of placed) {
    const inRow = [...placed.values()].filter((other) => other.y === at.y).length;
    const left = (width - inRow * (WIDE + GAP_X) + GAP_X) / 2;
    placed.set(id, { x: left + at.x * (WIDE + GAP_X), y: GAP_Y / 2 + at.y * (TALL + GAP_Y) });
  }
  const drawing = svg("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "img", "aria-label": "the requirements as their refinement graph" });
  const edges = svg("g", { class: "ov-edges" });
  const boxes = svg("g", {});
  drawing.append(edges, boxes);
  for (const node of nodes) {
    const at = placed.get(node.id);
    if (!at) continue;
    for (const parent of node.refines) {
      const from = placed.get(parent);
      if (!from) continue;
      const [x1, y1, x2, y2] = [from.x + WIDE / 2, from.y + TALL, at.x + WIDE / 2, at.y];
      const bend = (y2 - y1) / 2;
      const edge = svg("path", { d: `M${x1},${y1} C${x1},${y1 + bend} ${x2},${y2 - bend} ${x2},${y2}`, "data-from": parent, "data-to": node.id });
      edges.appendChild(edge);
    }
    const share = node.clauses ? node.held.implemented / node.clauses : 0;
    const worst = MARKS.map(([key]) => state(node.held[key], node.clauses));
    const kind = worst.every((s) => s === "full") ? "full" : node.held.implemented ? "some" : "none";
    const group = svg("g", { class: `ov-node ${kind}${risk.has(node.id) ? " risk" : ""}`, transform: `translate(${at.x},${at.y})`, tabindex: 0, role: "button", "data-id": node.id });
    group.appendChild(svg("rect", { class: "ov-box", width: WIDE, height: TALL, rx: 7 }));
    group.appendChild(svg("rect", { class: "ov-progress", x: 0, y: TALL - 5, width: Math.max(0, WIDE * share), height: 5, rx: 2 }));
    const name = node.id.length > 20 ? `${node.id.slice(0, 19)}…` : node.id;
    group.appendChild(svg("text", { class: "ov-id", x: 10, y: 19 }, name));
    MARKS.slice(1).forEach(([key, letter], index) => {
      const cx = 16 + index * 22;
      group.appendChild(svg("circle", { class: `ov-dot mark-${letter} ${state(node.held[key], node.clauses)}`, cx, cy: 33, r: 5.5 }));
    });
    group.appendChild(svg("text", { class: "ov-count", x: WIDE - 10, y: 37, "text-anchor": "end" }, `${node.held.implemented}/${node.clauses}`));
    const lit = (on: boolean) => {
      for (const edge of edges.querySelectorAll<SVGElement>("path")) {
        const touches = edge.dataset.from === node.id || edge.dataset.to === node.id;
        edge.classList.toggle("lit", on && touches);
      }
    };
    group.addEventListener("mouseenter", () => {
      lit(true);
      on.card(group, node.id);
    });
    group.addEventListener("mouseleave", () => {
      lit(false);
      on.uncard();
    });
    group.addEventListener("click", () => on.open(node.id));
    group.addEventListener("keydown", (event) => {
      if ((event as KeyboardEvent).key === "Enter") on.open(node.id);
    });
    boxes.appendChild(group);
  }
  holder.appendChild(drawing);
  return holder;
}

function tree(nodes: Node[], on: Handlers, risk: Set<string>): HTMLElement {
  const holder = el("div", "ov-graph");
  const [width, height] = [1100, Math.min(520, 160 + nodes.length * 12)];
  const drawing = svg("svg", { viewBox: `0 0 ${width} ${height}`, width: "100%", role: "img", "aria-label": "the requirements as a treemap: area is clauses, colour is how much is held" });
  const byId = new Map(nodes.map((node) => [node.id, node]));
  for (const tile of treemap(nodes.map((node) => ({ id: node.id, size: Math.max(1, node.clauses) })), 0, 0, width, height)) {
    const node = byId.get(tile.id);
    if (!node) continue;
    const group = svg("g", { class: `ov-tilebox step-${step(node)}${risk.has(node.id) ? " risk" : ""}`, transform: `translate(${tile.x},${tile.y})`, tabindex: 0, role: "button", "data-id": node.id });
    group.appendChild(svg("rect", { width: Math.max(0, tile.w - 2), height: Math.max(0, tile.h - 2), x: 1, y: 1, rx: 4 }));
    if (tile.w > 70 && tile.h > 22) {
      const fits = Math.max(3, Math.floor((tile.w - 12) / 7.4));
      group.appendChild(svg("text", { class: "ov-id", x: 7, y: 17 }, node.id.length > fits ? `${node.id.slice(0, fits - 1)}…` : node.id));
      if (tile.h > 40) group.appendChild(svg("text", { class: "ov-count", x: 7, y: 33 }, `${node.clauses} clauses · ${left(node)} marks left`));
    }
    group.addEventListener("mouseenter", () => on.card(group, node.id));
    group.addEventListener("mouseleave", () => on.uncard());
    group.addEventListener("click", () => on.open(node.id));
    drawing.appendChild(group);
  }
  holder.appendChild(drawing);
  return holder;
}

function controls(design: Design, redraw: () => void): HTMLElement {
  const bar = el("div", "ov-controls");
  const pick = (label: string, title: string, on: boolean, choose: () => void) => {
    const button = el("button", `ov-pick${on ? " on" : ""}`, label);
    button.title = title;
    button.addEventListener("click", () => {
      choose();
      redraw();
    });
    bar.appendChild(button);
  };
  const kinds: [Showing["filter"], string, string][] = [
    ["all", "All", "every requirement"],
    ["left", "Work left", "requirements with a clause missing a mark"],
    ["unimplemented", "Not implemented", "requirements with a clause nothing implements"],
    ["risk", `Touched by unsaved edits (${new Set(design.impact.map((i) => i.reqId)).size})`, "requirements an unsaved file claims"],
  ];
  for (const [kind, label, title] of kinds) pick(label, title, showing.filter === kind, () => (showing.filter = kind));
  const roots = design.nodes.filter((node) => !node.refines.some((parent) => design.nodes.some((other) => other.id === parent)));
  const root = el("select", "ov-root");
  root.title = "show only what refines this root";
  root.setAttribute("aria-label", "root");
  for (const [value, label] of [["", "every root"], ...roots.map((node) => [node.id, `under ${node.id}`])]) {
    const option = el("option", "", label);
    option.value = value;
    option.selected = showing.root === value;
    root.appendChild(option);
  }
  root.addEventListener("change", () => {
    showing.root = root.value;
    redraw();
  });
  const search = el("input", "ov-search");
  search.placeholder = "filter by name…";
  search.setAttribute("aria-label", "filter by name");
  search.value = showing.query;
  search.addEventListener("keydown", (event) => event.stopPropagation());
  search.addEventListener("input", () => {
    showing.query = search.value;
    redraw();
    const again = document.querySelector<HTMLInputElement>(".ov-search");
    again?.focus();
    again?.setSelectionRange(again.value.length, again.value.length);
  });
  bar.append(root, search, el("span", "ov-gap"));
  pick("Graph", "the refinement graph", showing.as === "graph", () => (showing.as = "graph"));
  pick("Treemap", "area: clauses · colour: how much is held", showing.as === "treemap", () => (showing.as = "treemap"));
  return bar;
}

/** Seconds as the largest unit that fits: `3 min`, `2 h`, `5 d`. */
export function ago(seconds: number | null): string {
  if (seconds === null) return "–";
  const steps: [number, string][] = [[86400, "d"], [3600, "h"], [60, "min"]];
  for (const [size, unit] of steps) if (seconds >= size) return `${Math.floor(seconds / size)} ${unit} ago`;
  return "just now";
}

function runs(design: Design, on: Handlers): HTMLElement {
  const box = el("div", "ov-runs");
  const tools = el("div", "ov-controls");
  for (const [label, action, title] of [
    ["Run differential tests", "drt.run", "compare every implementation with its model on generated cases"],
    ["Check the tree", "trace.check", "re-read every annotation and report what is missing"],
    ["Measure coverage", "drt.coverage", "which lines the tests run"],
  ]) {
    const button = el("button", "ov-pick", label);
    button.title = title;
    button.addEventListener("click", () => {
      button.textContent = "running…";
      button.disabled = true;
      on.act(action);
    });
    tools.appendChild(button);
  }
  box.appendChild(tools);
  if (!design.runs.length) {
    box.appendChild(el("p", "ov-empty", "No differential run is on record yet."));
    return box;
  }
  const table = el("table", "ov-matrix");
  const head = el("tr");
  for (const title of ["Compared operation", "Clauses it carries", "Cases", "Seed", "Recorded"]) head.appendChild(el("th", "", title));
  table.appendChild(head);
  for (const run of design.runs) {
    const row = el("tr");
    row.append(el("td", "ov-req", run.op), el("td", "", String(run.clauses)), el("td", "", run.cases.toLocaleString("en")), el("td", "", String(run.seed)), el("td", "ov-title", ago(run.age)));
    row.addEventListener("click", () => on.open(run.op.split(".")[0]));
    row.addEventListener("mouseenter", () => on.card(row, run.op));
    row.addEventListener("mouseleave", () => on.uncard());
    table.appendChild(row);
  }
  box.appendChild(table);
  return box;
}

function impact(design: Design, on: Handlers): HTMLElement {
  const table = el("table", "ov-matrix");
  const head = el("tr");
  for (const title of ["Clause", "Holds now", "Claimed by the unsaved file"]) head.appendChild(el("th", "", title));
  table.appendChild(head);
  for (const hit of design.impact) {
    const name = hit.clause ? `${hit.reqId}.${hit.clause}` : hit.reqId;
    const row = el("tr");
    const held = el("td", `ov-cell ${/[JTP]/.test(hit.marks) ? "some" : "empty"}`, hit.marks);
    held.title = /[JTP]/.test(hit.marks) ? "saving puts these marks in question until the evidence is redone" : "nothing earned yet, so nothing to lose";
    row.append(el("td", "ov-req", name), held, el("td", "ov-title", hit.file));
    row.addEventListener("click", () => on.open(hit.reqId));
    row.addEventListener("mouseenter", () => on.card(row, name));
    row.addEventListener("mouseleave", () => on.uncard());
    table.appendChild(row);
  }
  return table;
}

/**
 * What the agent changed and has waiting, a row per file in the order the changes came.
 *
 * @implements REQ-FIGURE.agent_steps_listed
 */
function steps(list: Step[], on: Handlers): HTMLElement {
  const table = el("table", "ov-matrix");
  const head = el("tr");
  for (const title of ["Step", "Change", "File", "Lines", "Clauses the file claims · marks held"]) head.appendChild(el("th", "", title));
  table.appendChild(head);
  for (const step of list) {
    const row = el("tr");
    const claimed = el("td", "ov-title");
    if (!step.clauses.length) claimed.textContent = "claims no clause";
    for (const [name, marks] of step.clauses) {
      const chip = el("span", `ov-chip ${/[JTP]/.test(marks) ? "some" : "empty"}`, `${name} ${marks}`);
      chip.title = /[JTP]/.test(marks) ? "accepting this change puts these marks in question" : "nothing earned yet, so nothing to lose";
      chip.addEventListener("mouseenter", () => on.card(chip, name));
      chip.addEventListener("mouseleave", () => on.uncard());
      claimed.appendChild(chip);
    }
    row.append(el("td", "", String(step.order)), el("td", "", step.kind), el("td", "ov-req", step.file), el("td", "", `+${step.added} −${step.removed}`), claimed);
    table.appendChild(row);
  }
  return table;
}

/** Each module's place in the layering: 0 uses nothing, others sit above what they use. */
export function depth(list: Module[]): Map<string, number> {
  const by = new Map(list.map((m) => [m.name, m]));
  const found = new Map<string, number>();
  const walk = (name: string, seen: Set<string>): number => {
    const had = found.get(name);
    if (had !== undefined) return had;
    if (seen.has(name)) return 0;
    seen.add(name);
    const uses = (by.get(name)?.uses ?? []).filter((used) => by.has(used));
    const at = uses.length ? 1 + Math.max(...uses.map((used) => walk(used, seen))) : 0;
    seen.delete(name);
    found.set(name, at);
    return at;
  };
  for (const m of list) walk(m.name, new Set());
  return found;
}

/**
 * The module dependency graph as a table: what each directory of Rust uses, what uses it,
 * and the requirements it serves. A module serving none is marked.
 *
 * @implements REQ-MODULES.drawn_in_overview
 */
function modules(list: Module[], on: Handlers): HTMLElement {
  const table = el("table", "ov-matrix ov-modules");
  const head = el("tr");
  for (const title of ["Module", "Files", "Uses", "Used by", "Serves"]) head.appendChild(el("th", "", title));
  table.appendChild(head);
  const layer = depth(list);
  const users = (name: string) => list.filter((m) => m.uses.includes(name)).map((m) => m.name);
  const chips = (cell: HTMLElement, names: string[], card: boolean) => {
    if (!names.length) cell.textContent = "–";
    for (const name of names) {
      const chip = el("span", "ov-chip", name);
      if (card) {
        chip.addEventListener("click", () => on.open(name));
        chip.addEventListener("mouseenter", () => on.card(chip, name));
        chip.addEventListener("mouseleave", () => on.uncard());
      }
      cell.appendChild(chip);
    }
  };
  for (const m of [...list].sort((a, b) => (layer.get(b.name) ?? 0) - (layer.get(a.name) ?? 0) || a.name.localeCompare(b.name))) {
    const row = el("tr", m.serves.length ? "" : "ov-idle");
    const [uses, usedBy, serves] = [el("td", "ov-title"), el("td", "ov-title"), el("td", "ov-title")];
    chips(uses, m.uses, false);
    chips(usedBy, users(m.name), false);
    chips(serves, m.serves, true);
    if (!m.serves.length) {
      serves.textContent = "claims no requirement";
      row.title = "no file here carries an annotation: code nothing asked for, or nothing says so";
    }
    row.append(el("td", "ov-req", m.name), el("td", "", String(m.files)), uses, usedBy, serves);
    table.appendChild(row);
  }
  return table;
}

function matrix(nodes: Node[], on: Handlers): HTMLElement {
  const open = on.open;
  const table = el("table", "ov-matrix");
  const head = el("tr");
  for (const title of ["Requirement", "", ...MARKS.map(([, letter, name]) => `${letter}|${name}`), "Lines|share of its code's lines that tests run"]) {
    const [text, tip] = title.split("|");
    const cell = el("th", "", text);
    if (tip) cell.title = tip;
    head.appendChild(cell);
  }
  table.appendChild(head);
  for (const node of [...nodes].sort((a, b) => left(b) - left(a) || a.id.localeCompare(b.id))) {
    const row = el("tr");
    row.tabIndex = 0;
    row.append(el("td", "ov-req", node.id), el("td", "ov-title", node.title));
    for (const [key, , name] of MARKS) {
      const cell = el("td", `ov-cell ${state(node.held[key], node.clauses)}`, `${node.held[key]}/${node.clauses}`);
      cell.title = `${node.held[key]} of ${node.clauses} clauses ${name.toLowerCase()}`;
      row.appendChild(cell);
    }
    const lines = node.covered ? `${percent(node.covered[0], node.covered[1])}%` : "–";
    row.appendChild(el("td", `ov-cell ${node.covered ? state(node.covered[0], node.covered[1]) : "empty"}`, lines));
    row.addEventListener("click", () => open(node.id));
    row.addEventListener("mouseenter", () => on.card(row, node.id));
    row.addEventListener("mouseleave", () => on.uncard());
    table.appendChild(row);
  }
  return table;
}

export function drawOverview(into: HTMLElement, design: Design, on: Handlers) {
  const keep = into.scrollTop;
  const { nodes, problems } = design;
  into.textContent = "";
  const head = el("div", "ov-head");
  const shut = el("button", "ov-close", "×");
  shut.title = "Close the overview (Escape)";
  shut.setAttribute("aria-label", "close the overview");
  shut.addEventListener("click", on.close);
  const copy = el("button", "ov-pick ov-copy", "Copy as Markdown");
  copy.title = "Put this overview on the clipboard as Markdown, for a pull request or a note";
  copy.addEventListener("click", () => {
    void (on.copy?.() ?? Promise.resolve(false)).then((did) => {
      copy.textContent = did ? "Copied" : "Could not copy";
      setTimeout(() => (copy.textContent = "Copy as Markdown"), 1500);
    });
  });
  head.append(el("h1", "", "Overview"), el("span", "ov-hint", "point at a requirement for its card · click to open it · the same as text: Space w t o"), copy, shut);
  into.appendChild(head);
  if (!nodes.length) {
    into.appendChild(el("p", "ov-empty", "No requirements here yet. Create one under reqs/ (File › new), then annotate code with @implements."));
    return;
  }
  const section = (title: string, hint: string, ...body: HTMLElement[]) => {
    const part = el("section", "ov-section");
    const heading = el("h2", "", title);
    heading.appendChild(el("span", "ov-hint", hint));
    part.append(heading, ...body);
    into.appendChild(part);
  };
  const risk = new Set(design.impact.map((hit) => hit.reqId));
  const kept = filtered(nodes, showing, risk);
  into.appendChild(tiles(nodes, problems));
  if (design.impact.length) {
    section("Your unsaved edits", `${design.impact.length} clause(s) are claimed by files you changed and have not saved`, impact(design, on));
  }
  if (design.steps?.length) {
    section("The agent's waiting changes", "in the order they came · accept or reject them in the sandbox station (Space w t s)", steps(design.steps, on));
  }
  section("How far the clauses got", "each mark is independent; none is averaged into another", funnel(nodes));
  const figure = !kept.length
    ? el("p", "ov-empty", "Nothing matches these filters.")
    : showing.as === "graph" ? graph(kept, on, risk) : tree(kept, on, risk);
  const hint = showing.as === "graph"
    ? "roots at the top · bar: implemented · dots: A J T P (filled all, ring some, hollow none)"
    : "area: number of clauses · colour: red little held, green everything held";
  section(`The design · ${kept.length} of ${nodes.length}`, hint, controls(design, () => drawOverview(into, design, on)), figure);
  section("Where work is left", "most left first · I implemented, A annotated, J judged, T tested, P proved", matrix(kept, on));
  section("Test runs on record", "what was compared with its model, and how hard", runs(design, on));
  if (design.modules?.length) {
    const idle = design.modules.filter((m) => !m.serves.length).length;
    section("Modules", `a directory of Rust each · users first, what they use below · ${idle} claim no requirement · as text: Space w t m`, modules(design.modules, on));
  }
  into.scrollTop = keep;
}
