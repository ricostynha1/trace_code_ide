//! Traceability Graph — MVP 2
//!
//! In-memory directed graph linking Requirements → Specs → CodeElements → Tests.
//! Built from filesystem conventions on startup, updated incrementally on file change.
//! Uses petgraph for storage and traversal.

pub mod types;
pub mod query;
pub mod scan;
pub mod export;

use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use types::*;
pub use export::{FullTraceGraphExport, TraceNodeExport, TraceEdgeExport};
pub use scan::parse_requirement_file_public;

// --- The Graph ---

#[derive(Debug)]
pub struct TraceGraph {
    pub(crate) graph: DiGraph<TraceNode, TraceEdge>,
    /// Fast lookups: requirement ID → node index
    pub(crate) req_index: HashMap<ReqId, NodeIndex>,
    /// Fast lookups: file path → node indices in that file
    pub(crate) file_index: HashMap<PathBuf, Vec<NodeIndex>>,
    /// Spec file → node index
    pub(crate) spec_index: HashMap<PathBuf, NodeIndex>,
    /// Code element key (file:name) → node index
    pub(crate) code_index: HashMap<String, NodeIndex>,
    /// Test file → node index
    pub(crate) test_index: HashMap<PathBuf, Vec<NodeIndex>>,
}

impl TraceGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            req_index: HashMap::new(),
            file_index: HashMap::new(),
            spec_index: HashMap::new(),
            code_index: HashMap::new(),
            test_index: HashMap::new(),
        }
    }

    // --- Node insertion ---

    pub fn add_requirement(&mut self, req: Requirement) -> NodeIndex {
        let id = req.id.clone();
        let file = req.file.clone();
        let idx = self.graph.add_node(TraceNode::Requirement(req));
        self.req_index.insert(id, idx);
        self.file_index.entry(file).or_default().push(idx);
        idx
    }

    pub fn add_spec(&mut self, spec: Spec) -> NodeIndex {
        let file = spec.file.clone();
        let idx = self.graph.add_node(TraceNode::Spec(spec));
        self.spec_index.insert(file.clone(), idx);
        self.file_index.entry(file).or_default().push(idx);
        idx
    }

    pub fn add_code_element(&mut self, elem: CodeElement) -> NodeIndex {
        let key = code_element_key(&elem.file, &elem.name);
        let file = elem.file.clone();
        let idx = self.graph.add_node(TraceNode::CodeElement(elem));
        self.code_index.insert(key, idx);
        self.file_index.entry(file).or_default().push(idx);
        idx
    }

    pub fn add_test(&mut self, test: Test) -> NodeIndex {
        let file = test.file.clone();
        let idx = self.graph.add_node(TraceNode::Test(test));
        self.test_index.entry(file.clone()).or_default().push(idx);
        self.file_index.entry(file).or_default().push(idx);
        idx
    }

    // --- Edge insertion ---

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge: TraceEdge) {
        self.graph.add_edge(from, to, edge);
    }

    /// Link requirement → spec by IDs
    pub fn link_req_to_spec(&mut self, req_id: &str, spec_file: &Path) -> bool {
        let req_idx = self.req_index.get(req_id).copied();
        let spec_idx = self.spec_index.get(spec_file).copied();
        if let (Some(r), Some(s)) = (req_idx, spec_idx) {
            self.graph.add_edge(r, s, TraceEdge::Formalisation);
            true
        } else {
            false
        }
    }

    /// Link spec → code element
    pub fn link_spec_to_code(&mut self, spec_file: &Path, code_file: &Path, code_name: &str) -> bool {
        let spec_idx = self.spec_index.get(spec_file).copied();
        let code_key = code_element_key(code_file, code_name);
        let code_idx = self.code_index.get(&code_key).copied();
        if let (Some(s), Some(c)) = (spec_idx, code_idx) {
            self.graph.add_edge(s, c, TraceEdge::Implementation);
            true
        } else {
            false
        }
    }

    /// Link code element → test
    pub fn link_code_to_test(&mut self, code_file: &Path, code_name: &str, test_file: &Path, test_name: &str) -> bool {
        let code_key = code_element_key(code_file, code_name);
        let code_idx = self.code_index.get(&code_key).copied();
        // Find specific test by name in the file
        let test_idx = self.test_index.get(test_file).and_then(|indices| {
            indices.iter().find(|&&idx| {
                matches!(&self.graph[idx], TraceNode::Test(t) if t.name == test_name)
            }).copied()
        });
        if let (Some(c), Some(t)) = (code_idx, test_idx) {
            self.graph.add_edge(c, t, TraceEdge::Verification);
            true
        } else {
            false
        }
    }

    // --- Accessors ---

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

impl Default for TraceGraph {
    fn default() -> Self {
        Self::new()
    }
}

// --- Helpers ---

pub(crate) fn code_element_key(file: &Path, name: &str) -> String {
    format!("{}:{}", file.display(), name)
}
