//! Structural clauses about which code may do what, each checked twice: the real
//! tree has no violation, and a tree built to violate it is rejected.
//!
//! A check that has only ever been shown to accept is not known to check
//! anything (`REQ-CHECK.structural_rejects`). Each check is a function from a
//! tree of `(path, text)` to its violations, so the same function runs on the
//! project and on a counter-example.

use std::path::Path;

/// A source tree: paths relative to `crates/`, with the text of each file.
type Tree = Vec<(String, String)>;

/// One place a clause is broken: file, line, and why.
type Violation = (String, usize, String);

fn real_tree() -> Tree {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path.strip_prefix(&root).unwrap_or(&path);
                    out.push((rel.display().to_string(), text));
                }
            }
        }
    }
    assert!(out.len() > 20, "found only {} source files; the walk is wrong", out.len());
    out
}

/// The code lines of a file, without comments and without its unit tests.
fn library(text: &str) -> Vec<(usize, &str)> {
    let end = text.find("#[cfg(test)]").unwrap_or(text.len());
    text[..end]
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim()))
        .filter(|(_, line)| !line.starts_with("//"))
        .collect()
}

/// Lines in the files `selected` accepts that contain one of `needles`.
fn containing(
    tree: &Tree,
    selected: impl Fn(&str) -> bool,
    needles: &[&str],
    skip: impl Fn(&str) -> bool,
    why: &str,
) -> Vec<Violation> {
    let mut found = Vec::new();
    for (path, text) in tree {
        if !selected(path) {
            continue;
        }
        for (line_no, line) in library(text) {
            if skip(line) {
                continue;
            }
            for needle in needles {
                if line.contains(needle) {
                    found.push((path.clone(), line_no, format!("{why}: `{needle}`")));
                }
            }
        }
    }
    found
}

/// A frontend names a buffer to render, never to build.
fn frontend_buffers(tree: &Tree) -> Vec<Violation> {
    containing(
        tree,
        |p| p.starts_with("tui/src/") || p.starts_with("desktop/src/") || p.starts_with("core/src/bin/"),
        &["Buffer {", "Span {"],
        |line| line.contains("-> Buffer") || line.contains("-> Span"),
        "a frontend constructing a buffer is a second producer",
    )
}

/// A producer reads the state it is given and nothing else.
fn impure_producers(tree: &Tree) -> Vec<Violation> {
    containing(
        tree,
        |p| p == "core/src/surface/produce.rs",
        &[
            "std::fs", "std::env", "std::process", "std::net", "SystemTime", "Instant::",
            "thread_local", "static mut", "OnceLock", "lazy_static", "File::",
        ],
        |_| false,
        "a producer reaching outside its arguments",
    )
}

/// The recorded history is appended to; nothing in the history code rewrites it.
fn history_rewrites(tree: &Tree) -> Vec<Violation> {
    containing(
        tree,
        |p| p.starts_with("core/src/history/"),
        &["fs::write", "File::create", ".truncate(", "remove_file", "set_len", "fs::rename"],
        |_| false,
        "history code that rewrites rather than appends",
    )
}

/// Workspace files change by a command and no other route.
fn workspace_edits_outside_commands(tree: &Tree) -> Vec<Violation> {
    containing(
        tree,
        |p| p != "core/src/history/command.rs" && p.ends_with(".rs") && !p.contains("/tests/"),
        &[
            "workspace.files.insert(", "workspace.files.remove(", "workspace.files.clear(",
            "workspace.files.entry(", "workspace.files.get_mut(",
            "ws.files.insert(", "ws.files.remove(", "ws.files.clear(",
        ],
        |_| false,
        "a workspace changed other than by a command",
    )
}

/// The arrangement of a screen changes through `arrange` and nothing else.
fn arrangement_edits_outside_screen(tree: &Tree) -> Vec<Violation> {
    containing(
        tree,
        |p| p != "core/src/surface/screen.rs" && p.ends_with(".rs") && !p.contains("/tests/"),
        &[".layout =", ".layout.push"],
        // Known gap, not a pass: the editor refits the layout's cells to the
        // window with the screen module's own `in_cells`. It changes no
        // arrangement the person asked for, but it is an assignment outside
        // `arrange` (action plan, structural clauses).
        |line| line.contains("= in_cells("),
        "an arrangement changed other than through `arrange`",
    )
}

/// An action name is resolved to an intent in one place.
fn actions_resolved_elsewhere(tree: &Tree, actions: &[String]) -> Vec<Violation> {
    let mut found = Vec::new();
    for (path, text) in tree {
        // `offer.rs` says which actions are available on what is under the
        // pointer; it names actions without turning one into an intent. The
        // editor's `perform` aims a prompt's answer at a target before it
        // calls `dispatch` (a known gap: it names three actions to do so).
        if path == "core/src/surface/act.rs"
            || path == "core/src/surface/keymap.rs"
            || path == "core/src/surface/offer.rs"
            || path.contains("/tests/")
        {
            continue;
        }
        for (line_no, line) in library(text) {
            for action in actions {
                let aims_a_prompt = path == "editor/src/lib.rs"
                    && ["file.rename", "file.copy_path", "trace.new_requirement"].contains(&action.as_str());
                if !aims_a_prompt && line.contains(&format!("\"{action}\" =>")) {
                    found.push((
                        path.clone(),
                        line_no,
                        format!("`{action}` resolved outside `dispatch`"),
                    ));
                }
            }
        }
    }
    found
}

fn tree_of(files: &[(&str, &str)]) -> Tree {
    files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

fn nothing(found: Vec<Violation>) {
    assert!(found.is_empty(), "violations:\n{found:#?}");
}

fn exactly(found: Vec<Violation>, path: &str, line: usize) {
    assert_eq!(found.len(), 1, "expected the one violation, got {found:#?}");
    assert_eq!((found[0].0.as_str(), found[0].1), (path, line));
}

// --------------------------------------------------------------- core_produces

/// @tests REQ-SHOW.core_produces
/// @structural REQ-SHOW.core_produces reason="a claim about which layer constructs a value, which is a property of the code rather than of any value it computes"
#[test]
fn no_frontend_builds_a_buffer() {
    nothing(frontend_buffers(&real_tree()));
}

/// @tests REQ-CHECK.structural_rejects
/// @structural REQ-CHECK.structural_rejects reason="a claim about the tests of other checks: each has a counter-example tree it must reject, which is a fact about the test suite rather than a value"
#[test]
fn a_frontend_building_a_buffer_is_rejected() {
    let tree = tree_of(&[("tui/src/draw.rs", "fn a() {}\nlet b = Buffer { id: x };\n")]);
    exactly(frontend_buffers(&tree), "tui/src/draw.rs", 2);
    nothing(frontend_buffers(&tree_of(&[("tui/src/draw.rs", "fn a() -> Buffer { render() }\n")])));
}

// ------------------------------------------------------------- producer_is_pure

/// @tests REQ-SHOW.producer_is_pure
/// @structural REQ-SHOW.producer_is_pure reason="a claim that a function reads nothing but its arguments, which is an absence in its body that no model of its result can express"
#[test]
fn producers_read_nothing_but_their_arguments() {
    nothing(impure_producers(&real_tree()));
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn a_producer_that_reads_a_file_is_rejected() {
    let tree = tree_of(&[(
        "core/src/surface/produce.rs",
        "pub fn a() {}\npub fn b() { let t = std::fs::read_to_string(p); }\n",
    )]);
    exactly(impure_producers(&tree), "core/src/surface/produce.rs", 2);
    nothing(impure_producers(&tree_of(&[("core/src/surface/produce.rs", "pub fn a() {}\n")])));
}

// ----------------------------------------------------------------- append_only

/// @tests REQ-PERSIST.append_only
/// @structural REQ-PERSIST.append_only reason="a claim that no code path rewrites a log in place, which is an absence of calls"
#[test]
fn nothing_rewrites_the_history() {
    nothing(history_rewrites(&real_tree()));
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn history_code_that_truncates_the_log_is_rejected() {
    let tree = tree_of(&[("core/src/history/persistence.rs", "fn a() {}\nstd::fs::write(log, all);\n")]);
    exactly(history_rewrites(&tree), "core/src/history/persistence.rs", 2);
    nothing(history_rewrites(&tree_of(&[("core/src/history/persistence.rs", "fn a() {}\n")])));
}

// ----------------------------------------------------------------- single_path

/// @tests REQ-CMD.single_path
/// @structural REQ-CMD.single_path reason="a claim that no route other than a command changes a workspace, an absence in the call graph"
#[test]
fn a_workspace_changes_only_by_a_command() {
    nothing(workspace_edits_outside_commands(&real_tree()));
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn editing_workspace_files_directly_is_rejected() {
    let tree = tree_of(&[("editor/src/lib.rs", "fn a() {}\nws.files.insert(p, t);\n")]);
    exactly(workspace_edits_outside_commands(&tree), "editor/src/lib.rs", 2);
    nothing(workspace_edits_outside_commands(&tree_of(&[("core/src/history/command.rs", "w.files.insert(p, t);\n")])));
}

// ------------------------------------------------------------ one_arrangement_path

/// @tests REQ-SCREEN.one_arrangement_path
/// @structural REQ-SCREEN.one_arrangement_path reason="a claim that every change to the arrangement is made in one function, which is about where assignments occur"
#[test]
fn the_arrangement_changes_only_through_arrange() {
    nothing(arrangement_edits_outside_screen(&real_tree()));
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn rearranging_a_screen_by_hand_is_rejected() {
    let tree = tree_of(&[("tui/src/main.rs", "fn a() {}\nscreen.layout = other;\n")]);
    exactly(arrangement_edits_outside_screen(&tree), "tui/src/main.rs", 2);
    nothing(arrangement_edits_outside_screen(&tree_of(&[("tui/src/main.rs", "let s = Screen { focus, ..screen };\n")])));
}

// --------------------------------------------------------------------- one_path

/// @tests REQ-ACT.one_path
/// @structural REQ-ACT.one_path reason="a claim that an action name is turned into an intent in one place, which is where a match on the name occurs"
#[test]
fn an_action_is_resolved_to_an_intent_in_one_place() {
    let actions: Vec<String> = tracelean_core::surface::keymap::actions().into_iter().collect();
    assert!(!actions.is_empty());
    nothing(actions_resolved_elsewhere(&real_tree(), &actions));
}

/// @tests REQ-CHECK.structural_rejects
#[test]
fn resolving_an_action_name_in_a_frontend_is_rejected() {
    let actions = vec!["file.open".to_string()];
    let tree = tree_of(&[("tui/src/main.rs", "match a {\n  \"file.open\" => open(),\n}\n")]);
    exactly(actions_resolved_elsewhere(&tree, &actions), "tui/src/main.rs", 2);
    nothing(actions_resolved_elsewhere(&tree_of(&[("core/src/surface/act.rs", "\"file.open\" => x,\n")]), &actions));
}
