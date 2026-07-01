//! Traceability Graph — MVP 2
//!
//! In-memory directed graph linking Requirements → Specs → CodeElements → Tests.
//! Built from filesystem conventions on startup, updated incrementally on file change.
//! Uses petgraph for storage and traversal.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// --- Node Types ---

/// Unique ID for a requirement (e.g. "REQ-001")
pub type ReqId = String;

/// Graph node — one of the four traceability node types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceNode {
    Requirement(Requirement),
    Spec(Spec),
    CodeElement(CodeElement),
    Test(Test),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: ReqId,
    pub title: String,
    pub status: ReqStatus,
    pub file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReqStatus {
    Draft,
    Approved,
    Linked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub req_id: ReqId,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeElement {
    pub name: String,
    pub kind: CodeElementKind,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeElementKind {
    Function,
    Method,
    Class,
    Struct,
    Module,
    Trait,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub name: String,
    pub kind: TestKind,
    pub file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestKind {
    Unit,
    Integration,
}

// --- Edge Types ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceEdge {
    /// Requirement → Spec (formalisation)
    Formalisation,
    /// Spec → CodeElement (implementation)
    Implementation,
    /// CodeElement → Test (verification)
    Verification,
    /// Requirement → Test (direct integration test)
    DirectTest,
}

// --- The Graph ---

#[derive(Debug)]
pub struct TraceGraph {
    graph: DiGraph<TraceNode, TraceEdge>,
    /// Fast lookups: requirement ID → node index
    req_index: HashMap<ReqId, NodeIndex>,
    /// Fast lookups: file path → node indices in that file
    file_index: HashMap<PathBuf, Vec<NodeIndex>>,
    /// Spec file → node index
    spec_index: HashMap<PathBuf, NodeIndex>,
    /// Code element key (file:name) → node index
    code_index: HashMap<String, NodeIndex>,
    /// Test file → node index
    test_index: HashMap<PathBuf, Vec<NodeIndex>>,
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

    // --- Query APIs (Task 2.9, 2.10) ---

    /// Given a requirement ID, return linked specs, code elements, and tests.
    pub fn query_requirement(&self, req_id: &str) -> Option<RequirementTrace<'_>> {
        let &req_idx = self.req_index.get(req_id)?;
        let req_node = self.get_requirement(req_idx)?;

        // Direct specs (Requirement → Spec)
        let specs: Vec<&Spec> = self.graph
            .neighbors_directed(req_idx, Direction::Outgoing)
            .filter_map(|idx| self.get_spec(idx))
            .collect();

        // Code elements (Spec → CodeElement)
        let code: Vec<&CodeElement> = specs.iter().flat_map(|_| {
            self.graph.neighbors_directed(req_idx, Direction::Outgoing)
                .flat_map(|spec_idx| {
                    self.graph.neighbors_directed(spec_idx, Direction::Outgoing)
                        .filter_map(|idx| self.get_code_element(idx))
                })
        }).collect();

        // Tests (CodeElement → Test) + direct (Requirement → Test)
        let mut tests: Vec<&Test> = Vec::new();
        // Direct tests
        for neighbor in self.graph.neighbors_directed(req_idx, Direction::Outgoing) {
            if let Some(t) = self.get_test(neighbor) {
                tests.push(t);
            }
        }
        // Tests via code
        for elem in &code {
            let key = code_element_key(&elem.file, &elem.name);
            if let Some(&code_idx) = self.code_index.get(&key) {
                for neighbor in self.graph.neighbors_directed(code_idx, Direction::Outgoing) {
                    if let Some(t) = self.get_test(neighbor) {
                        tests.push(t);
                    }
                }
            }
        }

        Some(RequirementTrace {
            requirement: req_node,
            specs,
            code,
            tests,
        })
    }

    /// Given a code element (file + name), return linked requirements and specs.
    pub fn query_code_element(&self, file: &Path, name: &str) -> Option<CodeElementTrace<'_>> {
        let key = code_element_key(file, name);
        let &code_idx = self.code_index.get(&key)?;
        let elem = self.get_code_element(code_idx)?;

        // Incoming specs (Spec → CodeElement)
        let specs: Vec<&Spec> = self.graph
            .neighbors_directed(code_idx, Direction::Incoming)
            .filter_map(|idx| self.get_spec(idx))
            .collect();

        // Requirements that point to those specs
        let reqs: Vec<&Requirement> = specs.iter().flat_map(|spec| {
            let spec_file = &spec.file;
            self.spec_index.get(spec_file).into_iter().flat_map(|&spec_idx| {
                self.graph.neighbors_directed(spec_idx, Direction::Incoming)
                    .filter_map(|idx| self.get_requirement(idx))
            })
        }).collect();

        // Outgoing tests
        let tests: Vec<&Test> = self.graph
            .neighbors_directed(code_idx, Direction::Outgoing)
            .filter_map(|idx| self.get_test(idx))
            .collect();

        Some(CodeElementTrace {
            element: elem,
            requirements: reqs,
            specs,
            tests,
        })
    }

    // --- Graph update on file change (Task 2.11) ---

    /// Remove all nodes and edges associated with a file, return true if anything was removed
    pub fn remove_file(&mut self, file: &Path) -> bool {
        let indices = match self.file_index.remove(file) {
            Some(indices) => indices,
            None => return false,
        };

        // Clean up secondary indices
        for &idx in &indices {
            match &self.graph[idx] {
                TraceNode::Requirement(r) => { self.req_index.remove(&r.id); }
                TraceNode::Spec(_) => { self.spec_index.remove(file); }
                TraceNode::CodeElement(e) => {
                    let key = code_element_key(&e.file, &e.name);
                    self.code_index.remove(&key);
                }
                TraceNode::Test(_) => { self.test_index.remove(file); }
            }
        }

        // Remove nodes (in reverse order to keep indices valid)
        // petgraph: removing a node swaps with last, so we sort descending
        let mut sorted: Vec<NodeIndex> = indices;
        sorted.sort_by(|a, b| b.index().cmp(&a.index()));
        for idx in sorted {
            self.graph.remove_node(idx);
        }

        // NOTE: After removal, indices shift. A full rebuild of indices is safer.
        // For MVP 2 we do a targeted rebuild.
        self.rebuild_indices();
        true
    }

    /// Rebuild all secondary indices from graph contents.
    /// Called after node removal (indices may have shifted).
    fn rebuild_indices(&mut self) {
        self.req_index.clear();
        self.file_index.clear();
        self.spec_index.clear();
        self.code_index.clear();
        self.test_index.clear();

        for idx in self.graph.node_indices() {
            match &self.graph[idx] {
                TraceNode::Requirement(r) => {
                    self.req_index.insert(r.id.clone(), idx);
                    self.file_index.entry(r.file.clone()).or_default().push(idx);
                }
                TraceNode::Spec(s) => {
                    self.spec_index.insert(s.file.clone(), idx);
                    self.file_index.entry(s.file.clone()).or_default().push(idx);
                }
                TraceNode::CodeElement(e) => {
                    let key = code_element_key(&e.file, &e.name);
                    self.code_index.insert(key, idx);
                    self.file_index.entry(e.file.clone()).or_default().push(idx);
                }
                TraceNode::Test(t) => {
                    self.test_index.entry(t.file.clone()).or_default().push(idx);
                    self.file_index.entry(t.file.clone()).or_default().push(idx);
                }
            }
        }
    }

    // --- Scanning (Tasks 2.5–2.8) ---

    /// Scan project directories and build the graph.
    /// Convention:
    ///   - `reqs/*.md` → Requirements (parsed for ID in frontmatter/heading)
    ///   - `specs/<REQ_ID>.lean` → Specs linked to requirement by filename
    ///   - `src/**/*` → CodeElements (from SymbolTable)
    ///   - `tests/unit/**` → Unit tests; `tests/integration/**` → Integration tests
    pub fn scan_project(
        &mut self,
        root: &Path,
        symbols: &HashMap<PathBuf, Vec<crate::parser::Symbol>>,
    ) {
        self.scan_requirements(root);
        self.scan_specs(root);
        self.scan_code_elements(symbols);
        self.scan_tests(root);
        self.link_by_convention(root);
    }

    fn scan_requirements(&mut self, root: &Path) {
        let reqs_dir = root.join("reqs");
        if !reqs_dir.is_dir() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&reqs_dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Some(req) = parse_requirement_file(&path, root) {
                self.add_requirement(req);
            }
        }
    }

    fn scan_specs(&mut self, root: &Path) {
        let specs_dir = root.join("specs");
        if !specs_dir.is_dir() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&specs_dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lean") {
                continue;
            }
            let file_stem = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            // Convention: spec filename = requirement ID (e.g. REQ-001.lean)
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let spec = Spec {
                id: file_stem.clone(),
                req_id: file_stem,
                file: rel,
            };
            self.add_spec(spec);
        }
    }

    fn scan_code_elements(&mut self, symbols: &HashMap<PathBuf, Vec<crate::parser::Symbol>>) {
        for (file, syms) in symbols {
            for sym in syms {
                let kind = match sym.kind {
                    crate::parser::SymbolKind::Function => CodeElementKind::Function,
                    crate::parser::SymbolKind::Method => CodeElementKind::Method,
                    crate::parser::SymbolKind::Class => CodeElementKind::Class,
                    crate::parser::SymbolKind::Struct => CodeElementKind::Struct,
                    crate::parser::SymbolKind::Module => CodeElementKind::Module,
                    crate::parser::SymbolKind::Trait => CodeElementKind::Trait,
                    // Skip Impl, Enum, Variable — not primary trace targets
                    _ => continue,
                };
                self.add_code_element(CodeElement {
                    name: sym.name.clone(),
                    kind,
                    file: file.clone(),
                    start_line: sym.start_line,
                    end_line: sym.end_line,
                });
            }
        }
    }

    fn scan_tests(&mut self, root: &Path) {
        self.scan_test_dir(&root.join("tests").join("unit"), root, TestKind::Unit);
        self.scan_test_dir(&root.join("tests").join("integration"), root, TestKind::Integration);
    }

    fn scan_test_dir(&mut self, dir: &Path, root: &Path, kind: TestKind) {
        if !dir.is_dir() {
            return;
        }
        self.scan_test_dir_recursive(dir, root, &kind);
    }

    fn scan_test_dir_recursive(&mut self, dir: &Path, root: &Path, kind: &TestKind) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_test_dir_recursive(&path, root, kind);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                self.add_test(Test {
                    name,
                    kind: kind.clone(),
                    file: rel,
                });
            }
        }
    }

    /// Link nodes by naming conventions after scan
    fn link_by_convention(&mut self, _root: &Path) {
        // Task 2.6: Requirement → Spec by filename match
        let req_ids: Vec<(ReqId, NodeIndex)> = self.req_index.iter()
            .map(|(id, &idx)| (id.clone(), idx))
            .collect();
        for (req_id, req_idx) in &req_ids {
            // Look for spec file named after req_id
            let spec_file = PathBuf::from("specs").join(format!("{}.lean", req_id));
            if let Some(&spec_idx) = self.spec_index.get(&spec_file) {
                self.graph.add_edge(*req_idx, spec_idx, TraceEdge::Formalisation);
            }
        }

        // Task 2.8: CodeElement → Test by path convention
        // Convention: tests/unit/test_<source_file>.rs tests code in src/<source_file>.rs
        let test_files: Vec<(PathBuf, Vec<NodeIndex>)> = self.test_index.iter()
            .map(|(f, indices)| (f.clone(), indices.clone()))
            .collect();
        for (test_file, test_indices) in &test_files {
            // Extract source file name from test file name
            if let Some(source_file) = test_to_source_path(test_file) {
                // Find all code elements in that source file
                if let Some(code_indices) = self.file_index.get(&source_file) {
                    for &code_idx in code_indices {
                        if matches!(&self.graph[code_idx], TraceNode::CodeElement(_)) {
                            for &test_idx in test_indices {
                                self.graph.add_edge(code_idx, test_idx, TraceEdge::Verification);
                            }
                        }
                    }
                }
            }
        }

        // Task 2.8: Integration tests → Requirements by filename
        // Convention: tests/integration/req_01_*.rs → REQ-01
        for (test_file, test_indices) in &test_files {
            if let Some(req_id) = integration_test_to_req_id(test_file) {
                if let Some(&req_idx) = self.req_index.get(&req_id) {
                    for &test_idx in test_indices {
                        self.graph.add_edge(req_idx, test_idx, TraceEdge::DirectTest);
                    }
                }
            }
        }
    }

    // --- Accessors ---

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn all_requirements(&self) -> Vec<&Requirement> {
        self.req_index.values().filter_map(|&idx| self.get_requirement(idx)).collect()
    }

    pub fn all_specs(&self) -> Vec<&Spec> {
        self.spec_index.values().filter_map(|&idx| self.get_spec(idx)).collect()
    }

    fn get_requirement(&self, idx: NodeIndex) -> Option<&Requirement> {
        match &self.graph[idx] {
            TraceNode::Requirement(r) => Some(r),
            _ => None,
        }
    }

    fn get_spec(&self, idx: NodeIndex) -> Option<&Spec> {
        match &self.graph[idx] {
            TraceNode::Spec(s) => Some(s),
            _ => None,
        }
    }

    fn get_code_element(&self, idx: NodeIndex) -> Option<&CodeElement> {
        match &self.graph[idx] {
            TraceNode::CodeElement(e) => Some(e),
            _ => None,
        }
    }

    fn get_test(&self, idx: NodeIndex) -> Option<&Test> {
        match &self.graph[idx] {
            TraceNode::Test(t) => Some(t),
            _ => None,
        }
    }

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

impl Default for TraceGraph {
    fn default() -> Self {
        Self::new()
    }
}

// --- Query result types ---

#[derive(Debug, Clone, Serialize)]
pub struct RequirementTrace<'a> {
    pub requirement: &'a Requirement,
    pub specs: Vec<&'a Spec>,
    pub code: Vec<&'a CodeElement>,
    pub tests: Vec<&'a Test>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeElementTrace<'a> {
    pub element: &'a CodeElement,
    pub requirements: Vec<&'a Requirement>,
    pub specs: Vec<&'a Spec>,
    pub tests: Vec<&'a Test>,
}

// --- File parsing helpers ---

/// Parse a requirement markdown file. Expected format:
/// ```
/// # REQ-001: Title here
/// Status: draft|approved|linked
///
/// Description text...
/// ```
fn parse_requirement_file(path: &Path, root: &Path) -> Option<Requirement> {
    let content = std::fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;

    // Parse "# REQ-XXX: Title"
    let heading = first_line.strip_prefix("# ")?;
    let (id, title) = heading.split_once(':')?;
    let id = id.trim().to_string();
    let title = title.trim().to_string();

    // Parse status from second meaningful line
    let status = content.lines()
        .find(|line| line.starts_with("Status:"))
        .and_then(|line| {
            let val = line.strip_prefix("Status:")?.trim().to_lowercase();
            match val.as_str() {
                "draft" => Some(ReqStatus::Draft),
                "approved" => Some(ReqStatus::Approved),
                "linked" => Some(ReqStatus::Linked),
                _ => Some(ReqStatus::Draft),
            }
        })
        .unwrap_or(ReqStatus::Draft);

    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    Some(Requirement { id, title, status, file: rel })
}

/// Convert test file path → source file path by convention.
/// `tests/unit/test_foo.rs` → `src/foo.rs`
/// `tests/unit/parser/test_lexer.rs` → `src/parser/lexer.rs`
fn test_to_source_path(test_file: &Path) -> Option<PathBuf> {
    let test_str = test_file.to_str()?;
    // Must be under tests/unit/
    let rest = test_str.strip_prefix("tests/unit/")?;
    let rest_path = Path::new(rest);
    let file_name = rest_path.file_name()?.to_str()?;

    // Strip test_ prefix
    let source_name = file_name.strip_prefix("test_")?;
    let parent = rest_path.parent().unwrap_or(Path::new(""));

    let source = PathBuf::from("src").join(parent).join(source_name);
    Some(source)
}

/// Extract requirement ID from integration test filename.
/// `tests/integration/req_01_something.rs` → `REQ-01`
fn integration_test_to_req_id(test_file: &Path) -> Option<String> {
    let test_str = test_file.to_str()?;
    let rest = test_str.strip_prefix("tests/integration/")?;
    let file_stem = Path::new(rest).file_stem()?.to_str()?;

    // Convention: req_XX_description
    if file_stem.starts_with("req_") {
        let parts: Vec<&str> = file_stem.splitn(3, '_').collect();
        if parts.len() >= 2 {
            let num = parts[1];
            return Some(format!("REQ-{}", num));
        }
    }
    None
}

fn code_element_key(file: &Path, name: &str) -> String {
    format!("{}:{}", file.display(), name)
}

// --- Serializable query result types for IPC ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementTraceOwned {
    pub requirement: Requirement,
    pub specs: Vec<Spec>,
    pub code: Vec<CodeElement>,
    pub tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeElementTraceOwned {
    pub element: CodeElement,
    pub requirements: Vec<Requirement>,
    pub specs: Vec<Spec>,
    pub tests: Vec<Test>,
}

impl TraceGraph {
    /// Query requirement → owned result (for serialization across IPC)
    pub fn query_requirement_owned(&self, req_id: &str) -> Option<RequirementTraceOwned> {
        let trace = self.query_requirement(req_id)?;
        Some(RequirementTraceOwned {
            requirement: trace.requirement.clone(),
            specs: trace.specs.into_iter().cloned().collect(),
            code: trace.code.into_iter().cloned().collect(),
            tests: trace.tests.into_iter().cloned().collect(),
        })
    }

    /// Query code element → owned result (for serialization across IPC)
    pub fn query_code_element_owned(&self, file: &Path, name: &str) -> Option<CodeElementTraceOwned> {
        let trace = self.query_code_element(file, name)?;
        Some(CodeElementTraceOwned {
            element: trace.element.clone(),
            requirements: trace.requirements.into_iter().cloned().collect(),
            specs: trace.specs.into_iter().cloned().collect(),
            tests: trace.tests.into_iter().cloned().collect(),
        })
    }
}

/// Public wrapper for parse_requirement_file (used by lib.rs for incremental updates)
pub fn parse_requirement_file_public(path: &Path, root: &Path) -> Option<Requirement> {
    parse_requirement_file(path, root)
}
