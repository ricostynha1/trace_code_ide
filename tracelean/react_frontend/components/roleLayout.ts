/**
 * Layout for the role graph: four columns, nodes stacked inside each.
 *
 * Deliberately not a force-directed layout. A force simulation puts a project's
 * structure wherever the physics lands it, so the same project looks different
 * every time it is opened and position carries no meaning. Here the horizontal
 * position *is* the claim -- requirement, model, implementation, evidence --
 * and reading left to right is reading the argument the project makes.
 *
 * Within a column, nodes are ordered by the requirement they serve, so an
 * edge is usually a short hop rather than a long diagonal across the picture.
 */

export type Column = "requirement" | "model" | "implementation" | "evidence";

export const COLUMNS: Column[] = ["requirement", "model", "implementation", "evidence"];

export const COLUMN_TITLE: Record<Column, string> = {
  requirement: "Requirements",
  model: "Models",
  implementation: "Code",
  evidence: "Evidence",
};

export interface LayoutInput {
  id: string;
  column: Column;
}

export interface Placed {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface LayoutOptions {
  width: number;
  /** Node box height. */
  nodeHeight?: number;
  /** Vertical space between boxes. */
  gap?: number;
  /** Space above the first row, for the column headings. */
  headerHeight?: number;
  padding?: number;
}

export interface Layout {
  placed: Map<string, Placed>;
  columnX: Record<Column, number>;
  columnWidth: number;
  height: number;
}

/**
 * Place every node. Ordering within a column follows the input order, which the
 * caller sorts by requirement — so the picture is stable between reloads and
 * two nodes serving the same clause sit near each other.
 */
export function layoutRoleGraph(nodes: LayoutInput[], options: LayoutOptions): Layout {
  const nodeHeight = options.nodeHeight ?? 34;
  const gap = options.gap ?? 10;
  const headerHeight = options.headerHeight ?? 20;
  const padding = options.padding ?? 8;

  const usable = Math.max(options.width - padding * 2, COLUMNS.length * 40);
  // A gutter between columns, wide enough for the edges to be readable.
  const gutter = Math.min(48, usable * 0.06);
  const columnWidth = (usable - gutter * (COLUMNS.length - 1)) / COLUMNS.length;

  const columnX = {} as Record<Column, number>;
  COLUMNS.forEach((column, i) => {
    columnX[column] = padding + i * (columnWidth + gutter);
  });

  const placed = new Map<string, Placed>();
  const counts: Record<string, number> = {};
  let tallest = 0;

  for (const node of nodes) {
    const index = counts[node.column] ?? 0;
    counts[node.column] = index + 1;
    const y = headerHeight + padding + index * (nodeHeight + gap);
    placed.set(node.id, {
      id: node.id,
      x: columnX[node.column],
      y,
      w: columnWidth,
      h: nodeHeight,
    });
    tallest = Math.max(tallest, y + nodeHeight);
  }

  return { placed, columnX, columnWidth, height: tallest + padding };
}

/**
 * A cubic curve from the right edge of one box to the left edge of another.
 *
 * Curved rather than straight because several edges commonly leave one node for
 * the same column, and straight lines at similar angles become impossible to
 * follow where they overlap.
 */
export function edgePath(from: Placed, to: Placed): string {
  const x1 = from.x + from.w;
  const y1 = from.y + from.h / 2;
  const x2 = to.x;
  const y2 = to.y + to.h / 2;
  const dx = Math.max(20, (x2 - x1) / 2);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

/** An edge that goes backwards or stays in one column, drawn as a detour. */
export function isBackEdge(from: Placed, to: Placed): boolean {
  return to.x <= from.x;
}
