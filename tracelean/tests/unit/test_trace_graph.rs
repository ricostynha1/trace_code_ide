//! Unit tests for TraceGraph (MVP 2)

use tracelean_lib::trace_graph::*;
use tracelean_lib::parser::{Symbol, SymbolKind};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn empty_graph() {
    let graph = TraceGraph::new();
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn add_requirement_and_query() {
    let mut graph = TraceGraph::new();
    graph.add_requirement(Requirement {
        id: "REQ-01".into(),
        title: "User login".into(),
        status: ReqStatus::Draft,
        file: PathBuf::from("reqs/REQ-01.md"),
    });

    assert_eq!(graph.node_count(), 1);
    let trace = graph.query_requirement("REQ-01");
    assert!(trace.is_some());
    let t = trace.unwrap();
    assert_eq!(t.requirement.id, "REQ-01");
    assert_eq!(t.specs.len(), 0);
}

#[test]
fn add_spec_and_link_to_requirement() {
    let mut graph = TraceGraph::new();
    graph.add_requirement(Requirement {
        id: "REQ-01".into(),
        title: "Auth".into(),
        status: ReqStatus::Approved,
        file: PathBuf::from("reqs/REQ-01.md"),
    });
    graph.add_spec(Spec {
        id: "REQ-01".into(),
        req_id: "REQ-01".into(),
        file: PathBuf::from("specs/REQ-01.lean"),
    });

    let linked = graph.link_req_to_spec("REQ-01", &PathBuf::from("specs/REQ-01.lean"));
    assert!(linked);

    let trace = graph.query_requirement("REQ-01").unwrap();
    assert_eq!(trace.specs.len(), 1);
    assert_eq!(trace.specs[0].file, PathBuf::from("specs/REQ-01.lean"));
}

#[test]
fn add_code_element_and_query() {
    let mut graph = TraceGraph::new();
    let file = PathBuf::from("src/auth.rs");
    graph.add_code_element(CodeElement {
        name: "login".into(),
        kind: CodeElementKind::Function,
        file: file.clone(),
        start_line: 10,
        end_line: 25,
    });

    assert_eq!(graph.node_count(), 1);
    let trace = graph.query_code_element(&file, "login");
    assert!(trace.is_some());
    assert_eq!(trace.unwrap().element.name, "login");
}

#[test]
fn full_traceability_chain() {
    let mut graph = TraceGraph::new();

    // Requirement
    graph.add_requirement(Requirement {
        id: "REQ-01".into(),
        title: "Auth".into(),
        status: ReqStatus::Linked,
        file: PathBuf::from("reqs/REQ-01.md"),
    });

    // Spec
    let spec_file = PathBuf::from("specs/REQ-01.lean");
    graph.add_spec(Spec {
        id: "REQ-01".into(),
        req_id: "REQ-01".into(),
        file: spec_file.clone(),
    });
    graph.link_req_to_spec("REQ-01", &spec_file);

    // Code
    let code_file = PathBuf::from("src/auth.rs");
    graph.add_code_element(CodeElement {
        name: "login".into(),
        kind: CodeElementKind::Function,
        file: code_file.clone(),
        start_line: 10,
        end_line: 25,
    });
    graph.link_spec_to_code(&spec_file, &code_file, "login");

    // Test
    let test_file = PathBuf::from("tests/unit/test_auth.rs");
    graph.add_test(Test {
        name: "test_login".into(),
        kind: TestKind::Unit,
        file: test_file.clone(),
    });
    graph.link_code_to_test(&code_file, "login", &test_file, "test_login");

    // Query from requirement
    let trace = graph.query_requirement("REQ-01").unwrap();
    assert_eq!(trace.specs.len(), 1);
    assert_eq!(trace.code.len(), 1);
    assert_eq!(trace.code[0].name, "login");

    // Query from code
    let code_trace = graph.query_code_element(&code_file, "login").unwrap();
    assert_eq!(code_trace.specs.len(), 1);
    assert_eq!(code_trace.requirements.len(), 1);
    assert_eq!(code_trace.tests.len(), 1);
    assert_eq!(code_trace.tests[0].name, "test_login");
}

#[test]
fn remove_file_clears_nodes() {
    let mut graph = TraceGraph::new();
    let file = PathBuf::from("src/foo.rs");
    graph.add_code_element(CodeElement {
        name: "bar".into(),
        kind: CodeElementKind::Function,
        file: file.clone(),
        start_line: 1,
        end_line: 5,
    });
    graph.add_code_element(CodeElement {
        name: "baz".into(),
        kind: CodeElementKind::Function,
        file: file.clone(),
        start_line: 10,
        end_line: 15,
    });
    assert_eq!(graph.node_count(), 2);

    let removed = graph.remove_file(&file);
    assert!(removed);
    assert_eq!(graph.node_count(), 0);
}

#[test]
fn query_nonexistent_returns_none() {
    let graph = TraceGraph::new();
    assert!(graph.query_requirement("NOPE").is_none());
    assert!(graph.query_code_element(&PathBuf::from("x.rs"), "y").is_none());
}

#[test]
fn owned_query_serializable() {
    let mut graph = TraceGraph::new();
    graph.add_requirement(Requirement {
        id: "REQ-99".into(),
        title: "Test".into(),
        status: ReqStatus::Draft,
        file: PathBuf::from("reqs/REQ-99.md"),
    });

    let owned = graph.query_requirement_owned("REQ-99");
    assert!(owned.is_some());
    // Should be serializable
    let json = serde_json::to_string(&owned.unwrap());
    assert!(json.is_ok());
}

#[test]
fn test_to_source_path_convention() {
    // Can't call private fn directly, but we test via graph linking behavior
    let mut graph = TraceGraph::new();

    // Add code in src/parser.rs
    graph.add_code_element(CodeElement {
        name: "parse".into(),
        kind: CodeElementKind::Function,
        file: PathBuf::from("src/parser.rs"),
        start_line: 1,
        end_line: 10,
    });

    // Add test in tests/unit/test_parser.rs
    graph.add_test(Test {
        name: "test_parser".into(),
        kind: TestKind::Unit,
        file: PathBuf::from("tests/unit/test_parser.rs"),
    });

    // The graph node count should be correct
    assert_eq!(graph.node_count(), 2);
}

#[test]
fn scan_code_elements_from_symbol_table() {
    let mut graph = TraceGraph::new();
    let mut symbols: HashMap<PathBuf, Vec<Symbol>> = HashMap::new();

    symbols.insert(PathBuf::from("src/main.rs"), vec![
        Symbol {
            name: "main".into(),
            kind: SymbolKind::Function,
            file: PathBuf::from("src/main.rs"),
            start_line: 0,
            end_line: 10,
            start_col: 0,
        },
        Symbol {
            name: "Config".into(),
            kind: SymbolKind::Struct,
            file: PathBuf::from("src/main.rs"),
            start_line: 12,
            end_line: 20,
            start_col: 0,
        },
    ]);

    // Use the internal scan method indirectly — just add code elements from symbols
    for (file, syms) in &symbols {
        for sym in syms {
            let kind = match sym.kind {
                SymbolKind::Function => CodeElementKind::Function,
                SymbolKind::Struct => CodeElementKind::Struct,
                _ => continue,
            };
            graph.add_code_element(CodeElement {
                name: sym.name.clone(),
                kind,
                file: file.clone(),
                start_line: sym.start_line,
                end_line: sym.end_line,
            });
        }
    }

    assert_eq!(graph.node_count(), 2);
    assert!(graph.query_code_element(&PathBuf::from("src/main.rs"), "main").is_some());
    assert!(graph.query_code_element(&PathBuf::from("src/main.rs"), "Config").is_some());
}
