//! Export functionality for the traceability graph (D3 visualization).

use serde::{Deserialize, Serialize};

use super::types::*;
use super::TraceGraph;

/// Serializable node for dashboard export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNodeExport {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub file: String,
    pub line: Option<u32>,
    pub status: Option<String>,
}

/// Serializable edge for dashboard export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEdgeExport {
    pub source: usize,
    pub target: usize,
    pub kind: String,
}

/// Full graph export for D3 visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTraceGraphExport {
    pub nodes: Vec<TraceNodeExport>,
    pub edges: Vec<TraceEdgeExport>,
}

impl TraceGraph {
    /// Export full graph as serializable nodes + edges for dashboard visualization.
    pub fn export_full_graph(&self) -> FullTraceGraphExport {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            let export_node = match node {
                TraceNode::Requirement(r) => TraceNodeExport {
                    id: format!("req:{}", r.id),
                    kind: "requirement".to_string(),
                    label: format!("{}: {}", r.id, r.title),
                    file: r.file.to_string_lossy().to_string(),
                    line: None,
                    status: Some(format!("{:?}", r.status)),
                },
                TraceNode::Spec(s) => TraceNodeExport {
                    id: format!("spec:{}", s.id),
                    kind: "spec".to_string(),
                    label: s.id.clone(),
                    file: s.file.to_string_lossy().to_string(),
                    line: None,
                    status: None,
                },
                TraceNode::CodeElement(e) => TraceNodeExport {
                    id: format!("code:{}:{}", e.file.to_string_lossy(), e.name),
                    kind: "code".to_string(),
                    label: e.name.clone(),
                    file: e.file.to_string_lossy().to_string(),
                    line: Some(e.start_line),
                    status: None,
                },
                TraceNode::Test(t) => TraceNodeExport {
                    id: format!("test:{}:{}", t.file.to_string_lossy(), t.name),
                    kind: "test".to_string(),
                    label: t.name.clone(),
                    file: t.file.to_string_lossy().to_string(),
                    line: None,
                    status: None,
                },
            };
            nodes.push(export_node);
        }

        for edge_idx in self.graph.edge_indices() {
            let (src, tgt) = self.graph.edge_endpoints(edge_idx).unwrap();
            let edge_weight = &self.graph[edge_idx];
            edges.push(TraceEdgeExport {
                source: src.index(),
                target: tgt.index(),
                kind: format!("{:?}", edge_weight),
            });
        }

        FullTraceGraphExport { nodes, edges }
    }
}
