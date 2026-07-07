//! Type definitions for the traceability graph.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
