//! Tests for the Surgical Editing API.
//! Uses a mock agent that selects editing mode via policy and executes edits.
//! All commands produced MUST be Insert/Delete only — never Replace.

use tracelean_core::commands::Command;
use tracelean_core::parser::Lang;
use tracelean_core::surgical_edit::ast_edit::{AstEdit, AstEditOp, NodeSelector};
use tracelean_core::surgical_edit::error::SurgicalEditError;
use tracelean_core::surgical_edit::patch_edit::PatchEdit;
use tracelean_core::surgical_edit::policy::{EditIntent, EditMode, EditPolicy};
use tracelean_core::surgical_edit::search_replace::SearchReplace;
use tracelean_core::surgical_edit::EditResult;
use std::path::PathBuf;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn assert_only_insert_delete(commands: &[Command]) {
    for cmd in commands {
        match cmd {
            Command::Insert { .. } | Command::Delete { .. } => {}
            other => panic!("Only Insert/Delete allowed, got: {:?}", other),
        }
    }
}

/// Apply Insert/Delete commands to a string buffer.
fn apply_commands(content: &str, commands: &[Command]) -> String {
    let mut buf = content.to_string();
    for cmd in commands {
        match cmd {
            Command::Delete { offset, len, .. } => {
                buf.drain(*offset..(*offset + *len));
            }
            Command::Insert { offset, text, .. } => {
                buf.insert_str(*offset, text);
            }
            _ => panic!("Only Insert/Delete allowed"),
        }
    }
    buf
}

// ─── Mock Agent ──────────────────────────────────────────────────────────────

struct MockAgent {
    files: std::collections::HashMap<PathBuf, String>,
}

impl MockAgent {
    fn new() -> Self {
        Self { files: std::collections::HashMap::new() }
    }

    fn add_file(&mut self, path: impl Into<PathBuf>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    fn get_content(&self, path: &PathBuf) -> Option<&str> {
        self.files.get(path).map(|s| s.as_str())
    }

    fn execute_edit(
        &mut self,
        path: &PathBuf,
        intent: EditIntent,
        edit_request: MockEditRequest,
    ) -> Result<EditResult, SurgicalEditError> {
        let source = self.files.get(path)
            .ok_or_else(|| SurgicalEditError::FileNotFound(path.display().to_string()))?
            .clone();

        let mode = EditPolicy::recommend(&intent);
        let lang_ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let has_lang = Lang::from_extension(lang_ext).is_some();

        let effective_mode = if mode == EditMode::AstEdit && !has_lang {
            EditMode::SearchReplace
        } else {
            mode
        };

        let result = match (effective_mode, edit_request) {
            (EditMode::AstEdit, MockEditRequest::Ast { selector, op }) => {
                let lang = Lang::from_extension(lang_ext)
                    .ok_or_else(|| SurgicalEditError::UnsupportedLanguage(lang_ext.into()))?;
                AstEdit::new(selector, op).apply(path, &source, &lang)?
            }
            (EditMode::GitPatch, MockEditRequest::Patch { patch_text }) => {
                PatchEdit::parse(&patch_text)?.apply(path, &source)?
            }
            (EditMode::SearchReplace, MockEditRequest::SearchReplace { search, replacement, allow_multiple }) => {
                SearchReplace::new(search, replacement)
                    .with_allow_multiple(allow_multiple)
                    .apply(path, &source)?
            }
            (_, MockEditRequest::Ast { selector, op }) => {
                let lang = Lang::from_extension(lang_ext)
                    .ok_or_else(|| SurgicalEditError::UnsupportedLanguage(lang_ext.into()))?;
                AstEdit::new(selector, op).apply(path, &source, &lang)?
            }
            (_, MockEditRequest::Patch { patch_text }) => {
                PatchEdit::parse(&patch_text)?.apply(path, &source)?
            }
            (_, MockEditRequest::SearchReplace { search, replacement, allow_multiple }) => {
                SearchReplace::new(search, replacement)
                    .with_allow_multiple(allow_multiple)
                    .apply(path, &source)?
            }
        };

        // Validate: ONLY Insert/Delete
        assert_only_insert_delete(&result.commands);

        // Validate: applying commands to source yields new_content
        let reconstructed = apply_commands(&source, &result.commands);
        assert_eq!(reconstructed, result.new_content,
            "Commands don't reconstruct shadow buffer!");

        // Update in-memory file
        self.files.insert(path.clone(), result.new_content.clone());
        Ok(result)
    }
}

#[derive(Debug)]
enum MockEditRequest {
    Ast { selector: NodeSelector, op: AstEditOp },
    Patch { patch_text: String },
    SearchReplace { search: String, replacement: String, allow_multiple: bool },
}

// ─── Policy Tests ────────────────────────────────────────────────────────────

#[test]
fn test_policy_ast_preferred_for_structural() {
    assert_eq!(EditPolicy::recommend(&EditIntent::ModifyDeclaration), EditMode::AstEdit);
    assert_eq!(EditPolicy::recommend(&EditIntent::InsertDeclaration), EditMode::AstEdit);
    assert_eq!(EditPolicy::recommend(&EditIntent::DeleteDeclaration), EditMode::AstEdit);
    assert_eq!(EditPolicy::recommend(&EditIntent::ModifyImport), EditMode::AstEdit);
}

#[test]
fn test_policy_patch_for_multi_region() {
    assert_eq!(EditPolicy::recommend(&EditIntent::MultiRegion), EditMode::GitPatch);
}

#[test]
fn test_policy_search_replace_for_text() {
    assert_eq!(EditPolicy::recommend(&EditIntent::ReplaceLiteral), EditMode::SearchReplace);
    assert_eq!(EditPolicy::recommend(&EditIntent::UpdateComment), EditMode::SearchReplace);
}

// ─── AST Edit via Mock Agent ─────────────────────────────────────────────────

#[test]
fn test_agent_ast_insert_function() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn existing() {\n    println!(\"hi\");\n}\n");

    let result = agent.execute_edit(&path, EditIntent::InsertDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("existing".into()), index: None },
        op: AstEditOp::InsertAfter { new_text: "\nfn new_function() {\n    // added\n}\n".into() },
    }).unwrap();

    assert_only_insert_delete(&result.commands);
    assert!(result.new_content.contains("fn new_function()"));
}

#[test]
fn test_agent_ast_delete_function() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn keep() {}\nfn remove_me() {\n    deprecated();\n}\nfn also_keep() {}\n");

    let result = agent.execute_edit(&path, EditIntent::DeleteDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("remove_me".into()), index: None },
        op: AstEditOp::DeleteNode,
    }).unwrap();

    assert_only_insert_delete(&result.commands);
    assert!(!result.new_content.contains("remove_me"));
    assert!(result.new_content.contains("keep"));
}

#[test]
fn test_agent_ast_replace_function_body() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/upload.rs");
    agent.add_file(&path, "fn validate_upload(file: &File) -> bool {\n    file.size() < MAX_SIZE\n}\n");

    let result = agent.execute_edit(&path, EditIntent::ModifyDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("validate_upload".into()), index: None },
        op: AstEditOp::ReplaceNode {
            new_text: "fn validate_upload(file: &File) -> Result<(), UploadError> {\n    if file.size() > MAX_SIZE {\n        return Err(UploadError::TooLarge);\n    }\n    Ok(())\n}".into(),
        },
    }).unwrap();

    assert_only_insert_delete(&result.commands);
    assert!(result.new_content.contains("Result<(), UploadError>"));
    assert!(!result.new_content.contains("-> bool"));
}

#[test]
fn test_agent_ast_node_not_found() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn existing() {}\n");

    let result = agent.execute_edit(&path, EditIntent::DeleteDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("ghost".into()), index: None },
        op: AstEditOp::DeleteNode,
    });
    assert!(matches!(result, Err(SurgicalEditError::NodeNotFound(_))));
    assert_eq!(agent.get_content(&path).unwrap(), "fn existing() {}\n");
}

#[test]
fn test_agent_ast_ambiguous_node() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn dup() {}\nfn dup() {}\n");

    let result = agent.execute_edit(&path, EditIntent::DeleteDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("dup".into()), index: None },
        op: AstEditOp::DeleteNode,
    });
    assert!(matches!(result, Err(SurgicalEditError::AmbiguousNode { .. })));
}

// ─── Git Patch via Mock Agent ────────────────────────────────────────────────

#[test]
fn test_agent_patch_add_line() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/models.rs");
    agent.add_file(&path, "pub enum UploadError {\n    EmptyFilename,\n    TooLarge,\n    IoError(String),\n}\n");

    let patch = " pub enum UploadError {\n     EmptyFilename,\n     TooLarge,\n     IoError(String),\n+    Unauthorized,\n }\n";
    let result = agent.execute_edit(&path, EditIntent::MultiRegion,
        MockEditRequest::Patch { patch_text: patch.into() }).unwrap();

    assert_only_insert_delete(&result.commands);
    assert!(result.new_content.contains("Unauthorized"));
}

#[test]
fn test_agent_patch_context_mismatch_fails() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn real_code() {}\n");

    let result = agent.execute_edit(&path, EditIntent::MultiRegion,
        MockEditRequest::Patch { patch_text: " fn different_code() {}\n+fn added() {}\n".into() });
    assert!(matches!(result, Err(SurgicalEditError::PatchFailed(_))));
    assert_eq!(agent.get_content(&path).unwrap(), "fn real_code() {}\n");
}

// ─── Search & Replace via Mock Agent ─────────────────────────────────────────

#[test]
fn test_agent_search_replace_literal() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/config.rs");
    agent.add_file(&path, "const MAX_SIZE: u64 = 100 * 1024 * 1024;\nfn check() {}\n");

    let result = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "const MAX_SIZE: u64 = 100 * 1024 * 1024;".into(),
        replacement: "const MAX_SIZE: u64 = 200 * 1024 * 1024;".into(),
        allow_multiple: false,
    }).unwrap();

    assert_only_insert_delete(&result.commands);
    assert!(result.new_content.contains("200 * 1024 * 1024"));
}

#[test]
fn test_agent_search_replace_not_found() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "fn main() {}\n");

    let result = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "nonexistent".into(), replacement: "x".into(), allow_multiple: false,
    });
    assert!(matches!(result, Err(SurgicalEditError::SearchTextNotFound)));
}

#[test]
fn test_agent_search_replace_multiple_fails() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    agent.add_file(&path, "let x = TODO;\nlet y = TODO;\n");

    let result = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "TODO".into(), replacement: "done".into(), allow_multiple: false,
    });
    assert!(matches!(result, Err(SurgicalEditError::MultipleMatches(2))));
}

// ─── Transactional behavior ──────────────────────────────────────────────────

#[test]
fn test_all_edits_transactional_on_failure() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/lib.rs");
    let original = "fn real() {}\n";
    agent.add_file(&path, original);

    // AST: node not found
    let _ = agent.execute_edit(&path, EditIntent::DeleteDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("x".into()), index: None },
        op: AstEditOp::DeleteNode,
    });
    assert_eq!(agent.get_content(&path).unwrap(), original);

    // Patch: context mismatch
    let _ = agent.execute_edit(&path, EditIntent::MultiRegion,
        MockEditRequest::Patch { patch_text: " fn wrong() {}\n+added\n".into() });
    assert_eq!(agent.get_content(&path).unwrap(), original);

    // Search: not found
    let _ = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "ghost".into(), replacement: "x".into(), allow_multiple: false,
    });
    assert_eq!(agent.get_content(&path).unwrap(), original);
}

// ─── Multi-step workflow ─────────────────────────────────────────────────────

#[test]
fn test_agent_multi_step_edit_workflow() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("src/upload.rs");
    agent.add_file(&path, "use std::io;\n\nconst MAX_SIZE: u64 = 100 * 1024 * 1024;\n\nfn validate_upload(name: &str, size: u64) -> bool {\n    !name.is_empty() && size < MAX_SIZE\n}\n\nfn process_upload(data: &[u8]) {\n    // process\n}\n");

    // Step 1: search & replace constant
    let r = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "const MAX_SIZE: u64 = 100 * 1024 * 1024;".into(),
        replacement: "const MAX_SIZE: u64 = 200 * 1024 * 1024;".into(),
        allow_multiple: false,
    }).unwrap();
    assert_only_insert_delete(&r.commands);

    // Step 2: AST replace function
    let r = agent.execute_edit(&path, EditIntent::ModifyDeclaration, MockEditRequest::Ast {
        selector: NodeSelector { node_type: "function_item".into(), name: Some("validate_upload".into()), index: None },
        op: AstEditOp::ReplaceNode {
            new_text: "fn validate_upload(name: &str, size: u64) -> Result<(), String> {\n    if name.is_empty() {\n        return Err(\"Empty filename\".into());\n    }\n    if size > MAX_SIZE {\n        return Err(\"Too large\".into());\n    }\n    Ok(())\n}".into(),
        },
    }).unwrap();
    assert_only_insert_delete(&r.commands);

    let final_content = agent.get_content(&path).unwrap();
    assert!(final_content.contains("200 * 1024 * 1024"));
    assert!(final_content.contains("Result<(), String>"));
    assert!(!final_content.contains("100 * 1024 * 1024"));
    assert!(final_content.contains("fn process_upload"));
}

#[test]
fn test_agent_file_not_found() {
    let mut agent = MockAgent::new();
    let path = PathBuf::from("nonexistent.rs");
    let result = agent.execute_edit(&path, EditIntent::ReplaceLiteral, MockEditRequest::SearchReplace {
        search: "x".into(), replacement: "y".into(), allow_multiple: false,
    });
    assert!(matches!(result, Err(SurgicalEditError::FileNotFound(_))));
}
