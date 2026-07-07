//! Query methods for the traceability graph.

use petgraph::Direction;
use std::path::Path;

use super::types::*;
use super::{code_element_key, TraceGraph};

impl TraceGraph {
    // --- Query APIs ---

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

    // --- Accessors ---

    pub fn all_requirements(&self) -> Vec<&Requirement> {
        self.req_index.values().filter_map(|&idx| self.get_requirement(idx)).collect()
    }

    pub fn all_specs(&self) -> Vec<&Spec> {
        self.spec_index.values().filter_map(|&idx| self.get_spec(idx)).collect()
    }

    pub(crate) fn get_requirement(&self, idx: petgraph::graph::NodeIndex) -> Option<&Requirement> {
        match &self.graph[idx] {
            TraceNode::Requirement(r) => Some(r),
            _ => None,
        }
    }

    pub(crate) fn get_spec(&self, idx: petgraph::graph::NodeIndex) -> Option<&Spec> {
        match &self.graph[idx] {
            TraceNode::Spec(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn get_code_element(&self, idx: petgraph::graph::NodeIndex) -> Option<&CodeElement> {
        match &self.graph[idx] {
            TraceNode::CodeElement(e) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn get_test(&self, idx: petgraph::graph::NodeIndex) -> Option<&Test> {
        match &self.graph[idx] {
            TraceNode::Test(t) => Some(t),
            _ => None,
        }
    }
}
