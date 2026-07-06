import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import * as d3 from "d3";

// --- Types matching backend TraceNodeExport / TraceEdgeExport ---

interface TraceNodeExport {
  id: string;
  kind: "requirement" | "spec" | "code" | "test";
  label: string;
  file: string;
  line: number | null;
  status: string | null;
}

interface TraceEdgeExport {
  source: number;
  target: number;
  kind: string;
}

interface FullTraceGraph {
  nodes: TraceNodeExport[];
  edges: TraceEdgeExport[];
}

// D3 simulation node
interface SimNode extends d3.SimulationNodeDatum {
  id: string;
  kind: string;
  label: string;
  file: string;
  line: number | null;
  status: string | null;
  index?: number;
}

interface SimLink extends d3.SimulationLinkDatum<SimNode> {
  kind: string;
}

interface TraceabilityDashboardProps {
  visible: boolean;
  onClose: () => void;
  onNavigate: (file: string, line?: number) => void;
}

const KIND_COLORS: Record<string, string> = {
  requirement: "#61afef",
  spec: "#c678dd",
  code: "#98c379",
  test: "#e5c07b",
};

const KIND_RADIUS: Record<string, number> = {
  requirement: 14,
  spec: 11,
  code: 9,
  test: 9,
};

const STATUS_COLORS: Record<string, string> = {
  Draft: "#888",
  Approved: "#e5c07b",
  Linked: "#98c379",
};

export function TraceabilityDashboard({ visible, onClose, onNavigate }: TraceabilityDashboardProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [graphData, setGraphData] = useState<FullTraceGraph | null>(null);
  const [filterKind, setFilterKind] = useState<string>("all");
  const [filterReq, setFilterReq] = useState<string>("");
  const [searchText, setSearchText] = useState<string>("");
  const [hoveredNode, setHoveredNode] = useState<SimNode | null>(null);

  const loadGraph = useCallback(async () => {
    try {
      // Build/rebuild the trace graph from project files first
      await invoke("build_trace_graph");
      const data = await invoke<FullTraceGraph>("get_full_trace_graph");
      setGraphData(data);
    } catch (e) {
      console.error("Failed to load trace graph:", e);
    }
  }, []);

  // Load on visible, live-update on file changes
  useEffect(() => {
    if (!visible) return;
    loadGraph();
    const unlisten = listen("trace-graph-changed", () => loadGraph());
    // Also listen for generic file changes
    const unlisten2 = listen("file-changed", () => loadGraph());
    return () => {
      unlisten.then((fn) => fn());
      unlisten2.then((fn) => fn());
    };
  }, [visible, loadGraph]);

  // D3 rendering
  useEffect(() => {
    if (!visible || !graphData || !svgRef.current) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll("*").remove();

    // Filter nodes
    let nodes: SimNode[] = graphData.nodes.map((n, i) => ({
      ...n,
      index: i,
    }));

    if (filterKind !== "all") {
      const keep = new Set(nodes.filter((n) => n.kind === filterKind).map((n) => n.index));
      nodes = nodes.filter((n) => keep.has(n.index!));
    }

    if (filterReq) {
      // Filter by requirement: keep req + connected nodes
      const reqIndices = new Set(
        graphData.nodes
          .map((n, i) => (n.kind === "requirement" && n.id.includes(filterReq)) ? i : -1)
          .filter((i) => i >= 0)
      );
      const connected = new Set<number>(reqIndices);
      graphData.edges.forEach((e) => {
        if (reqIndices.has(e.source)) connected.add(e.target);
        if (reqIndices.has(e.target)) connected.add(e.source);
      });
      // Expand one more level
      graphData.edges.forEach((e) => {
        if (connected.has(e.source)) connected.add(e.target);
        if (connected.has(e.target)) connected.add(e.source);
      });
      nodes = nodes.filter((n) => connected.has(n.index!));
    }

    if (searchText) {
      const lower = searchText.toLowerCase();
      nodes = nodes.filter(
        (n) => n.label.toLowerCase().includes(lower) || n.file.toLowerCase().includes(lower)
      );
    }

    const nodeIndices = new Set(nodes.map((n) => n.index));
    const links: SimLink[] = graphData.edges
      .filter((e) => nodeIndices.has(e.source) && nodeIndices.has(e.target))
      .map((e) => ({
        source: nodes.find((n) => n.index === e.source)!,
        target: nodes.find((n) => n.index === e.target)!,
        kind: e.kind,
      }))
      .filter((l) => l.source && l.target);

    if (nodes.length === 0) return;

    const width = svgRef.current.clientWidth || 800;
    const height = svgRef.current.clientHeight || 600;

    const g = svg.append("g");

    // Zoom
    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.2, 4])
      .on("zoom", (event) => g.attr("transform", event.transform));
    svg.call(zoom);

    // Force simulation
    const simulation = d3.forceSimulation<SimNode>(nodes)
      .force("link", d3.forceLink<SimNode, SimLink>(links).id((d) => d.id).distance(80))
      .force("charge", d3.forceManyBody().strength(-200))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .force("collision", d3.forceCollide().radius(20));

    // Edges
    const link = g.append("g")
      .selectAll("line")
      .data(links)
      .enter()
      .append("line")
      .attr("stroke", "#555")
      .attr("stroke-width", 1.5)
      .attr("stroke-opacity", 0.6)
      .attr("marker-end", "url(#arrow)");

    // Arrow marker
    svg.append("defs").append("marker")
      .attr("id", "arrow")
      .attr("viewBox", "0 -5 10 10")
      .attr("refX", 20)
      .attr("refY", 0)
      .attr("markerWidth", 6)
      .attr("markerHeight", 6)
      .attr("orient", "auto")
      .append("path")
      .attr("d", "M0,-5L10,0L0,5")
      .attr("fill", "#555");

    // Nodes
    const node = g.append("g")
      .selectAll<SVGGElement, SimNode>("g")
      .data(nodes)
      .enter()
      .append("g")
      .style("cursor", "pointer")
      .call(d3.drag<SVGGElement, SimNode>()
        .on("start", (event, d) => {
          if (!event.active) simulation.alphaTarget(0.3).restart();
          d.fx = d.x;
          d.fy = d.y;
        })
        .on("drag", (event, d) => {
          d.fx = event.x;
          d.fy = event.y;
        })
        .on("end", (event, d) => {
          if (!event.active) simulation.alphaTarget(0);
          d.fx = null;
          d.fy = null;
        })
      );

    // Node circles
    node.append("circle")
      .attr("r", (d) => KIND_RADIUS[d.kind] || 9)
      .attr("fill", (d) => {
        if (d.kind === "requirement" && d.status) {
          return STATUS_COLORS[d.status] || KIND_COLORS[d.kind];
        }
        return KIND_COLORS[d.kind] || "#666";
      })
      .attr("stroke", "#222")
      .attr("stroke-width", 1.5);

    // Labels
    node.append("text")
      .text((d) => d.label.length > 20 ? d.label.slice(0, 18) + "…" : d.label)
      .attr("x", 14)
      .attr("y", 4)
      .attr("font-size", "10px")
      .attr("fill", "#ccc")
      .style("pointer-events", "none");

    // Click → navigate
    node.on("click", (_event, d) => {
      onNavigate(d.file, d.line || undefined);
    });

    // Hover → show tooltip
    node.on("mouseenter", (_event, d) => setHoveredNode(d));
    node.on("mouseleave", () => setHoveredNode(null));

    // Tick
    simulation.on("tick", () => {
      link
        .attr("x1", (d) => (d.source as SimNode).x!)
        .attr("y1", (d) => (d.source as SimNode).y!)
        .attr("x2", (d) => (d.target as SimNode).x!)
        .attr("y2", (d) => (d.target as SimNode).y!);

      node.attr("transform", (d) => `translate(${d.x},${d.y})`);
    });

    return () => { simulation.stop(); };
  }, [visible, graphData, filterKind, filterReq, searchText, onNavigate]);

  if (!visible) return null;

  return (
    <div className="trace-dashboard">
      <div className="trace-dash-header">
        <span className="trace-dash-title">Traceability Dashboard</span>
        <div className="trace-dash-controls">
          <input
            className="trace-search"
            placeholder="Search…"
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
          />
          <select value={filterKind} onChange={(e) => setFilterKind(e.target.value)}>
            <option value="all">All Types</option>
            <option value="requirement">Requirements</option>
            <option value="spec">Specs</option>
            <option value="code">Code</option>
            <option value="test">Tests</option>
          </select>
          <input
            className="trace-filter-req"
            placeholder="Filter REQ…"
            value={filterReq}
            onChange={(e) => setFilterReq(e.target.value)}
          />
        </div>
        <button className="trace-dash-close" onClick={onClose}>✕</button>
      </div>

      <div className="trace-dash-legend">
        {Object.entries(KIND_COLORS).map(([kind, color]) => (
          <span key={kind} className="legend-item">
            <span className="legend-dot" style={{ background: color }} />
            {kind}
          </span>
        ))}
        <span className="legend-item">
          <span className="legend-dot" style={{ background: STATUS_COLORS.Linked }} />
          passing
        </span>
        <span className="legend-item">
          <span className="legend-dot" style={{ background: "#888" }} />
          untested
        </span>
      </div>

      <div className="trace-dash-body">
        <svg ref={svgRef} className="trace-svg" />
      </div>

      {hoveredNode && (
        <div className="trace-tooltip">
          <div className="tt-kind">{hoveredNode.kind}</div>
          <div className="tt-label">{hoveredNode.label}</div>
          <div className="tt-file">{hoveredNode.file}{hoveredNode.line ? `:${hoveredNode.line}` : ""}</div>
          {hoveredNode.status && <div className="tt-status">Status: {hoveredNode.status}</div>}
        </div>
      )}
    </div>
  );
}
