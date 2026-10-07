import { describe, expect, it } from "vitest";

import { hierarchyFromPaths, layoutTreemap, squarify, type Rect } from "./treemap";

const CONTAINER: Rect = { x: 0, y: 0, w: 320, h: 180 };

/** Area of the intersection of two rectangles. */
function overlapArea(a: Rect, b: Rect): number {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

/** Scale raw weights so their sum is the container's area, as the layout expects. */
function scaled(values: number[], rect: Rect): number[] {
  const total = values.reduce((sum, v) => sum + v, 0);
  const area = rect.w * rect.h;
  return values.map((v) => (v / total) * area);
}

describe("squarify", () => {
  it("tiles the container exactly", () => {
    // The property the old map violated: areas must be conserved. A treemap
    // whose rectangles do not sum to the container is reporting a proportion
    // that is not the proportion in the data.
    const values = [50, 30, 12, 5, 2, 1];
    const rects = squarify(scaled(values, CONTAINER), CONTAINER);
    const covered = rects.reduce((sum, r) => sum + r.w * r.h, 0);
    expect(covered).toBeCloseTo(CONTAINER.w * CONTAINER.h, 3);
  });

  it("produces disjoint rectangles", () => {
    const rects = squarify(scaled([40, 25, 20, 10, 5], CONTAINER), CONTAINER);
    for (let i = 0; i < rects.length; i += 1) {
      for (let j = i + 1; j < rects.length; j += 1) {
        expect(overlapArea(rects[i], rects[j])).toBeCloseTo(0, 6);
      }
    }
  });

  it("keeps every rectangle inside the container", () => {
    // The old layout let `y` run past the canvas, after which later rectangles
    // clamped to zero height and vanished without a trace.
    const rects = squarify(scaled([9, 8, 7, 6, 5, 4, 3, 2, 1], CONTAINER), CONTAINER);
    for (const r of rects) {
      expect(r.x).toBeGreaterThanOrEqual(-1e-6);
      expect(r.y).toBeGreaterThanOrEqual(-1e-6);
      expect(r.x + r.w).toBeLessThanOrEqual(CONTAINER.x + CONTAINER.w + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(CONTAINER.y + CONTAINER.h + 1e-6);
    }
  });

  it("gives every non-zero value a visible rectangle", () => {
    const rects = squarify(scaled([100, 1], CONTAINER), CONTAINER);
    expect(rects).toHaveLength(2);
    for (const r of rects) {
      expect(r.w * r.h).toBeGreaterThan(0);
    }
  });

  it("returns each rectangle in the order of its value", () => {
    // Callers zip the result with their own array; a reordered result would
    // silently mislabel every tile.
    const rects = squarify(scaled([10, 20, 30], CONTAINER), CONTAINER);
    const areas = rects.map((r) => r.w * r.h);
    expect(areas[1]).toBeGreaterThan(areas[0]);
    expect(areas[2]).toBeGreaterThan(areas[1]);
  });

  it("survives a zero-valued entry without collapsing the rest", () => {
    const rects = squarify(scaled([10, 0.0000001, 10], CONTAINER), CONTAINER);
    const covered = rects.reduce((sum, r) => sum + r.w * r.h, 0);
    expect(covered).toBeCloseTo(CONTAINER.w * CONTAINER.h, 2);
  });

  it("handles an empty input", () => {
    expect(squarify([], CONTAINER)).toEqual([]);
  });
});

describe("hierarchyFromPaths", () => {
  it("folds paths into directories whose value is the sum of their contents", () => {
    const roots = hierarchyFromPaths([
      { path: "engine/pricing.py", value: 40, data: "a" },
      { path: "engine/util.py", value: 10, data: "b" },
      { path: "service/receipt.rs", value: 25, data: "c" },
    ]);
    const engine = roots.find((r) => r.name === "engine");
    expect(engine?.value).toBe(50);
    expect(engine?.children).toHaveLength(2);
    expect(roots.find((r) => r.name === "service")?.value).toBe(25);
  });

  it("keeps several records for one file as separate leaves", () => {
    // A file with three annotated symbols is three rectangles, not one: they
    // are different pieces of code with different requirements.
    const roots = hierarchyFromPaths([
      { path: "a.py", value: 5, data: "one" },
      { path: "a.py", value: 7, data: "two" },
    ]);
    expect(roots).toHaveLength(2);
    expect(roots.map((r) => r.value).sort()).toEqual([5, 7]);
  });

  it("orders siblings largest first, so the map is stable between renders", () => {
    const roots = hierarchyFromPaths([
      { path: "small/x", value: 1, data: null },
      { path: "big/y", value: 100, data: null },
    ]);
    expect(roots[0].name).toBe("big");
  });
});

describe("layoutTreemap", () => {
  const tree = hierarchyFromPaths([
    { path: "engine/pricing.py", value: 40, data: "pricing" },
    { path: "engine/util.py", value: 20, data: "util" },
    { path: "service/receipt.rs", value: 30, data: "receipt" },
    { path: "harness/adapter.py", value: 10, data: "adapter" },
  ]);

  it("emits a container before the children drawn inside it", () => {
    // Painting order is the only thing keeping a child visible over its parent.
    const tiles = layoutTreemap(tree, CONTAINER);
    const firstContainer = tiles.findIndex((t) => t.container);
    const firstChild = tiles.findIndex((t) => t.depth === 1);
    expect(firstContainer).toBeGreaterThanOrEqual(0);
    expect(firstChild).toBeGreaterThan(firstContainer);
  });

  it("keeps children inside their parent", () => {
    const tiles = layoutTreemap(tree, CONTAINER);
    const containers = tiles.filter((t) => t.container && t.depth === 0);
    for (const parent of containers) {
      const children = tiles.filter(
        (t) => t.depth === 1 && overlapArea(t.rect, parent.rect) > 0
      );
      for (const child of children) {
        expect(child.rect.x).toBeGreaterThanOrEqual(parent.rect.x - 1e-6);
        expect(child.rect.y).toBeGreaterThanOrEqual(parent.rect.y - 1e-6);
        expect(child.rect.x + child.rect.w).toBeLessThanOrEqual(
          parent.rect.x + parent.rect.w + 1e-6
        );
        expect(child.rect.y + child.rect.h).toBeLessThanOrEqual(
          parent.rect.y + parent.rect.h + 1e-6
        );
      }
    }
  });

  it("does not expand a node too small to show its children", () => {
    // A directory that would be four pixels wide stays one rectangle; drawing
    // sub-pixel children inside it is how a map becomes noise.
    const tiles = layoutTreemap(tree, { x: 0, y: 0, w: 20, h: 12 });
    expect(tiles.every((t) => !t.container)).toBe(true);
  });

  it("respects maxDepth", () => {
    const tiles = layoutTreemap(tree, CONTAINER, { maxDepth: 0 });
    expect(tiles.every((t) => t.depth === 0)).toBe(true);
  });
});
