//! Integration tests: Surgical Edit ↔ Undo Tree interaction.
//!
//! Scenario: user types words, undoes, then agent makes surgical edits.
//! ALL commands are the single Replace primitive. Undo = apply inverse (swap old/new).
//! Verifies ALL states reachable via undo/redo/jump_to.

use tracelean_core::commands::Command;
use tracelean_core::parser::Lang;
use tracelean_core::state::AppState;
use tracelean_core::surgical_edit::ast_edit::{AstEdit, AstEditOp, NodeSelector};
use tracelean_core::surgical_edit::patch_edit::PatchEdit;
use tracelean_core::surgical_edit::search_replace::SearchReplace;
use tracelean_core::undo_tree::NodeId;
use std::path::PathBuf;

/// Apply surgical edit commands into AppState. Returns node IDs created.
fn apply_surgical(state: &mut AppState, commands: Vec<Command>) -> Vec<NodeId> {
    // Validate: only Replace
    for cmd in &commands {
        match cmd {
            Command::Replace { .. } => {}
            other => panic!("Surgical edit produced non-Replace: {:?}", other),
        }
    }
    commands.into_iter().map(|cmd| {
        state.apply(cmd).expect("witness must match");
        state.undo_tree().current_node().unwrap().id
    }).collect()
}

// ─── Core test: User edits → Undo → Agent edit → All states reachable ───────

#[test]
fn test_user_edits_undo_agent_edit_all_states_reachable() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/main.rs");
    state.load_file(path.clone(), String::new());

    // User types "fn " (Insert at 0)
    state.apply(Command::insert(path.clone(), 0, "fn ".into())).unwrap();
    let node_w1 = state.undo_tree().current_node().unwrap().id;
    assert_eq!(state.get_content(&path).unwrap(), "fn ");

    // User types "hello" (Insert at 3)
    state.apply(Command::insert(path.clone(), 3, "hello".into())).unwrap();
    let node_w2 = state.undo_tree().current_node().unwrap().id;
    assert_eq!(state.get_content(&path).unwrap(), "fn hello");

    // User types "() {}" (Insert at 8)
    state.apply(Command::insert(path.clone(), 8, "() {}".into())).unwrap();
    let node_w3 = state.undo_tree().current_node().unwrap().id;
    assert_eq!(state.get_content(&path).unwrap(), "fn hello() {}");

    // UNDO "() {}"
    assert!(state.undo().changed);
    assert_eq!(state.get_content(&path).unwrap(), "fn hello");

    // UNDO "hello"
    assert!(state.undo().changed);
    assert_eq!(state.get_content(&path).unwrap(), "fn ");

    // Agent: search&replace "fn " → "fn main() {\n    println!(\"hi\");\n}\n"
    let content = state.get_content(&path).unwrap().to_string();
    let sr = SearchReplace::new("fn ", "fn main() {\n    println!(\"hi\");\n}\n");
    let result = sr.apply(&path, &content).unwrap();
    let agent_nodes = apply_surgical(&mut state, result.commands);
    // May produce 1 or 2 commands (delete "fn " + insert new), let's grab last node
    let node_agent = *agent_nodes.last().unwrap();

    assert_eq!(state.get_content(&path).unwrap(), "fn main() {\n    println!(\"hi\");\n}\n");

    // === ALL states reachable ===

    // Undo all agent commands
    for _ in &agent_nodes {
        assert!(state.undo().changed);
    }
    assert_eq!(state.get_content(&path).unwrap(), "fn ");

    // Redo → goes to agent branch
    for _ in &agent_nodes {
        assert!(state.redo().changed);
    }
    assert_eq!(state.get_content(&path).unwrap(), "fn main() {\n    println!(\"hi\");\n}\n");

    // Jump to node_w2 (the old branch with "hello")
    let jump_cmds = state.jump_to_node(node_w2).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    assert_eq!(state.get_content(&path).unwrap(), "fn hello");

    // Jump to node_w3 ("fn hello() {}")
    let jump_cmds = state.jump_to_node(node_w3).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    assert_eq!(state.get_content(&path).unwrap(), "fn hello() {}");

    // Jump back to agent
    let jump_cmds = state.jump_to_node(node_agent).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    assert_eq!(state.get_content(&path).unwrap(), "fn main() {\n    println!(\"hi\");\n}\n");

    // Jump to word1
    let jump_cmds = state.jump_to_node(node_w1).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    assert_eq!(state.get_content(&path).unwrap(), "fn ");
}

// ─── AST edit creates proper undo branch ─────────────────────────────────────

#[test]
fn test_ast_edit_branching_in_undo_tree() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/lib.rs");
    state.load_file(path.clone(), "fn alpha() {\n    v1();\n}\n".into());

    // User adds beta function
    state.apply(Command::insert(
        path.clone(),
        state.get_content(&path).unwrap().chars().count(),
        "\nfn beta() {\n    original();\n}\n".into(),
    )).unwrap();
    let node_beta_added = state.undo_tree().current_node().unwrap().id;

    // User modifies beta via search&replace
    let content = state.get_content(&path).unwrap().to_string();
    let sr = SearchReplace::new("original()", "modified()");
    let result = sr.apply(&path, &content).unwrap();
    let mod_nodes = apply_surgical(&mut state, result.commands);
    let node_beta_modified = *mod_nodes.last().unwrap();

    // Undo all modification commands
    for _ in &mod_nodes {
        assert!(state.undo().changed);
    }
    assert!(state.get_content(&path).unwrap().contains("original()"));

    // Agent: AST edit on alpha
    let content = state.get_content(&path).unwrap().to_string();
    let lang = Lang::from_extension("rs").unwrap();
    let ast_edit = AstEdit::new(
        NodeSelector { node_type: "function_item".into(), name: Some("alpha".into()), index: None },
        AstEditOp::ReplaceNode { new_text: "fn alpha() {\n    v2_agent();\n}".into() },
    );
    let result = ast_edit.apply(&path, &content, &lang).unwrap();
    let agent_nodes = apply_surgical(&mut state, result.commands);
    let node_agent_alpha = *agent_nodes.last().unwrap();

    // Current: alpha=v2_agent, beta=original
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("v2_agent()"));
    assert!(content.contains("original()"));

    // === All states reachable ===

    // Jump to modified beta (alpha should be v1 in that branch)
    let jump_cmds = state.jump_to_node(node_beta_modified).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("modified()"));
    assert!(content.contains("v1()"));

    // Jump back to agent
    let jump_cmds = state.jump_to_node(node_agent_alpha).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("v2_agent()"));
    assert!(content.contains("original()"));
}

// ─── Git Patch + undo roundtrip ──────────────────────────────────────────────

#[test]
fn test_patch_edit_undo_roundtrip() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/config.rs");
    let original = "const A: u32 = 1;\nconst B: u32 = 2;\nconst C: u32 = 3;\n";
    state.load_file(path.clone(), original.into());

    let patch = " const A: u32 = 1;\n-const B: u32 = 2;\n+const B: u32 = 20;\n const C: u32 = 3;\n";
    let edit = PatchEdit::parse(patch).unwrap();
    let result = edit.apply(&path, state.get_content(&path).unwrap()).unwrap();
    let nodes = apply_surgical(&mut state, result.commands);

    assert!(state.get_content(&path).unwrap().contains("const B: u32 = 20;"));

    // Undo → back to original
    for _ in &nodes { assert!(state.undo().changed); }
    assert_eq!(state.get_content(&path).unwrap(), original);

    // Redo → back to patched
    for _ in &nodes { assert!(state.redo().changed); }
    assert!(state.get_content(&path).unwrap().contains("const B: u32 = 20;"));
}

// ─── Interleaved user + agent edits, undo all ────────────────────────────────

#[test]
fn test_interleaved_user_and_agent_edits_undo_all() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/app.rs");
    state.load_file(path.clone(), "fn start() {}\n".into());

    // User edit 1: add function
    state.apply(Command::insert(path.clone(), 14, "\nfn user_fn() {}\n".into())).unwrap();
    let _node_user1 = state.undo_tree().current_node().unwrap().id;

    // Agent edit 1: search & replace
    let content = state.get_content(&path).unwrap().to_string();
    let sr = SearchReplace::new("fn start() {}", "fn start() { init(); }");
    let result = sr.apply(&path, &content).unwrap();
    let agent1_nodes = apply_surgical(&mut state, result.commands);

    // User edit 2: insert comment
    state.apply(Command::insert(path.clone(), 0, "// app module\n".into())).unwrap();

    // Agent edit 2: AST edit on user_fn
    let content = state.get_content(&path).unwrap().to_string();
    let lang = Lang::from_extension("rs").unwrap();
    let ast_edit = AstEdit::new(
        NodeSelector { node_type: "function_item".into(), name: Some("user_fn".into()), index: None },
        AstEditOp::ReplaceNode { new_text: "fn user_fn() { improved(); }".into() },
    );
    let result = ast_edit.apply(&path, &content, &lang).unwrap();
    let agent2_nodes = apply_surgical(&mut state, result.commands);

    // Final state
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("// app module"));
    assert!(content.contains("init()"));
    assert!(content.contains("improved()"));

    // === Undo ALL back to start ===
    // agent2
    for _ in &agent2_nodes { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("fn user_fn() {}"));
    assert!(content.contains("// app module"));

    // user2 (comment)
    assert!(state.undo().changed);
    let content = state.get_content(&path).unwrap();
    assert!(!content.contains("// app module"));
    assert!(content.contains("init()"));

    // agent1
    for _ in &agent1_nodes { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("fn start() {}"));
    assert!(content.contains("fn user_fn() {}"));

    // user1
    assert!(state.undo().changed);
    assert_eq!(state.get_content(&path).unwrap(), "fn start() {}\n");

    // root
    assert!(!state.undo().changed);

    // === Redo ALL forward ===
    assert!(state.redo().changed); // user1
    assert!(state.get_content(&path).unwrap().contains("fn user_fn() {}"));
    for _ in &agent1_nodes { assert!(state.redo().changed); } // agent1
    assert!(state.get_content(&path).unwrap().contains("init()"));
    assert!(state.redo().changed); // user2
    assert!(state.get_content(&path).unwrap().contains("// app module"));
    for _ in &agent2_nodes { assert!(state.redo().changed); } // agent2
    assert!(state.get_content(&path).unwrap().contains("improved()"));
    assert!(!state.redo().changed); // leaf
}

// ─── Branch preservation: old branch reachable after agent branch ────────────

#[test]
fn test_undo_branch_agent_edit_preserves_old_branch() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/lib.rs");
    state.load_file(path.clone(), "// base\n".into());

    // User path A
    state.apply(Command::insert(path.clone(), 8, "line A1\n".into())).unwrap();
    let _node_a1 = state.undo_tree().current_node().unwrap().id;
    state.apply(Command::insert(path.clone(), 16, "line A2\n".into())).unwrap();
    let node_a2 = state.undo_tree().current_node().unwrap().id;

    // Undo both
    assert!(state.undo().changed);
    assert!(state.undo().changed);
    assert_eq!(state.get_content(&path).unwrap(), "// base\n");

    // Agent creates branch B
    let content = state.get_content(&path).unwrap().to_string();
    let sr = SearchReplace::new("// base\n", "// base\nfn agent_added() {}\n");
    let result = sr.apply(&path, &content).unwrap();
    let agent_nodes = apply_surgical(&mut state, result.commands);
    let node_b = *agent_nodes.last().unwrap();

    assert!(state.get_content(&path).unwrap().contains("fn agent_added()"));

    // Old branch A still reachable
    let jump_cmds = state.jump_to_node(node_a2).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("line A1"));
    assert!(content.contains("line A2"));
    assert!(!content.contains("agent_added"));

    // Back to agent
    let jump_cmds = state.jump_to_node(node_b).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    assert!(state.get_content(&path).unwrap().contains("agent_added"));
    assert!(!state.get_content(&path).unwrap().contains("line A1"));
}

// ─── Consecutive agent edits undo independently ──────────────────────────────

#[test]
fn test_consecutive_agent_edits_undo_each() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/cfg.rs");
    state.load_file(path.clone(), "const A: u32 = 1;\nconst B: u32 = 2;\nconst C: u32 = 3;\n".into());

    // Agent edits A, B, C one by one
    let content = state.get_content(&path).unwrap().to_string();
    let r = SearchReplace::new("const A: u32 = 1;", "const A: u32 = 10;").apply(&path, &content).unwrap();
    let n1 = apply_surgical(&mut state, r.commands);

    let content = state.get_content(&path).unwrap().to_string();
    let r = SearchReplace::new("const B: u32 = 2;", "const B: u32 = 20;").apply(&path, &content).unwrap();
    let n2 = apply_surgical(&mut state, r.commands);

    let content = state.get_content(&path).unwrap().to_string();
    let r = SearchReplace::new("const C: u32 = 3;", "const C: u32 = 30;").apply(&path, &content).unwrap();
    let n3 = apply_surgical(&mut state, r.commands);

    // All changed
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("A: u32 = 10"));
    assert!(content.contains("B: u32 = 20"));
    assert!(content.contains("C: u32 = 30"));

    // Undo C
    for _ in &n3 { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("A: u32 = 10"));
    assert!(content.contains("B: u32 = 20"));
    assert!(content.contains("C: u32 = 3"));

    // Undo B
    for _ in &n2 { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("A: u32 = 10"));
    assert!(content.contains("B: u32 = 2"));
    assert!(content.contains("C: u32 = 3"));

    // Undo A
    for _ in &n1 { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("A: u32 = 1"));
    assert!(content.contains("B: u32 = 2"));
    assert!(content.contains("C: u32 = 3"));

    // Redo all
    for _ in &n1 { assert!(state.redo().changed); }
    for _ in &n2 { assert!(state.redo().changed); }
    for _ in &n3 { assert!(state.redo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("A: u32 = 10"));
    assert!(content.contains("B: u32 = 20"));
    assert!(content.contains("C: u32 = 30"));
}

// ─── AST edit after multi undo — content correctness ─────────────────────────

#[test]
fn test_agent_ast_edit_after_multi_undo() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    let path = PathBuf::from("src/math.rs");
    state.load_file(path.clone(), "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n".into());

    // User adds sub
    state.apply(Command::insert(
        path.clone(),
        state.get_content(&path).unwrap().chars().count(),
        "\nfn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n".into(),
    )).unwrap();

    // User adds mul
    state.apply(Command::insert(path.clone(), state.get_content(&path).unwrap().chars().count(), "\nfn mul(a: i32, b: i32) -> i32 {\n    a * b\n}\n".into())).unwrap();
    let node_with_mul = state.undo_tree().current_node().unwrap().id;

    // Undo mul
    assert!(state.undo().changed);
    assert!(!state.get_content(&path).unwrap().contains("fn mul"));

    // Agent AST-edits add()
    let content = state.get_content(&path).unwrap().to_string();
    let lang = Lang::from_extension("rs").unwrap();
    let ast_edit = AstEdit::new(
        NodeSelector { node_type: "function_item".into(), name: Some("add".into()), index: None },
        AstEditOp::ReplaceNode { new_text: "fn add(a: i32, b: i32) -> i32 {\n    a.wrapping_add(b)\n}".into() },
    );
    let result = ast_edit.apply(&path, &content, &lang).unwrap();
    let agent_nodes = apply_surgical(&mut state, result.commands);
    let node_agent = *agent_nodes.last().unwrap();

    // Verify: agent edit applied, mul absent, sub present
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("wrapping_add"));
    assert!(content.contains("fn sub"));
    assert!(!content.contains("fn mul"));

    // Undo agent
    for _ in &agent_nodes { assert!(state.undo().changed); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("a + b"));
    assert!(!content.contains("wrapping_add"));

    // Jump to mul state
    let jump_cmds = state.jump_to_node(node_with_mul).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("fn mul"));
    assert!(content.contains("a + b"));

    // Jump to agent state
    let jump_cmds = state.jump_to_node(node_agent).unwrap();
    for cmd in &jump_cmds { state.execute_raw(cmd); }
    let content = state.get_content(&path).unwrap();
    assert!(content.contains("wrapping_add"));
    assert!(!content.contains("fn mul"));
}
