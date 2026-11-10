// The figures' arithmetic, without a browser: what is filtered, where a treemap puts its
// rectangles, which row of the graph a requirement is drawn on.
//
// @tests REQ-FIGURE.graph_filtered
// @tests REQ-FIGURE.treemap_areas
// @tests REQ-LOOK.overview_drawn
// @tests REQ-MODULES.drawn_in_overview
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const dist = join(dirname(fileURLToPath(import.meta.url)), "..", "dist");
const { depth: moduleDepth, filtered, treemap, layers, under, state, step, left, ago } = await import(pathToFileURL(join(dist, "overview.js")).href);

let failed = 0;
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `  ${detail}`}`);
  if (!ok) failed += 1;
};

const node = (id, refines, clauses, held) => ({
  id, title: `title of ${id}`, refines, clauses, modelled: 0, covered: null,
  held: { implemented: held[0], annotated: held[1], judged: held[2], tested: held[3], proved: held[4] },
});
const nodes = [
  node("ARCH-X", [], 2, [2, 2, 2, 2, 2]),
  node("ARCH-Y", [], 1, [0, 0, 0, 0, 0]),
  node("REQ-A", ["ARCH-X"], 4, [4, 4, 0, 2, 0]),
  node("REQ-B", ["REQ-A", "ARCH-Y"], 3, [1, 3, 0, 0, 0]),
  node("REQ-C", ["REQ-GONE"], 1, [1, 1, 1, 1, 1]),
];
const all = { filter: "all", root: "", query: "", as: "graph" };
const ids = (found) => found.map((n) => n.id).join(",");
const none = new Set();

check("no filter keeps every requirement", ids(filtered(nodes, all, none)) === "ARCH-X,ARCH-Y,REQ-A,REQ-B,REQ-C");
check("work left keeps those missing a mark", ids(filtered(nodes, { ...all, filter: "left" }, none)) === "ARCH-Y,REQ-A,REQ-B");
check("not implemented keeps those with a clause nothing implements",
  ids(filtered(nodes, { ...all, filter: "unimplemented" }, none)) === "ARCH-Y,REQ-B");
check("touched keeps only what the unsaved edit names",
  ids(filtered(nodes, { ...all, filter: "risk" }, new Set(["REQ-A"]))) === "REQ-A");
check("a root keeps itself and everything refining it, however far",
  ids(filtered(nodes, { ...all, root: "ARCH-X" }, none)) === "ARCH-X,REQ-A,REQ-B", ids(filtered(nodes, { ...all, root: "ARCH-X" }, none)));
check("every word typed must match the name or the title",
  ids(filtered(nodes, { ...all, query: "req title b" }, none)) === "REQ-B");
check("the filters combine", ids(filtered(nodes, { ...all, root: "ARCH-Y", filter: "unimplemented", query: "arch" }, none)) === "ARCH-Y");
check("under follows refinement downwards only", [...under(nodes, "REQ-A")].sort().join() === "REQ-A,REQ-B");

const depth = layers(nodes);
check("a root is on the first row and a requirement one below the lowest it refines",
  depth.get("ARCH-X") === 0 && depth.get("REQ-A") === 1 && depth.get("REQ-B") === 2);
check("a requirement refining only something absent is drawn as a root", depth.get("REQ-C") === 0);
const looped = layers([node("P", ["Q"], 1, [0, 0, 0, 0, 0]), node("Q", ["P"], 1, [0, 0, 0, 0, 0])]);
check("a cycle is still laid out", looped.size === 2);

const mod = (name, uses) => ({ name, files: 1, uses, serves: [] });
const layering = moduleDepth([mod("top", ["mid", "low"]), mod("mid", ["low", "outside"]), mod("low", []), mod("a", ["b"]), mod("b", ["a"])]);
check("a module using nothing is on the first layer and one above everything it uses",
  layering.get("low") === 0 && layering.get("mid") === 1 && layering.get("top") === 2, JSON.stringify([...layering]));
check("a module using only what is not listed is on the first layer", moduleDepth([mod("x", ["gone"])]).get("x") === 0);
check("a cycle of modules is still laid out", layering.has("a") && layering.has("b"));

const [W, H] = [1000, 400];
const items = nodes.map((n) => ({ id: n.id, size: n.clauses }));
const tiles = treemap(items, 0, 0, W, H);
const total = items.reduce((s, i) => s + i.size, 0);
check("every requirement gets one rectangle", tiles.length === items.length && new Set(tiles.map((t) => t.id)).size === items.length);
check("each area is its share of the clauses",
  tiles.every((t) => Math.abs(t.w * t.h - (W * H * items.find((i) => i.id === t.id).size) / total) < 1e-6),
  JSON.stringify(tiles));
check("the rectangles tile the figure", Math.abs(tiles.reduce((s, t) => s + t.w * t.h, 0) - W * H) < 1e-6);
const overlap = (a, b) => a.x < b.x + b.w - 1e-9 && b.x < a.x + a.w - 1e-9 && a.y < b.y + b.h - 1e-9 && b.y < a.y + a.h - 1e-9;
check("none overlaps another", tiles.every((a, i) => tiles.every((b, j) => i >= j || !overlap(a, b))));
check("none leaves the figure", tiles.every((t) => t.x >= -1e-9 && t.y >= -1e-9 && t.x + t.w <= W + 1e-9 && t.y + t.h <= H + 1e-9));
check("nothing is drawn for nothing", treemap([], 0, 0, W, H).length === 0);

check("a mark is full, some, none, or has nothing to hold",
  [state(2, 2), state(1, 2), state(0, 2), state(0, 0)].join() === "full,some,none,empty");
check("what is left counts every missing clause-mark", left(nodes[0]) === 0 && left(nodes[2]) === 10 && left(nodes[1]) === 5);
check("the colour steps from nothing held to everything held", step(nodes[1]) === 0 && step(nodes[0]) === 3 && step(nodes[2]) === 1);
check("an age is said in its largest unit", [ago(null), ago(5), ago(200), ago(7300), ago(200000)].join("|") === "–|just now|3 min ago|2 h ago|2 d ago");

if (failed) {
  console.log(`\n${failed} overview check(s) failed`);
  process.exit(1);
}
console.log("\nall overview checks passed");
