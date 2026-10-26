//! @implements REQ-ACT.action_to_intent

use serde::{Deserialize, Serialize};

use crate::history::command::{Command, Workspace};
use crate::surface::screen::{Arrangement, Axis, Direction};
use crate::surface::view::BufferKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub kind: BufferKind,
    pub offset: usize,
    pub under: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Move {
    Back,
    Forward,
    Branch,
    To { node: u64 },
    Base,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Watch {
    Start,
    Accept { command: Command },
    Reject,
    Create,
    Copy,
    Finish,
    CopyAll,
    CopyLine,
    CopyPath { path: String },
    AcceptFile { path: String, command: Command },
    RejectFile { path: String },
    ContextToggle { part: crate::surface::context::Part },
    ContextCopy,
    HistoryFilter { filter: crate::surface::history_view::Filter },
    DesignToggle { id: String },
    DesignExpand { id: Option<String> },
    DesignFold,
    DesignOpenOnly,
}

/// @implements REQ-ACT.unknown_is_refused
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Blocked {
    UnknownAction { action: String },
    NeedsTarget { action: String, what: String },
}

/// @implements REQ-ACT.action_to_intent
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Intent {
    Display { what: BufferKind },
    Edit { command: Command },
    Travel {
        #[serde(rename = "move")]
        move_: Move,
    },
    Observe { watch: Watch },
    Arrange { how: Arrangement },
    Persist,
    Refuse { why: Blocked },
}

fn report(title: &str) -> Intent {
    Intent::Display { what: BufferKind::Record { title: title.to_string() } }
}

fn focus_path(focus: &Focus) -> Option<String> {
    match &focus.kind {
        BufferKind::File { path } => Some(path.clone()),
        _ => None,
    }
}

fn node_number(named: &str) -> Option<u64> {
    let digits = named.strip_prefix('#')?;
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub fn requirement_template(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: What this requirement is about\nstatus: draft\nclauses:\n  first: The system shall ...\n---\n\n# {id}\n\nWhy this requirement exists.\n"
    )
}

fn approve(focus: &Focus, w: &Workspace) -> Intent {
    const DRAFT: &str = "\nstatus: draft\n";
    let found = focus_path(focus).or_else(|| focus.under.clone()).and_then(|path| {
        let text = w.files.get(&path)?;
        let at = text.find(DRAFT)?;
        Some((path.clone(), text[..at].len() + "\nstatus: ".len()))
    });
    match found {
        None => needs("trace.approve", "a draft requirement"),
        Some((file, offset)) => Intent::Edit {
            command: Command::Batch {
                commands: vec![
                    Command::Delete { file: file.clone(), offset, deleted: "draft".into() },
                    Command::Insert { file, offset, text: "approved".into() },
                ],
            },
        },
    }
}

fn changed_file(focus: &Focus) -> Option<String> {
    match &focus.kind {
        BufferKind::Review { target } => Some(target.clone()),
        _ => focus.under.clone(),
    }
}

fn needs(action: &str, what: &str) -> Intent {
    Intent::Refuse {
        why: Blocked::NeedsTarget { action: action.to_string(), what: what.to_string() },
    }
}

pub fn touched(command: &Command) -> String {
    match command {
        Command::Insert { file, .. } | Command::Delete { file, .. } => file.clone(),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.clone(),
        Command::RenameFile { from, to } => format!("{from} → {to}"),
        Command::Batch { commands } => commands.iter().map(touched).collect::<Vec<_>>().join(", "),
    }
}

fn name_at(focus: &Focus, w: &Workspace) -> Option<String> {
    match focus_path(focus).and_then(|path| w.files.get(&path).cloned()) {
        Some(text) => crate::surface::definition::identifier_at(&text, focus.offset),
        None => focus.under.clone().filter(|name| !name.is_empty()),
    }
}

/// @implements REQ-ACT.action_to_intent
/// @implements REQ-ACT.focus_is_carried
/// @implements REQ-ACT.one_path
/// @implements REQ-ACT.unknown_is_refused
/// @implements REQ-ACT.missing_target_is_refused
/// @implements REQ-ACT.edits_are_commands
/// @implements REQ-ACT.dispatch_is_pure
/// @drt REQ-ACT.action_to_intent
/// @drt REQ-ACT.one_path
/// @drt REQ-ACT.focus_is_carried
/// @drt REQ-ACT.missing_target_is_refused
/// @drt REQ-ACT.edits_are_commands
/// @drt REQ-ACT.dispatch_is_pure
/// @drt REQ-ACT.unknown_is_refused
pub fn dispatch(action: String, focus: Focus, w: Workspace, waiting: Vec<Command>) -> Intent {
    let focus = match crate::surface::view::action_target(&action) {
        Some(target) => Focus { under: Some(target.to_string()), ..focus },
        None => focus,
    };
    let action = crate::surface::view::action_name(&action).to_string();
    match action.as_str() {
        "file.open" => match &focus.under {
            None => needs("file.open", "a path"),
            Some(path) => Intent::Display { what: BufferKind::File { path: path.clone() } },
        },
        "file.new" => match &focus.under {
            None => needs("file.new", "a name"),
            Some(path) => Intent::Edit { command: Command::CreateFile { path: path.clone() } },
        },
        "file.rename" => match focus_path(&focus) {
            None => needs("file.rename", "a file"),
            Some(from) => match &focus.under {
                None => needs("file.rename", "a new name"),
                Some(to) => Intent::Edit { command: Command::RenameFile { from, to: to.clone() } },
            },
        },
        "file.delete" => match focus_path(&focus) {
            None => needs("file.delete", "a file"),
            Some(path) => match w.files.get(&path) {
                None => needs("file.delete", "a file that exists"),
                Some(content) => Intent::Edit {
                    command: Command::DeleteFile { path: path.clone(), content: content.clone() },
                },
            },
        },
        "file.save" => Intent::Persist,
        "history.undo" => Intent::Travel { move_: Move::Back },
        "history.redo" => Intent::Travel { move_: Move::Forward },
        "history.branch" => Intent::Travel { move_: Move::Branch },
        "history.jump" => match focus.under.as_deref().and_then(node_number) {
            None => needs("history.jump", "a point in the history"),
            Some(0) => Intent::Travel { move_: Move::Base },
            Some(k) => Intent::Travel { move_: Move::To { node: k - 1 } },
        },
        "history.tree" => report("history"),
        "observe.start" => Intent::Observe { watch: Watch::Start },
        "observe.accept" => {
            if waiting.is_empty() {
                needs("observe.accept", "a change waiting")
            } else {
                Intent::Observe {
                    watch: Watch::Accept { command: Command::Batch { commands: waiting } },
                }
            }
        }
        "observe.reject" => Intent::Observe { watch: Watch::Reject },
        "observe.accept_file" => match changed_file(&focus) {
            None => needs("observe.accept_file", "a changed file"),
            Some(path) => {
                let taken: Vec<Command> =
                    waiting.into_iter().filter(|command| touched(command) == path).collect();
                if taken.is_empty() {
                    needs("observe.accept_file", "a change waiting to that file")
                } else {
                    Intent::Observe {
                        watch: Watch::AcceptFile {
                            path,
                            command: Command::Batch { commands: taken },
                        },
                    }
                }
            }
        },
        "observe.reject_file" => match changed_file(&focus) {
            None => needs("observe.reject_file", "a changed file"),
            Some(path) => Intent::Observe { watch: Watch::RejectFile { path } },
        },
        "sandbox.new" => Intent::Observe { watch: Watch::Create },
        "sandbox.copy" => Intent::Observe { watch: Watch::Copy },
        "sandbox.end" => Intent::Observe { watch: Watch::Finish },
        "observe.diff" => match &focus.under {
            None => needs("observe.diff", "a change"),
            Some(target) => {
                Intent::Display { what: BufferKind::Review { target: target.clone() } }
            }
        },
        "trace.check" => report("check"),
        "trace.evidence" => report("evidence"),
        "trace.findings" => report("findings"),
        "definitions.edit" => match &focus.under {
            None => needs("definitions.edit", "a setting"),
            Some(setting) => report(&format!("edit {setting}")),
        },
        "file.find" => report("find"),
        "file.find_lines" => report("find lines"),
        "trace.lock" => report("lock"),
        "trace.context" => match &focus.under {
            None => needs("trace.context", "a requirement"),
            Some(id) => report(&format!("context {id}")),
        },
        "trace.coverage" => match &focus.under {
            None => needs("trace.coverage", "a requirement"),
            Some(id) => report(&format!("coverage {id}")),
        },
        "context.toggle" => match focus.under.clone().and_then(crate::surface::context::part_named) {
            None => needs("context.toggle", "a part of the context"),
            Some(part) => Intent::Observe { watch: Watch::ContextToggle { part } },
        },
        "context.copy" => Intent::Observe { watch: Watch::ContextCopy },
        "history.filter" => match focus.under.as_deref().and_then(crate::surface::history_view::filter_named) {
            None => needs("history.filter", "a view of the history"),
            Some(filter) => Intent::Observe { watch: Watch::HistoryFilter { filter } },
        },
        "design.toggle" => match &focus.under {
            None => needs("design.toggle", "a requirement"),
            Some(id) => Intent::Observe { watch: Watch::DesignToggle { id: id.clone() } },
        },
        "design.expand_all" => match &focus.under {
            None => needs("design.expand_all", "a requirement"),
            Some(id) => Intent::Observe { watch: Watch::DesignExpand { id: Some(id.clone()) } },
        },
        "design.expand_everything" => Intent::Observe { watch: Watch::DesignExpand { id: None } },
        "design.fold_all" => Intent::Observe { watch: Watch::DesignFold },
        "design.open_only" => Intent::Observe { watch: Watch::DesignOpenOnly },
        "trace.judge" => match &focus.under {
            None => needs("trace.judge", "a clause"),
            Some(clause) => report(&format!("judge {clause}")),
        },
        "file.copy" => Intent::Observe { watch: Watch::CopyAll },
        "file.copy_line" => Intent::Observe { watch: Watch::CopyLine },
        "file.copy_path" => match focus_path(&focus) {
            None => needs("file.copy_path", "a file"),
            Some(path) => Intent::Observe { watch: Watch::CopyPath { path } },
        },
        "file.definition" => match name_at(&focus, &w) {
            None => needs("file.definition", "a name"),
            Some(name) => report(&format!("definition {name}")),
        },
        "file.references" => match name_at(&focus, &w) {
            None => needs("file.references", "a name"),
            Some(name) => report(&format!("references {name}")),
        },
        "trace.new_requirement" => match &focus.under {
            None => needs("trace.new_requirement", "an identifier"),
            Some(id) => {
                let path = format!("reqs/{id}.md");
                Intent::Edit {
                    command: Command::Batch {
                        commands: vec![
                            Command::CreateFile { path: path.clone() },
                            Command::Insert { file: path, offset: 0, text: requirement_template(id) },
                        ],
                    },
                }
            }
        },
        "trace.approve" => approve(&focus, &w),
        "trace.requirement" => match &focus.under {
            None => needs("trace.requirement", "a requirement"),
            Some(id) => report(&format!("requirement {id}")),
        },
        "trace.rollup" => report("rollup"),
        "trace.stale" => report("stale"),
        "drt.bindings" => report("drt bindings"),
        "drt.coverage" => report("drt coverage"),
        "drt.judge" => report("drt judge"),
        "drt.run" => report("drt run"),
        "drt.shrink" => report("drt shrink"),
        "screen.split.across" => Intent::Arrange { how: Arrangement::Split { axis: Axis::Across } },
        "screen.split.down" => Intent::Arrange { how: Arrangement::Split { axis: Axis::Down } },
        "screen.close" => Intent::Arrange { how: Arrangement::Close },
        "screen.focus.left" => Intent::Arrange { how: Arrangement::Focus { dir: Direction::Left } },
        "screen.focus.right" => {
            Intent::Arrange { how: Arrangement::Focus { dir: Direction::Right } }
        }
        "screen.focus.up" => Intent::Arrange { how: Arrangement::Focus { dir: Direction::Up } },
        "screen.focus.down" => Intent::Arrange { how: Arrangement::Focus { dir: Direction::Down } },
        "screen.grow" => Intent::Arrange { how: Arrangement::Resize { amount: 1 } },
        "screen.shrink" => Intent::Arrange { how: Arrangement::Resize { amount: -1 } },
        "screen.show" => match &focus.under {
            None => needs("screen.show", "a buffer"),
            Some(buffer) => {
                Intent::Arrange { how: Arrangement::ShowBuffer { buffer: buffer.clone() } }
            }
        },
        "screen.strip" => Intent::Display { what: BufferKind::Menu { title: "opened".into() } },
        "screen.offers" => Intent::Display { what: BufferKind::Menu { title: "offers".into() } },
        other => match other.strip_prefix("screen.station.") {
            Some(station) => match crate::surface::screen::station_kind(station.to_string()) {
                Some(what) => Intent::Display { what },
                None => Intent::Refuse {
                    why: Blocked::UnknownAction { action: other.to_string() },
                },
            },
            None => Intent::Refuse {
                why: Blocked::UnknownAction { action: other.to_string() },
            },
        },
    }
}

/// @implements REQ-ACT.unknown_is_refused
pub fn acts(intent: Intent) -> bool {
    !matches!(intent, Intent::Refuse { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::keymap;

    fn in_file(under: Option<&str>) -> Focus {
        Focus {
            kind: BufferKind::File { path: "a.rs".into() },
            offset: 0,
            under: under.map(str::to_string),
        }
    }

    fn in_listing(under: Option<&str>) -> Focus {
        Focus {
            kind: BufferKind::Directory { path: "src".into() },
            offset: 0,
            under: under.map(str::to_string),
        }
    }

    /// @tests REQ-ACT.action_to_intent
    #[test]
    fn every_action_the_keymap_names_resolves_to_something_done() {
        let mut workspace = Workspace::default();
        workspace.files.insert("a.rs".to_string(), "fn main() {}".to_string());
        workspace.files.insert("reqs/R.md".to_string(), "---\nstatus: draft\n---\n".to_string());
        let cursors = [
            in_file(Some("src/lib.rs")),
            in_file(Some("project")),
            in_file(Some("menu:opened")),
            in_file(Some("#3")),
            Focus { kind: BufferKind::File { path: "reqs/R.md".into() }, offset: 0, under: None },
            Focus { kind: BufferKind::Record { title: "context REQ-R".into() }, offset: 0, under: Some("code".into()) },
            Focus { kind: BufferKind::Record { title: "history".into() }, offset: 0, under: Some("Saved".into()) },
        ];
        let waiting = vec![Command::Insert { file: "src/lib.rs".into(), offset: 0, text: "x".into() }];
        for action in keymap::ACTIONS {
            let answers: Vec<Intent> = cursors
                .iter()
                .map(|focus| dispatch(action.to_string(), focus.clone(), workspace.clone(), waiting.clone()))
                .collect();
            assert!(
                answers.iter().any(|intent| acts(intent.clone())),
                "`{action}` is in the keymap and resolves to nothing: {answers:?}"
            );
        }
    }

    /// @tests REQ-ACT.focus_is_carried
    #[test]
    fn opening_follows_the_cursor_not_the_buffer() {
        let from_file = dispatch("file.open".into(), in_file(Some("src/lib.rs")), Workspace::default(), Vec::new());
        let from_listing =
            dispatch("file.open".into(), in_listing(Some("src/lib.rs")), Workspace::default(), Vec::new());
        assert_eq!(from_file, from_listing);
        assert_eq!(
            from_file,
            Intent::Display { what: BufferKind::File { path: "src/lib.rs".into() } }
        );
    }

    /// @tests REQ-ACT.missing_target_is_refused
    #[test]
    fn an_action_without_its_target_says_what_is_missing() {
        assert_eq!(
            dispatch("file.open".into(), in_listing(None), Workspace::default(), Vec::new()),
            Intent::Refuse {
                why: Blocked::NeedsTarget { action: "file.open".into(), what: "a path".into() }
            }
        );
        assert_eq!(
            dispatch("file.delete".into(), in_file(None), Workspace::default(), Vec::new()),
            Intent::Refuse {
                why: Blocked::NeedsTarget {
                    action: "file.delete".into(),
                    what: "a file that exists".into()
                }
            }
        );
    }

    /// @tests REQ-ACT.unknown_is_refused
    #[test]
    fn a_name_no_keymap_dispatches_is_refused_by_name() {
        let intent = dispatch("file.explode".into(), in_file(None), Workspace::default(), Vec::new());
        assert!(!acts(intent.clone()));
        assert_eq!(
            intent,
            Intent::Refuse {
                why: Blocked::UnknownAction { action: "file.explode".into() }
            }
        );
    }

    /// @tests REQ-ACT.edits_are_commands
    #[test]
    fn every_change_to_the_workspace_is_a_command() {
        let mut workspace = Workspace::default();
        workspace.files.insert("a.rs".to_string(), "x".to_string());
        let edits: Vec<Intent> = ["file.new", "file.rename", "file.delete"]
            .iter()
            .map(|action| dispatch(action.to_string(), in_file(Some("b.rs")), workspace.clone(), Vec::new()))
            .collect();
        for intent in edits {
            assert!(
                matches!(intent, Intent::Edit { .. }),
                "an edit that is not a command: {intent:?}"
            );
        }
    }
}
