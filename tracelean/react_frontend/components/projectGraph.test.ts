import { describe, expect, it } from "vitest";
import { orderNodes, type RoleGraphData, type RoleNode } from "./ProjectGraph";

function node(id: string, column: RoleNode["column"], label = id): RoleNode {
  return {
    id,
    column,
    label,
    sublabel: "",
    file: "f.rs",
    start_line: 0,
    end_line: 0,
    roles: [],
    assurance: null,
    strength: null,
    coverage: null,
    tests: null,
    harness: null,
    stale: false,
    exempt: false,
    partial: false,
    findings: [],
  };
}

describe("orderNodes", () => {
  it("emits columns in claim order: requirement, model, implementation, evidence", () => {
    const data: RoleGraphData = {
      nodes: [
        node("t.rs::test_a", "evidence"),
        node("i.py::a", "implementation"),
        node("REQ-A.one", "requirement"),
        node("m.lean::a", "model"),
      ],
      edges: [],
      unlinked_clauses: [],
    };
    expect(orderNodes(data).map((n) => n.column)).toEqual([
      "requirement",
      "model",
      "implementation",
      "evidence",
    ]);
  });

  it("places a declaration beside the requirement it serves", () => {
    // Two requirements, and the model of the *second* listed first in the raw
    // data. Ordering by the requirement's rank is what keeps an edge a short
    // hop instead of a diagonal across the picture.
    const data: RoleGraphData = {
      nodes: [
        node("REQ-A.one", "requirement"),
        node("REQ-B.one", "requirement"),
        node("m.lean::b", "model"),
        node("m.lean::a", "model"),
      ],
      edges: [
        { from: "REQ-A.one", to: "m.lean::a", role: "models", stale: false },
        { from: "REQ-B.one", to: "m.lean::b", role: "models", stale: false },
      ],
    unlinked_clauses: [],
    };
    const models = orderNodes(data).filter((n) => n.column === "model");
    expect(models.map((n) => n.id)).toEqual(["m.lean::a", "m.lean::b"]);
  });

  it("keeps a declaration serving several clauses next to the first of them", () => {
    const data: RoleGraphData = {
      nodes: [
        node("REQ-A.one", "requirement"),
        node("REQ-B.one", "requirement"),
        node("m.lean::shared", "model"),
        node("m.lean::later", "model"),
      ],
      edges: [
        { from: "REQ-B.one", to: "m.lean::shared", role: "models", stale: false },
        { from: "REQ-A.one", to: "m.lean::shared", role: "models", stale: false },
        { from: "REQ-B.one", to: "m.lean::later", role: "models", stale: false },
      ],
      unlinked_clauses: [],
    };
    const models = orderNodes(data).filter((n) => n.column === "model");
    expect(models[0].id).toBe("m.lean::shared");
  });

  it("loses nothing it was given", () => {
    const data: RoleGraphData = {
      nodes: [
        node("REQ-A.one", "requirement"),
        node("m.lean::a", "model"),
        node("i.py::a", "implementation"),
        node("t.py::a", "evidence"),
        node("orphan.py::x", "implementation"),
      ],
      edges: [{ from: "REQ-A.one", to: "m.lean::a", role: "models", stale: false }],
      unlinked_clauses: ["REQ-A.two"],
    };
    expect(orderNodes(data)).toHaveLength(5);
  });
});

describe("graph colour budget", () => {
  it("spends colour on assurance, not on edge roles", async () => {
    // Two role colours were once the same hex as two assurance levels —
    // `implements` green as L3, `proves` blue as L4 — so a green line and a
    // green box meant unrelated things. The fix is that edges have no role
    // palette at all, and this is the guard on it.
    // Read as raw text rather than importing the module: the assertion is
    // about what the file says, and importing a .tsx with JSX into a plain
    // vitest environment would pull in React for no reason.
    const source: string = (
      await import("./ProjectGraph.tsx?raw")
    ).default;
    expect(source).not.toContain("ROLE_COLOR");
    const levelColors = ["#c05a3a", "#c8952f", "#4f8f4f", "#3f7fbf"];
    const edgeColors = [...source.matchAll(/const EDGE_(?:COLOR|HIGHLIGHT) = "(#[0-9a-f]{6})"/g)].map(
      (m) => m[1]
    );
    expect(edgeColors.length).toBe(2);
    for (const edge of edgeColors) {
      expect(levelColors).not.toContain(edge);
    }
  });
});
