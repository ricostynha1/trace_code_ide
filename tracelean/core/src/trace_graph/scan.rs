//! Scan methods for building the traceability graph from filesystem conventions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::types::*;
use super::{code_element_key, TraceGraph};

impl TraceGraph {
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
        // Requirement → Spec by filename match
        let req_ids: Vec<(ReqId, petgraph::graph::NodeIndex)> = self.req_index.iter()
            .map(|(id, &idx)| (id.clone(), idx))
            .collect();
        for (req_id, req_idx) in &req_ids {
            // Look for spec file named after req_id
            let spec_file = PathBuf::from("specs").join(format!("{}.lean", req_id));
            if let Some(&spec_idx) = self.spec_index.get(&spec_file) {
                self.graph.add_edge(*req_idx, spec_idx, TraceEdge::Formalisation);
            }
        }

        // CodeElement → Test by path convention
        // Convention: tests/unit/test_<source_file>.rs tests code in src/<source_file>.rs
        let test_files: Vec<(PathBuf, Vec<petgraph::graph::NodeIndex>)> = self.test_index.iter()
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

        // Integration tests → Requirements by filename
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

    // --- Graph update on file change ---

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
        let mut sorted: Vec<petgraph::graph::NodeIndex> = indices;
        sorted.sort_by(|a, b| b.index().cmp(&a.index()));
        for idx in sorted {
            self.graph.remove_node(idx);
        }

        // After removal, indices shift. Rebuild.
        self.rebuild_indices();
        true
    }

    /// Rebuild all secondary indices from graph contents.
    pub(crate) fn rebuild_indices(&mut self) {
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
}

// --- File parsing helpers ---

/// Parse a requirement markdown file. Expected format:
/// ```text
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

/// Public wrapper for parse_requirement_file (used by lib.rs for incremental updates)
pub fn parse_requirement_file_public(path: &Path, root: &Path) -> Option<Requirement> {
    parse_requirement_file(path, root)
}
