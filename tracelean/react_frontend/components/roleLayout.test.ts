import { describe, expect, it } from "vitest";
import { COLUMNS, edgePath, isBackEdge, layoutRoleGraph, type Column } from "./roleLayout";

describe("layoutRoleGraph", () => {
  it("puts each column at its own x, left to right in claim order", () => {
    const layout = layoutRoleGraph(
      [
        { id: "r", column: "requirement" },
        { id: "m", column: "model" },
        { id: "i", column: "implementation" },
        { id: "e", column: "evidence" },
      ],
      { width: 800 }
    );
    const xs = COLUMNS.map((c) => layout.columnX[c]);
    expect(xs).toEqual([...xs].sort((a, b) => a - b));
    expect(new Set(xs).size).toBe(4);
  });

  it("stacks nodes within a column without overlapping", () => {
    const layout = layoutRoleGraph(
      [
        { id: "a", column: "model" },
        { id: "b", column: "model" },
        { id: "c", column: "model" },
      ],
      { width: 600, nodeHeight: 30, gap: 10 }
    );
    const a = layout.placed.get("a")!;
    const b = layout.placed.get("b")!;
    const c = layout.placed.get("c")!;
    expect(b.y).toBeGreaterThanOrEqual(a.y + a.h);
    expect(c.y).toBeGreaterThanOrEqual(b.y + b.h);
    expect(a.x).toBe(b.x);
  });

  it("counts every node it was given", () => {
    const nodes = Array.from({ length: 17 }, (_, i) => ({
      id: `n${i}`,
      column: (i % 2 ? "model" : "evidence") as Column,
    }));
    const layout = layoutRoleGraph(nodes, { width: 500 });
    expect(layout.placed.size).toBe(17);
  });

  it("grows its height to fit the tallest column", () => {
    const one = layoutRoleGraph([{ id: "a", column: "model" }], { width: 500 });
    const many = layoutRoleGraph(
      Array.from({ length: 6 }, (_, i) => ({ id: `n${i}`, column: "model" as const })),
      { width: 500 }
    );
    expect(many.height).toBeGreaterThan(one.height);
  });

  it("draws an edge from the right of one box to the left of the next", () => {
    const layout = layoutRoleGraph(
      [
        { id: "r", column: "requirement" },
        { id: "m", column: "model" },
      ],
      { width: 800 }
    );
    const from = layout.placed.get("r")!;
    const to = layout.placed.get("m")!;
    const path = edgePath(from, to);
    expect(path.startsWith(`M ${from.x + from.w} `)).toBe(true);
    expect(path).toContain(`${to.x} `);
    expect(isBackEdge(from, to)).toBe(false);
    expect(isBackEdge(to, from)).toBe(true);
  });
});
