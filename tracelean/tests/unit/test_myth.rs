//! Myth phase 4: dynamic action providers, the mode stack, and the lowering of
//! LSP workspace edits into invertible commands.

use std::path::{Path, PathBuf};

use serde_json::json;
use tracelean_lib::commands::Command;
use tracelean_lib::lsp::edits::{char_offset, lower_workspace_edit, LowerError, PositionEncoding};
use tracelean_lib::myth::keymap::{self, KeyResult, Keymap, ModeStack};
use tracelean_lib::myth::provider::{
    self, Action, ActionProvider, CursorCx, StaticProvider, TraceActionProvider,
};
use tracelean_lib::state::AppState;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/trace_project")
}

// --- the mode stack ---------------------------------------------------------

#[test]
fn escape_in_file_returns_to_options_not_main() {
    // The bug the stack exists to fix: the README documented "back to Options"
    // while every mode's Escape was spelled `→Main`, so File dropped two
    // levels. Now Escape pops exactly one.
    let km = Keymap::load();
    let mut stack = ModeStack::new();
    stack.apply(&km.interpret(stack.current(), "C-Space"));
    assert_eq!(stack.current(), "Options");
    stack.apply(&km.interpret(stack.current(), "f"));
    assert_eq!(stack.current(), "File");
    stack.apply(&km.interpret(stack.current(), "Escape"));
    assert_eq!(stack.current(), "Options");
    stack.apply(&km.interpret(stack.current(), "Escape"));
    assert_eq!(stack.current(), "Main");
}

#[test]
fn escape_at_the_root_is_a_no_op() {
    let mut stack = ModeStack::new();
    stack.pop();
    stack.pop();
    assert_eq!(stack.current(), "Main");
    assert_eq!(stack.depth(), 1);
}

#[test]
fn an_unbound_key_cancels_the_whole_stack() {
    let km = Keymap::load();
    let mut stack = ModeStack::new();
    stack.apply(&km.interpret(stack.current(), "C-Space"));
    stack.apply(&km.interpret(stack.current(), "f"));
    assert_eq!(stack.current(), "File");
    // `NM: reset` — two levels deep, one stray key returns to Main.
    stack.apply(&km.interpret(stack.current(), "%"));
    assert_eq!(stack.current(), "Main");
}

#[test]
fn dispatching_an_action_ends_the_mode() {
    let km = Keymap::load();
    let mut stack = ModeStack::new();
    stack.apply(&km.interpret(stack.current(), "C-Space"));
    let result = km.interpret(stack.current(), "u");
    assert!(matches!(result, KeyResult::Dispatch { .. }));
    stack.apply(&result);
    assert_eq!(stack.current(), "Main");
}

#[test]
fn the_stack_reports_its_path_for_a_breadcrumb() {
    let km = Keymap::load();
    let mut stack = ModeStack::new();
    stack.apply(&km.interpret(stack.current(), "C-Space"));
    stack.apply(&km.interpret(stack.current(), "t"));
    assert_eq!(stack.path(), &["Main".to_string(), "Options".into(), "Trace".into()]);
}

#[test]
fn the_new_modes_are_reachable_and_valid() {
    let km = Keymap::load();
    for mode in ["Goto", "Verify", "Trace", "CodeActions"] {
        assert!(km.has_state(mode), "keymap.json must define `{}`", mode);
    }
    let problems = km.validate(tracelean_lib::myth::actions::registry());
    assert!(problems.is_empty(), "keymap problems: {:?}", problems);
}

#[test]
fn which_key_labels_actions_with_titles() {
    let km = Keymap::load();
    let trace = km.bindings_for_state("Trace");
    let provenance = trace
        .iter()
        .find(|b| b.target == "goto_provenance")
        .expect("Trace mode binds goto_provenance");
    assert_eq!(provenance.title.as_deref(), Some("Who wrote this line"));
}

#[test]
fn dynamic_bindings_report_what_they_could_not_show() {
    let many: Vec<Action> = (0..40)
        .map(|i| Action::new("lsp_code_action", format!("fix {}", i), "quickfix", 30))
        .collect();
    let (bindings, dropped) = keymap::dynamic_bindings(&many);
    assert!(!bindings.is_empty());
    assert_eq!(
        bindings.len() + dropped,
        40,
        "a truncated menu must say how many entries it hid"
    );
    assert!(dropped > 0);
}

// --- providers --------------------------------------------------------------

#[test]
fn the_static_map_is_just_another_provider() {
    let cx = CursorCx {
        captures: vec!["file".to_string()],
        ..Default::default()
    };
    let actions = StaticProvider.actions_at(&cx);
    assert!(
        actions.iter().any(|a| a.name == "open_file"),
        "bindings.json binds open_file to the `file` capture: {:?}",
        actions
    );
    assert!(actions.iter().all(|a| a.provider == "static"));
}

#[test]
fn an_unannotated_line_is_offered_an_annotation() {
    let index = tracelean_lib::trace::build(&fixture_root());
    let provider = TraceActionProvider { index: &index };
    let cx = CursorCx {
        file: Some(PathBuf::from("docs/notes.md")),
        line: 0,
        ..Default::default()
    };
    let actions = provider.actions_at(&cx);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].name, "annotate");
}

#[test]
fn an_annotated_declaration_offers_navigation_and_evidence() {
    let index = tracelean_lib::trace::build(&fixture_root());
    let link = index
        .links
        .iter()
        .find(|l| l.anchor.file == PathBuf::from("src/auth.rs"))
        .expect("the fixture annotates src/auth.rs");
    let provider = TraceActionProvider { index: &index };
    let cx = CursorCx {
        file: Some(PathBuf::from("src/auth.rs")),
        line: link.anchor.start_line,
        ..Default::default()
    };
    let actions = provider.actions_at(&cx);
    for expected in ["goto_requirement", "goto_model", "goto_code", "show_evidence", "explain_gap"] {
        assert!(
            actions.iter().any(|a| a.name == expected),
            "expected `{}` at an annotated line: {:?}",
            expected,
            actions.iter().map(|a| &a.name).collect::<Vec<_>>()
        );
    }
    // The requirement the cursor is inside is carried, so the action does not
    // have to ask the user which requirement they meant.
    let goto = actions.iter().find(|a| a.name == "goto_requirement").unwrap();
    assert_eq!(goto.args.as_ref().unwrap()["req_id"], json!(link.req_id));
}

#[test]
fn collected_actions_are_ordered_by_priority_and_deduplicated() {
    struct Fake(Vec<Action>);
    impl ActionProvider for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn actions_at(&self, _cx: &CursorCx) -> Vec<Action> {
            self.0.clone()
        }
    }
    let a = Fake(vec![
        Action::new("low", "Low", "other", 1),
        Action::new("high", "High", "quickfix", 50),
    ]);
    // Same name, same (absent) args as one already offered: one entry, not two.
    let b = Fake(vec![
        Action::new("high", "High", "quickfix", 50),
        Action::new("mid", "Mid", "trace", 20),
    ]);
    let actions = provider::collect(&[&a, &b], &CursorCx::default());
    let names: Vec<&str> = actions.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["high", "mid", "low"]);
}

#[test]
fn groups_render_in_a_fixed_order() {
    let actions = vec![
        Action::new("x", "X", "file", 0),
        Action::new("y", "Y", "quickfix", 30),
        Action::new("z", "Z", "trace", 10),
    ];
    let groups: Vec<String> = provider::grouped(actions).into_iter().map(|(g, _)| g).collect();
    assert_eq!(groups, vec!["quickfix", "trace", "file"]);
}

#[test]
fn cursor_positions_convert_to_lsp_coordinates() {
    let content = "ab\ncdé\nf";
    // After "ab\ncd" → line 1, character 2.
    let (line, character) = provider::line_character(content, 5);
    assert_eq!((line, character), (1, 2));
    // "é" is one char but one UTF-16 unit; an emoji is two.
    let (_, ch) = provider::line_character("🙂x", 1);
    assert_eq!(ch, 2, "LSP counts UTF-16 code units, not characters");
}

// --- WorkspaceEdit lowering -------------------------------------------------

fn reader(content: &'static str) -> impl Fn(&Path) -> Option<String> {
    move |_| Some(content.to_string())
}

#[test]
fn a_simple_text_edit_becomes_a_replace_with_a_witness() {
    let content = "fn old() {}\n";
    let edit = json!({
        "changes": {
            "file:///proj/src/a.rs": [
                { "range": { "start": {"line": 0, "character": 3},
                             "end":   {"line": 0, "character": 6} },
                  "newText": "new" }
            ]
        }
    });
    let cmd = lower_workspace_edit(
        &edit,
        Path::new("/proj"),
        PositionEncoding::Utf16,
        &reader(content),
    )
    .unwrap();
    let Command::Batch { commands } = cmd else { panic!("expected a batch") };
    assert_eq!(commands.len(), 1);
    match &commands[0] {
        Command::Replace { file, at, old, new } => {
            assert_eq!(file, &PathBuf::from("src/a.rs"));
            assert_eq!(*at, 3);
            assert_eq!(old, "old");
            assert_eq!(new, "new");
        }
        other => panic!("expected Replace, got {:?}", other),
    }
}

#[test]
fn several_edits_in_one_file_apply_back_to_front() {
    // LSP edits are simultaneous; `Replace` commands are sequential against
    // absolute offsets, so the later edit must be emitted first or it would
    // land at a shifted position.
    let content = "aaa bbb ccc";
    let edit = json!({
        "changes": {
            "file:///p/x.rs": [
                { "range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":3}},
                  "newText": "LONGER" },
                { "range": {"start": {"line":0,"character":8}, "end": {"line":0,"character":11}},
                  "newText": "z" }
            ]
        }
    });
    let cmd =
        lower_workspace_edit(&edit, Path::new("/p"), PositionEncoding::Utf16, &reader(content))
            .unwrap();
    let Command::Batch { commands } = cmd else { panic!() };
    let ats: Vec<usize> = commands
        .iter()
        .map(|c| match c {
            Command::Replace { at, .. } => *at,
            other => panic!("expected Replace, got {:?}", other),
        })
        .collect();
    assert_eq!(ats, vec![8, 0], "later edits must come first");

    // And the whole batch really does produce the right buffer.
    let mut state = AppState::new();
    state.set_coalescing(false);
    state.load_file(PathBuf::from("x.rs"), content.to_string());
    for c in commands {
        let c = match c {
            Command::Replace { at, old, new, .. } => Command::Replace {
                file: PathBuf::from("x.rs"),
                at,
                old,
                new,
            },
            other => other,
        };
        state.apply(c).unwrap();
    }
    assert_eq!(
        state.get_content(&PathBuf::from("x.rs")).unwrap(),
        "LONGER bbb z"
    );
}

#[test]
fn document_changes_carry_renames_creates_and_deletes() {
    let edit = json!({
        "documentChanges": [
            { "kind": "create", "uri": "file:///p/new.rs" },
            { "textDocument": {"uri": "file:///p/new.rs", "version": 0},
              "edits": [ { "range": {"start":{"line":0,"character":0},
                                     "end":{"line":0,"character":0}},
                           "newText": "hi" } ] },
            { "kind": "rename", "oldUri": "file:///p/a.rs", "newUri": "file:///p/b.rs" },
            { "kind": "delete", "uri": "file:///p/old.rs" }
        ]
    });
    let cmd =
        lower_workspace_edit(&edit, Path::new("/p"), PositionEncoding::Utf16, &reader("")).unwrap();
    let Command::Batch { commands } = cmd else { panic!() };
    assert!(matches!(&commands[0], Command::CreateFile { path } if path == Path::new("new.rs")));
    assert!(matches!(&commands[1], Command::Replace { new, .. } if new == "hi"));
    assert!(
        matches!(&commands[2], Command::RenameFile { from, to }
            if from == Path::new("a.rs") && to == Path::new("b.rs"))
    );
    assert!(matches!(&commands[3], Command::DeleteFile { path, .. } if path == Path::new("old.rs")));
}

#[test]
fn a_lowered_rename_is_invertible_like_any_other_edit() {
    // The point of lowering at all: an LSP rename must undo like a hand edit,
    // and T1's anchors only follow a rename that is a command.
    let edit = json!({
        "documentChanges": [
            { "kind": "rename", "oldUri": "file:///p/a.rs", "newUri": "file:///p/b.rs" }
        ]
    });
    let cmd =
        lower_workspace_edit(&edit, Path::new("/p"), PositionEncoding::Utf16, &reader("")).unwrap();
    match cmd.inverse() {
        Command::Batch { commands } => match &commands[0] {
            Command::RenameFile { from, to } => {
                assert_eq!(from, Path::new("b.rs"));
                assert_eq!(to, Path::new("a.rs"));
            }
            other => panic!("expected RenameFile, got {:?}", other),
        },
        other => panic!("expected Batch, got {:?}", other),
    }
}

#[test]
fn an_edit_to_an_unopened_document_is_refused_whole() {
    let edit = json!({
        "changes": {
            "file:///p/a.rs": [ { "range": {"start":{"line":0,"character":0},
                                            "end":{"line":0,"character":0}},
                                  "newText": "x" } ]
        }
    });
    let err = lower_workspace_edit(
        &edit,
        Path::new("/p"),
        PositionEncoding::Utf16,
        &|_: &Path| None,
    )
    .unwrap_err();
    assert!(matches!(err, LowerError::UnknownDocument(_)));
}

#[test]
fn a_position_past_the_end_of_the_file_is_refused() {
    let edit = json!({
        "changes": {
            "file:///p/a.rs": [ { "range": {"start":{"line":9,"character":0},
                                            "end":{"line":9,"character":1}},
                                  "newText": "x" } ]
        }
    });
    let err = lower_workspace_edit(
        &edit,
        Path::new("/p"),
        PositionEncoding::Utf16,
        &reader("one line\n"),
    )
    .unwrap_err();
    assert!(
        matches!(err, LowerError::PositionOutOfRange { .. }),
        "a stale server view must fail loudly, got {:?}",
        err
    );
}

#[test]
fn utf16_positions_land_correctly_on_non_ascii_lines() {
    // The failure nobody catches in testing: "🙂" is one char and two UTF-16
    // units, so treating the two as interchangeable corrupts exactly the files
    // with emoji or accents in them.
    let content = "let s = \"🙂é\";";
    assert_eq!(char_offset(content, 0, 9, PositionEncoding::Utf16), Some(9));
    // After the emoji: 11 UTF-16 units in, 10 chars in.
    assert_eq!(char_offset(content, 0, 11, PositionEncoding::Utf16), Some(10));
    // The same position counted in chars (UTF-32) is a different offset.
    assert_eq!(char_offset(content, 0, 10, PositionEncoding::Utf32), Some(10));
}

#[test]
fn a_character_past_the_line_end_clamps_to_it() {
    // The spec's own rule, so not a guess: "if the character value is greater
    // than the line length it defaults back to the line length."
    let content = "ab\ncd\n";
    assert_eq!(char_offset(content, 0, 99, PositionEncoding::Utf16), Some(2));
}

// --- the code-action action --------------------------------------------------

#[test]
fn applying_a_code_action_produces_undoable_commands() {
    let mut state = AppState::new();
    state.set_project_root(PathBuf::from("/p"));
    state.load_file(PathBuf::from("a.rs"), "fn old() {}".to_string());
    let ctx = tracelean_lib::myth::ActionCtx {
        surface: "editor".into(),
        file: Some(PathBuf::from("a.rs")),
        args: Some(json!({
            "title": "rename to new",
            "kind": "quickfix",
            "edit": {
                "changes": {
                    "file:///p/a.rs": [
                        { "range": {"start":{"line":0,"character":3},
                                    "end":{"line":0,"character":6}},
                          "newText": "new" }
                    ]
                }
            }
        })),
        ..Default::default()
    };
    let outcome = tracelean_lib::myth::actions::registry()
        .dispatch("lsp_code_action", &state, &ctx)
        .unwrap();
    match outcome {
        tracelean_lib::myth::ActionOutcome::Commands { commands } => {
            assert_eq!(commands.len(), 1);
            assert!(matches!(&commands[0], Command::Batch { .. }));
        }
        other => panic!("expected Commands, got {:?}", other),
    }
}

#[test]
fn a_server_side_only_code_action_is_refused_rather_than_half_run() {
    let state = AppState::new();
    let ctx = tracelean_lib::myth::ActionCtx {
        file: Some(PathBuf::from("a.rs")),
        args: Some(json!({ "title": "organize imports", "command": {"command": "rust-analyzer.x"} })),
        ..Default::default()
    };
    let err = tracelean_lib::myth::actions::registry()
        .dispatch("lsp_code_action", &state, &ctx)
        .unwrap_err();
    assert!(err.contains("server-side command"), "got: {}", err);
}

// --- trace-mode actions ------------------------------------------------------

#[test]
fn goto_model_finds_the_lean_model_for_the_requirement_at_the_cursor() {
    let mut state = AppState::new();
    state.set_project_root(fixture_root());
    let file = PathBuf::from("src/auth.rs");
    let content = std::fs::read_to_string(fixture_root().join(&file)).unwrap();
    // Put the cursor inside the annotated `login` declaration.
    let at = content.find("password.len()").unwrap();
    let char_pos = content[..at].chars().count();
    state.load_file(file.clone(), content);

    let ctx = tracelean_lib::myth::ActionCtx {
        surface: "editor".into(),
        file: Some(file),
        char_pos: Some(char_pos),
        ..Default::default()
    };
    let outcome = tracelean_lib::myth::actions::registry()
        .dispatch("goto_model", &state, &ctx)
        .unwrap();
    match outcome {
        tracelean_lib::myth::ActionOutcome::Ui { effect } => {
            assert_eq!(effect["kind"], "reveal");
            assert_eq!(effect["file"], json!("model/Auth.lean"));
        }
        other => panic!("expected Ui, got {:?}", other),
    }
}

#[test]
fn a_requirement_with_no_such_link_reports_the_gap_instead_of_failing() {
    // "Nothing implements this" is the answer the user pressed the key to get.
    let mut state = AppState::new();
    state.set_project_root(fixture_root());
    let file = PathBuf::from("src/auth.rs");
    let content = std::fs::read_to_string(fixture_root().join(&file)).unwrap();
    state.load_file(file.clone(), content);
    let ctx = tracelean_lib::myth::ActionCtx {
        file: Some(file),
        char_pos: Some(0),
        args: Some(json!({ "req_id": "REQ-NOBODY" })),
        ..Default::default()
    };
    match tracelean_lib::myth::actions::registry()
        .dispatch("goto_code", &state, &ctx)
        .unwrap()
    {
        tracelean_lib::myth::ActionOutcome::Ui { effect } => {
            assert_eq!(effect["kind"], "no_target");
            assert!(effect["message"].as_str().unwrap().contains("REQ-NOBODY"));
        }
        other => panic!("expected Ui, got {:?}", other),
    }
}

#[test]
fn provenance_jumps_to_the_edit_that_wrote_the_line() {
    let mut state = AppState::new();
    state.set_coalescing(false);
    state.load_file(PathBuf::from("a.rs"), String::new());
    state
        .apply(Command::Replace {
            file: PathBuf::from("a.rs"),
            at: 0,
            old: String::new(),
            new: "fn login() {}".into(),
        })
        .unwrap();
    let ctx = tracelean_lib::myth::ActionCtx {
        file: Some(PathBuf::from("a.rs")),
        char_pos: Some(4),
        ..Default::default()
    };
    match tracelean_lib::myth::actions::registry()
        .dispatch("goto_provenance", &state, &ctx)
        .unwrap()
    {
        tracelean_lib::myth::ActionOutcome::Ui { effect } => {
            assert_eq!(effect["kind"], "goto_node");
            assert_eq!(effect["provenance"]["inserted"], json!("fn login() {}"));
        }
        other => panic!("expected Ui, got {:?}", other),
    }
}

#[test]
fn zoom_level_rejects_a_level_outside_the_range() {
    let state = AppState::new();
    let ctx = tracelean_lib::myth::ActionCtx {
        args: Some(json!({ "level": 9 })),
        ..Default::default()
    };
    assert!(tracelean_lib::myth::actions::registry()
        .dispatch("zoom_level", &state, &ctx)
        .is_err());
}
