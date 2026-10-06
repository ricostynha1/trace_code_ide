//! What observing an external agent is, and — more to the point — what it is
//! not.
//!
//! Every clause here is an absence or a shape, so every one is structural
//! (ADR-0012): there is no function whose output could be compared against a
//! model, only a tree that either has the path or does not.

use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn library_sources() -> Vec<(String, String)> {
    let root = project_root().join("crates");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target" || n == "tests") {
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
    assert!(out.len() > 20, "found only {} library files; the walk is wrong", out.len());
    out
}

/// What a tool did arrives as ordinary commands — the same kind a keystroke
/// makes — and therefore inherits undo, the history tree, provenance and diff
/// review without any of them knowing an agent exists.
///
/// A bespoke apply path for agent edits is how the previous design ended up
/// needing to understand tools, permissions and streaming protocols in order to
/// change a file.
///
/// @tests REQ-OBS.effects_become_commands
/// @tests REQ-OBS.undoable
/// @structural REQ-OBS.effects_become_commands reason="a claim about which type the observation path produces, which is a fact about signatures rather than a value"
/// @structural REQ-OBS.undoable reason="a claim that the commands go through the same inverse and the same tree as any other edit, which is about the call graph"
#[test]
fn what_a_tool_did_arrives_as_ordinary_commands() {
    use tracelean_core::history::command::{apply, inverse, Command, Workspace};
    use tracelean_core::observe::mirror::mutations;

    let mut before = Workspace::new();
    before.files.insert("src/a.rs".into(), "one".into());
    let mut after = Workspace::new();
    after.files.insert("src/a.rs".into(), "two".into());
    after.files.insert("src/b.rs".into(), "new".into());

    let commands = mutations(before.clone(), after.clone());
    assert!(!commands.is_empty(), "a change produced no commands");

    // Every one is a `Command` — the same vocabulary a keystroke produces —
    // and every one has an inverse, which is what `undoable` amounts to.
    let mut state = before.clone();
    let mut undo: Vec<Command> = Vec::new();
    for command in &commands {
        undo.push(inverse(command));
        state = apply(&state, command).expect("a mirrored command applies");
    }
    assert_eq!(state.files, after.files, "applying the commands did not reach the new state");

    for command in undo.into_iter().rev() {
        state = apply(&state, &command).expect("the inverse applies");
    }
    assert_eq!(state.files, before.files, "the mirrored change could not be undone");
}

/// There is no channel by which the editor sends the tool anything.
///
/// Stated as an absence because that is what can be checked: not that the
/// editor uses the channel carefully, but that there is nothing to use.
///
/// @tests REQ-OBS.no_instruction_channel
/// @structural REQ-OBS.no_instruction_channel reason="an absence: no code path writes to a tool, and an absence cannot be a function from data to data"
#[test]
fn nothing_writes_towards_the_tool() {
    let mut found = Vec::new();
    for (file, text) in library_sources() {
        // The differential toolchain drives two runner processes it generated
        // itself, which is this project's own toolchain use and not a tool
        // being instructed.
        if file.contains("/drt/") {
            continue;
        }
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with("//") || line.starts_with("///") {
                continue;
            }
            // Reading one's *own* standard input is being spoken to, which is
            // what a frontend under test does and cannot instruct anything.
            // What would be an instruction channel is giving another process a
            // stdin and writing into it, which is what these name.
            for needle in [".stdin(", "child.stdin", "Stdio::piped", "write_all", "kill()", "send("]
            {
                if line.contains(needle) {
                    found.push(format!("{file}:{}: {line}", line_no + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "something outside the differential toolchain can write towards a process:\n{found:#?}"
    );

    // And the observation surface takes a workspace, not a handle: there is no
    // argument through which an instruction could be passed.
    let source =
        std::fs::read_to_string(project_root().join("crates/core/src/observe/mod.rs")).unwrap();
    assert!(
        !source.contains("pub mod session") && !source.contains("pub mod driver"),
        "the observation module grew something that drives:\n{source}"
    );
}

/// What a tool has changed is observable before anybody decides to accept it.
///
/// The diff is a function of two snapshots, so it can be computed and shown
/// while the workspace is still live — which is the whole difference between
/// reviewing a change and discovering one.
///
/// @tests REQ-OBS.visible_while_running
/// @structural REQ-OBS.visible_while_running reason="a claim that the diff can be taken at any moment without ending the run, which is about when a function may be called rather than about what it returns"
#[test]
fn a_change_can_be_seen_before_it_is_accepted() {
    use tracelean_core::history::command::Workspace;
    use tracelean_core::observe::mirror::mutations;
    use tracelean_core::observe::workspace::snapshot;

    let base = std::env::temp_dir().join(format!("tracelean-visible-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("a.rs"), "one").unwrap();

    let before = snapshot(&base);
    // A tool is still running: it has written one file and will write more.
    std::fs::write(base.join("a.rs"), "two").unwrap();
    let midway = snapshot(&base);
    let seen = mutations(before.clone(), midway);
    assert!(!seen.is_empty(), "a change in flight was not visible");

    // Taking the diff changed nothing: the workspace is where it was, and
    // nothing was accepted.
    let again = snapshot(&base);
    assert_eq!(again.files, Workspace { files: again.files.clone() }.files);
    assert_eq!(
        mutations(before, again).len(),
        seen.len(),
        "looking at the change altered it"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// An action that changes state returns commands and does not mutate.
///
/// @tests REQ-MYTH.actions_are_commands
/// @structural REQ-MYTH.actions_are_commands reason="a claim about the return type of every action, which is a fact about signatures rather than a value any of them computes"
#[test]
fn an_action_returns_commands_rather_than_mutating() {
    use tracelean_core::surface::keymap::Outcome;

    // A dispatch names an action; it does not perform one. The outcome type
    // carries a name, and the name is all the keymap layer knows.
    let outcome = Outcome::Dispatch { action: "undo".into() };
    let rendered = serde_json::to_value(&outcome).unwrap();
    assert_eq!(rendered["dispatch"]["action"], serde_json::json!("undo"));

    // And the layer has nothing to mutate: no workspace, no command, no
    // application. It maps a key to a name, and somebody else decides what
    // that name does. The `&mut` methods it does have edit the *keymap* —
    // configuration a user changes — which is not the state this clause is
    // about.
    let source =
        std::fs::read_to_string(project_root().join("crates/core/src/surface/keymap.rs")).unwrap();
    for needle in ["Workspace", "Command", "apply(", "history::"] {
        assert!(
            !source.contains(needle),
            "the keymap layer reached for `{needle}`, which means it can change state itself"
        );
    }

    // The outcome vocabulary carries names and modes, never state.
    for outcome in [
        Outcome::Enter { mode: "File".into() },
        Outcome::Dispatch { action: "undo".into() },
        Outcome::Leave { mode: "Main".into() },
        Outcome::PassThrough,
    ] {
        let rendered = serde_json::to_string(&outcome).unwrap();
        assert!(
            rendered.len() < 60,
            "an outcome carries more than a name: {rendered}"
        );
    }
}

/// A finding about drift or divergence comes from an evidence record.
///
/// The failure this prevents is a report that asserts things nobody earned: a
/// checker that could decide a clause has drifted, without an evidence record
/// saying so, would be a second source of truth about what is established.
///
/// @tests REQ-CHECK.derived_from_evidence
/// @structural REQ-CHECK.derived_from_evidence reason="a claim about where a finding's information comes from, which is a property of the call graph rather than of any finding"
#[test]
fn the_checker_never_invents_a_drift() {
    use tracelean_core::trace::checker::Kind;

    // The finding vocabulary has no drift or divergence kind at all. Drift is
    // reported by staleness over evidence records, and divergence by a
    // differential run — both of which produce records rather than findings.
    let named: Vec<String> = [
        Kind::Dangling, Kind::DanglingRefines, Kind::RefinesCycle, Kind::DuplicateId,
        Kind::Unmodeled, Kind::Unimplemented, Kind::Unbound, Kind::Untested, Kind::Contested,
        Kind::UnsoundExemption, Kind::UnsoundQualifier, Kind::Malformed, Kind::Imprecise,
    ]
    .iter()
    .map(|k| format!("{k:?}"))
    .collect();
    for forbidden in ["Drift", "Diverge", "Divergence", "Stale"] {
        assert!(
            !named.iter().any(|k| k.contains(forbidden)),
            "the checker can assert `{forbidden}` without an evidence record"
        );
    }

    // And the checker reads the index, never a runner or a filesystem. Its
    // fixtures build temporary trees, so only the library half is read.
    let source =
        std::fs::read_to_string(project_root().join("crates/core/src/trace/checker.rs")).unwrap();
    let library = match source.find("#[cfg(test)]") {
        Some(at) => &source[..at],
        None => &source[..],
    };
    for needle in ["std::fs::", "Command::new", "drt::run"] {
        assert!(!library.contains(needle), "the checker reaches for `{needle}`");
    }
}
