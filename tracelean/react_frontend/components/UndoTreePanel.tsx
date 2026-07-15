import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface UndoNodeView {
  id: string;
  parent: string | null;
  children: string[];
  command_summary: string;
  file: string | null;
  timestamp: string;
  is_commit_point: boolean;
  commit_name: string | null;
}

interface UndoTreeData {
  nodes: UndoNodeView[];
  current_id: string | null;
}

interface CommandLogEntry {
  index: number;
  summary: string;
}

interface UndoTreePanelProps {
  visible: boolean;
  onClose: () => void;
  onNodeJump: () => void;
  onFileSelect: (path: string) => void;
  currentFile: string | null;
}

type FilterMode = "global" | "file" | "commits" | "batch";

function cmdMarker(summary: string): string {
  if (summary.startsWith("Insert")) return "I";
  if (summary.startsWith("Initial")) return "In";
  if (summary.startsWith("Delete @") && !summary.startsWith("Delete file")) return "D";
  if (summary.startsWith("Replace")) return "R";
  if (summary.startsWith("Cursor")) return "C";
  if (summary.startsWith("Select")) return "S";
  if (summary.startsWith("Create")) return "+";
  if (summary.startsWith("Delete file")) return "×";
  if (summary.startsWith("Rename")) return "→";
  if (summary.startsWith("Batch")) return "B";
  return "?";
}

function shortTooltip(summary: string): string {
  const m = summary.match(/"([^"]*)"/);
  if (m) return m[1];
  return summary.split(" ")[0];
}

function fileColor(file: string | null): string {
  if (!file) return "#666";
  let hash = 0;
  for (let i = 0; i < file.length; i++) {
    hash = file.charCodeAt(i) + ((hash << 5) - hash);
  }
  return `hsl(${Math.abs(hash) % 360}, 50%, 55%)`;
}

// --- Simple layout: no coalescing, just place nodes ---

interface LayoutNode {
  id: string;
  col: number;
  row: number;
  marker: string;
  isCurrent: boolean;
  isCommitPoint: boolean;
  summary: string;
  file: string | null;
}

interface LayoutEdge {
  x1: number; y1: number;
  x2: number; y2: number;
  isBranch: boolean;
}

const CELL_W = 28;
const CELL_H = 24;
const NODE_R = 9;
const PAD = 12;

function cx(col: number) { return col * CELL_W + CELL_W / 2 + PAD; }
function cy(row: number) { return row * CELL_H + CELL_H / 2 + PAD; }

function layoutTree(data: UndoTreeData, filterFile: string | null): {
  nodes: LayoutNode[];
  edges: LayoutEdge[];
  width: number;
  height: number;
} {
  if (data.nodes.length === 0) return { nodes: [], edges: [], width: 0, height: 0 };

  let rawNodes = data.nodes;
  if (filterFile) {
    rawNodes = rawNodes.filter((n) => n.file === filterFile || n.file === null);
  }

  const nodeMap = new Map<string, UndoNodeView>();
  rawNodes.forEach((n) => nodeMap.set(n.id, n));

  const roots = rawNodes.filter((n) => n.parent === null || !nodeMap.has(n.parent!));

  // Track occupied rows per column: occupied[col] = Set<row>
  const occupied: Set<number>[] = [];

  function isRangeFree(col: number, fromRow: number, count: number): boolean {
    if (col >= occupied.length) return true;
    const set = occupied[col];
    for (let r = fromRow; r < fromRow + count; r++) {
      if (set.has(r)) return false;
    }
    return true;
  }

  function reserveRange(col: number, fromRow: number, count: number) {
    while (occupied.length <= col) occupied.push(new Set());
    for (let r = fromRow; r < fromRow + count; r++) {
      occupied[col].add(r);
    }
  }

  function findCol(fromRow: number, count: number): number {
    // Try col 1, 2, ... (col 0 reserved for main trunk)
    for (let col = 1; col < occupied.length + 1; col++) {
      if (isRangeFree(col, fromRow, count)) return col;
    }
    return occupied.length;
  }

  // Compute subtree depth
  const subtreeDepth = new Map<string, number>();
  function computeDepth(nodeId: string): number {
    if (subtreeDepth.has(nodeId)) return subtreeDepth.get(nodeId)!;
    const node = nodeMap.get(nodeId);
    if (!node) return 0;
    const children = node.children.filter((c) => nodeMap.has(c));
    if (children.length === 0) { subtreeDepth.set(nodeId, 1); return 1; }
    const d = 1 + Math.max(...children.map((c) => computeDepth(c)));
    subtreeDepth.set(nodeId, d);
    return d;
  }
  roots.forEach((r) => computeDepth(r.id));

  const positions = new Map<string, { col: number; row: number }>();

  function dfs(nodeId: string, row: number, col: number) {
    const node = nodeMap.get(nodeId);
    if (!node || positions.has(nodeId)) return;
    positions.set(nodeId, { col, row });
    reserveRange(col, row, 1);

    const children = node.children.filter((c) => nodeMap.has(c));
    if (children.length === 0) return;

    // First child same column
    dfs(children[0], row + 1, col);

    // Other children: find leftmost free column
    for (let i = 1; i < children.length; i++) {
      const depth = subtreeDepth.get(children[i]) || 1;
      const branchCol = findCol(row + 1, depth);
      reserveRange(branchCol, row + 1, depth);
      dfs(children[i], row + 1, branchCol);
    }
  }

  // Main trunk on col 0
  roots.forEach((root) => {
    dfs(root.id, 0, 0);
  });

  // Build output
  const layoutNodes: LayoutNode[] = [];
  const layoutEdges: LayoutEdge[] = [];

  for (const node of rawNodes) {
    const pos = positions.get(node.id);
    if (!pos) continue;

    layoutNodes.push({
      id: node.id,
      col: pos.col,
      row: pos.row,
      marker: cmdMarker(node.command_summary),
      isCurrent: node.id === data.current_id,
      isCommitPoint: node.is_commit_point,
      summary: node.command_summary,
      file: node.file,
    });

    if (node.parent && positions.has(node.parent)) {
      const parentPos = positions.get(node.parent)!;
      layoutEdges.push({
        x1: cx(parentPos.col), y1: cy(parentPos.row),
        x2: cx(pos.col), y2: cy(pos.row),
        isBranch: parentPos.col !== pos.col,
      });
    }
  }

  const maxCol = layoutNodes.length > 0 ? Math.max(...layoutNodes.map((n) => n.col)) : 0;
  const maxRow = layoutNodes.length > 0 ? Math.max(...layoutNodes.map((n) => n.row)) : 0;
  const width = (maxCol + 1) * CELL_W + PAD * 2;
  const height = (maxRow + 1) * CELL_H + PAD * 2;

  return { nodes: layoutNodes, edges: layoutEdges, width, height };
}

// --- Components ---

export function UndoTreePanel({ visible, onClose, onNodeJump, onFileSelect, currentFile }: UndoTreePanelProps) {
  const [treeData, setTreeData] = useState<UndoTreeData | null>(null);
  const [commandLog, setCommandLog] = useState<CommandLogEntry[]>([]);
  const [activeTab, setActiveTab] = useState<"tree" | "log">("tree");
  const [filterMode, setFilterMode] = useState<FilterMode>("global");
  const [_diffPreview, setDiffPreview] = useState<string | null>(null);
  const [_hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const fileFilter = filterMode === "file" ? currentFile : null;
      const tree = await invoke<UndoTreeData>("get_undo_tree", {
        fileFilter: fileFilter || null,
      });
      setTreeData(tree);
      const log = await invoke<CommandLogEntry[]>("get_command_log", { limit: 100 });
      setCommandLog(log);
    } catch (e) {
      console.error("Failed to load undo tree:", e);
    }
  }, [filterMode, currentFile]);

  useEffect(() => {
    if (visible) {
      refresh();
      const unlisten = listen("undo-tree-changed", () => {
        refresh();
      });
      return () => { unlisten.then((fn) => fn()); };
    }
  }, [visible, refresh]);

  const handleJump = async (nodeId: string, file: string | null) => {
    try {
      const success = await invoke<boolean>("jump_to_node", { nodeId });
      if (success) {
        // Only switch file if the jump target is a different file than currently active
        if (file && file !== currentFile) onFileSelect(file);
        await refresh();
        onNodeJump();
      }
    } catch (e) {
      console.error("Jump failed:", e);
    }
  };

  const handleHover = async (nodeId: string | null) => {
    setHoveredNodeId(nodeId);
    if (nodeId) {
      try {
        const diff = await invoke<string>("get_undo_node_diff", { nodeId });
        setDiffPreview(diff);
        // T0: Emit event so Editor can show inline diff view
        window.dispatchEvent(new CustomEvent("undo-hover-diff", { detail: { diff, nodeId } }));
      } catch {
        setDiffPreview(null);
        window.dispatchEvent(new CustomEvent("undo-hover-diff", { detail: null }));
      }
    } else {
      setDiffPreview(null);
      window.dispatchEvent(new CustomEvent("undo-hover-diff", { detail: null }));
    }
  };

  if (!visible) return null;

  // Apply filter modes
  let fileFilter: string | null = null;
  let filteredData = treeData;

  if (filterMode === "file") {
    fileFilter = currentFile;
  } else if (filterMode === "commits" && treeData) {
    filteredData = {
      ...treeData,
      nodes: treeData.nodes.filter((n) => n.is_commit_point),
    };
  } else if (filterMode === "batch" && treeData) {
    filteredData = {
      ...treeData,
      nodes: treeData.nodes.filter((n) => n.command_summary.startsWith("Batch")),
    };
  }

  const layout = filteredData ? layoutTree(filteredData, fileFilter) : { nodes: [], edges: [], width: 0, height: 0 };

  return (
    <div className="undo-tree-panel">
      <div className="panel-header">
        <div className="panel-tabs">
          <button className={activeTab === "tree" ? "active" : ""}
            onClick={() => setActiveTab("tree")}>Tree</button>
          <button className={activeTab === "log" ? "active" : ""}
            onClick={() => setActiveTab("log")}>Log</button>
        </div>
        <button className="panel-close" onClick={onClose}>✕</button>
      </div>

      {activeTab === "tree" && (
        <div className="tree-filter">
          <button className={filterMode === "global" ? "active" : ""}
            onClick={() => setFilterMode("global")}>All</button>
          <button className={filterMode === "file" ? "active" : ""}
            onClick={() => setFilterMode("file")}
            disabled={!currentFile}>File</button>
          <button className={filterMode === "commits" ? "active" : ""}
            onClick={() => setFilterMode("commits")}>Commits</button>
          <button className={filterMode === "batch" ? "active" : ""}
            onClick={() => setFilterMode("batch")}>Batch</button>
          <button className="clear-btn" onClick={async () => {
            try {
              await invoke("clear_undo_tree");
              refresh();
              onNodeJump();
            } catch (e) { console.error(e); }
          }}>Clear</button>
        </div>
      )}

      <div className="panel-content">
        {activeTab === "tree" ? (
          <>
            <TreeGraph layout={layout} onJump={handleJump} onHover={handleHover} filterMode={filterMode} />
          </>
        ) : (
          <LogView entries={commandLog} />
        )}
      </div>
    </div>
  );
}

function TreeGraph({
  layout,
  onJump,
  onHover,
  filterMode,
}: {
  layout: { nodes: LayoutNode[]; edges: LayoutEdge[]; width: number; height: number };
  onJump: (id: string, file: string | null) => void;
  onHover: (id: string | null) => void;
  filterMode: string;
}) {
  if (layout.nodes.length === 0) {
    return <div className="empty-state">No history yet.</div>;
  }

  // Detect branch points (nodes with >1 child in the layout)
  const parentEdges = new Map<string, number>();
  layout.edges.forEach((e) => {
    const key = `${e.x1},${e.y1}`;
    parentEdges.set(key, (parentEdges.get(key) || 0) + 1);
  });

  return (
    <div className="tree-graph">
      <svg width={layout.width} height={layout.height}>
        {layout.edges.map((e, i) => {
          if (!e.isBranch) {
            return <line key={i} x1={e.x1} y1={e.y1} x2={e.x2} y2={e.y2}
              stroke="#444" strokeWidth={1.5} />;
          } else {
            const midY = (e.y1 + e.y2) / 2;
            return <path key={i}
              d={`M${e.x1},${e.y1} L${e.x1},${midY} L${e.x2},${midY} L${e.x2},${e.y2}`}
              fill="none" stroke="#e5c07b" strokeWidth={1.5} strokeDasharray="3,2" />;
          }
        })}

        {layout.nodes.map((node) => {
          const x = cx(node.col), y = cy(node.row);

          let fill = "#3e3e3e";
          let stroke = "#555";
          let textColor = "#bbb";

          if (node.isCurrent) {
            fill = "#264f78";
            stroke = "#6cb6ff";
            textColor = "#fff";
          } else if (node.isCommitPoint) {
            fill = "#1e3a2a";
            stroke = "#4ec9b0";
            textColor = "#4ec9b0";
          } else if (filterMode === "global" && node.file) {
            stroke = fileColor(node.file);
          }

          return (
            <g key={node.id} style={{ cursor: "pointer" }}
              onClick={() => onJump(node.id, node.file)}
              onMouseEnter={() => onHover(node.id)}
              onMouseLeave={() => onHover(null)}>
              <title>{shortTooltip(node.summary)}</title>
              <circle cx={x} cy={y} r={NODE_R}
                fill={fill} stroke={stroke} strokeWidth={1.5} />
              <text x={x} y={y + 3.5} textAnchor="middle"
                fontSize={9} fontWeight="bold" fill={textColor}
                style={{ pointerEvents: "none", userSelect: "none" }}>
                {node.marker}
              </text>
              {/* Commit point label */}
              {node.isCommitPoint && (
                <text x={x + NODE_R + 4} y={y + 3} fontSize={8} fill="#4ec9b0"
                  style={{ pointerEvents: "none" }}>
                  ●
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function LogView({ entries }: { entries: CommandLogEntry[] }) {
  if (entries.length === 0) {
    return <div className="empty-state">No commands recorded yet.</div>;
  }

  return (
    <div className="log-view">
      {entries.map((entry) => (
        <div key={entry.index} className="log-entry">
          <span className="log-index">#{entry.index}</span>
          <span className="log-summary">{entry.summary}</span>
        </div>
      ))}
    </div>
  );
}
