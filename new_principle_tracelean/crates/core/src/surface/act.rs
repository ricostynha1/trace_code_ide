//! From an action name to something the editor does.
//!
//! The keymap turns a key into an action *name*; a span carries action names, so
//! a rendered affordance carries one too. Neither says what an action does. This
//! is the middle, and it is one function — which is why a key and a button
//! cannot mean different things.
//!
//! Resolution takes the focus as well as the action: the buffer, the offset and
//! the text of the span under it. That is what makes `file.open` mean anything.
//!
//! @implements REQ-ACT.action_to_intent

use serde::{Deserialize, Serialize};

use crate::history::command::{Command, Workspace};
use crate::surface::screen::{Arrangement, Axis, Direction};
use crate::surface::view::BufferKind;

/// Where the cursor is and what is under it.
///
/// `under` is the text of the span the cursor is in, which is how an action
/// learns what it is about. A cursor in open space has none, and an action
/// needing one refuses rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub kind: BufferKind,
    pub offset: usize,
    pub under: Option<String>,
}

/// A move through the history tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Move {
    Back,
    Forward,
    Branch,
    /// Straight to one node of the history, by its number.
    To { node: u64 },
}

/// A change to what is being watched, or to what a watch produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Watch {
    Start,
    Accept,
    Reject,
    /// Make a sandbox: a copy of the project for an agent the user starts.
    Create,
    /// Hand the user the command that enters the sandbox.
    Copy,
    /// Discard the sandbox.
    Finish,
    /// Put the focused buffer's whole text on the clipboard.
    CopyAll,
    /// Put the line the cursor is on on the clipboard.
    CopyLine,
    /// Put a file's path on the clipboard.
    CopyPath { path: String },
    /// Take one file's observed change in, leaving the rest waiting.
    AcceptFile { path: String },
    /// Take one file's observed change back out, leaving the rest waiting.
    RejectFile { path: String },
}

/// Why nothing happened.
///
/// A key that appears to do nothing is the failure nobody reports, so the reason
/// is a value the editor can show.
///
/// @implements REQ-ACT.unknown_is_refused
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Blocked {
    UnknownAction { action: String },
    NeedsTarget { action: String, what: String },
}

/// What the editor is to do next.
///
/// @implements REQ-ACT.action_to_intent
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Intent {
    /// Produce this buffer and show it. Which producer that is follows from the
    /// kind, and the shell knows that producing a report means computing one.
    Display { what: BufferKind },
    /// Change the workspace, through the one path that has an inverse.
    Edit { command: Command },
    Travel {
        /// `move` is a keyword here and not in the model's language, where this
        /// field is spelled `move`; the wire name follows the model.
        #[serde(rename = "move")]
        move_: Move,
    },
    Observe { watch: Watch },
    /// Change the arrangement: split, close, move the focus, resize, show.
    ///
    /// Carried as a named change rather than performed here, so that a key and a
    /// pointer reach the one function that performs it (`REQ-SCREEN`).
    Arrange { how: Arrangement },
    /// Write the state where it belongs.
    Persist,
    Refuse { why: Blocked },
}

fn report(title: &str) -> Intent {
    Intent::Display { what: BufferKind::Record { title: title.to_string() } }
}

/// The path of the file the cursor is in, if it is in one.
fn focus_path(focus: &Focus) -> Option<String> {
    match &focus.kind {
        BufferKind::File { path } => Some(path.clone()),
        _ => None,
    }
}

/// The node a history row names, as `#12`: digits only, and few enough to be
/// a number on both sides of the model.
fn node_number(named: &str) -> Option<u64> {
    let digits = named.strip_prefix('#')?;
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A new requirement's document: the frontmatter every requirement has, a
/// first clause to rewrite, and a heading. A draft until someone approves it.
pub fn requirement_template(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: What this requirement is about\nstatus: draft\nclauses:\n  first: The system shall ...\n---\n\n# {id}\n\nWhy this requirement exists.\n"
    )
}

/// Approving a draft requirement: the document the cursor is in, else the one
/// it is on, its first `status: draft` line made `status: approved` — an
/// edit like any other, which Ctrl+Z takes back.
fn approve(focus: &Focus, w: &Workspace) -> Intent {
    const DRAFT: &str = "\nstatus: draft\n";
    let found = focus_path(focus).or_else(|| focus.under.clone()).and_then(|path| {
        let text = w.files.get(&path)?;
        let at = text.find(DRAFT)?;
        Some((path.clone(), text[..at].chars().count() + "\nstatus: ".chars().count()))
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

/// The file a change is about: the one a review shows, else the one under the
/// cursor.
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

/// What an action means, here, now.
///
/// Total: every name the keymap can dispatch has an answer, and every name it
/// cannot has a refusal. Nothing falls through.
///
/// The workspace is here because deleting carries the content it removed — the
/// witness is part of the command, which is what gives it an inverse.
///
/// @implements REQ-ACT.action_to_intent
/// @implements REQ-ACT.focus_is_carried
/// @implements REQ-ACT.one_path
/// @implements REQ-ACT.unknown_is_refused
/// @implements REQ-ACT.missing_target_is_refused
/// @implements REQ-ACT.edits_are_commands
/// @implements REQ-ACT.dispatch_is_pure
/// @drt REQ-ACT.action_to_intent
/// @drt REQ-ACT.focus_is_carried
/// @drt REQ-ACT.missing_target_is_refused
/// @drt REQ-ACT.edits_are_commands
/// @drt REQ-ACT.dispatch_is_pure
pub fn dispatch(action: String, focus: Focus, w: Workspace) -> Intent {
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
            Some(node) => Intent::Travel { move_: Move::To { node } },
        },
        "history.tree" => report("history"),
        "observe.start" => Intent::Observe { watch: Watch::Start },
        "observe.accept" => Intent::Observe { watch: Watch::Accept },
        "observe.reject" => Intent::Observe { watch: Watch::Reject },
        "observe.accept_file" => match changed_file(&focus) {
            None => needs("observe.accept_file", "a changed file"),
            Some(path) => Intent::Observe { watch: Watch::AcceptFile { path } },
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
        "trace.lock" => report("lock"),
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
        "file.definition" => report("definition"),
        "file.references" => report("references"),
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
        // One weight a press. A pointer dragging a divider sends the amount it
        // measured; the key means "a bit more", and a bit is one.
        "screen.grow" => Intent::Arrange { how: Arrangement::Resize { amount: 1 } },
        "screen.shrink" => Intent::Arrange { how: Arrangement::Resize { amount: -1 } },
        "screen.show" => match &focus.under {
            None => needs("screen.show", "a buffer"),
            Some(buffer) => {
                Intent::Arrange { how: Arrangement::ShowBuffer { buffer: buffer.clone() } }
            }
        },
        // The opened set, as a buffer you can put the cursor in. That is how a
        // bare keyboard reaches `screen.show`, which needs a target and takes
        // it from the row under the cursor.
        "screen.strip" => Intent::Display { what: BufferKind::Menu { title: "opened".into() } },
        "screen.offers" => Intent::Display { what: BufferKind::Menu { title: "offers".into() } },
        // A station is an action of its own rather than one action taking the
        // station's name as a target. A station has to be reachable from a bare
        // keyboard, and a target comes from what the cursor is on — so one
        // `screen.station` would be reachable only when a station row was
        // already under the cursor, which is only true once you have got to the
        // stations, which is what the action was for.
        //
        // What each one opens is `station_kind`'s answer and not a case here: a
        // window that knew `requirements` meant the requirement index and a
        // terminal that did not would be two editors, and a list of stations
        // written twice is a list that drifts.
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

/// Whether an intent is one the editor will act on. A refusal is an answer, not
/// an action.
///
/// @implements REQ-ACT.unknown_is_refused
/// @drt REQ-ACT.unknown_is_refused
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

    /// Every action the keymap can dispatch resolves to something the editor
    /// acts on. An action in the registry that resolves to a refusal is a key
    /// that does nothing, which is the failure nobody reports.
    ///
    /// @tests REQ-ACT.action_to_intent
    #[test]
    fn every_action_the_keymap_names_resolves_to_something_done() {
        let mut workspace = Workspace::default();
        workspace.files.insert("a.rs".to_string(), "fn main() {}".to_string());
        workspace.files.insert("reqs/R.md".to_string(), "---\nstatus: draft\n---\n".to_string());
        // One focus per kind of thing a cursor can be on, because actions want
        // different targets: `file.open` wants a path, `screen.station` wants
        // one of the station names, `screen.show` wants a buffer's identity,
        // `history.jump` a numbered point.
        // The claim is that no action in the registry is dead — that *some*
        // cursor makes it do something — not that one cursor suits them all.
        let cursors = [
            in_file(Some("src/lib.rs")),
            in_file(Some("project")),
            in_file(Some("menu:opened")),
            in_file(Some("#3")),
            Focus { kind: BufferKind::File { path: "reqs/R.md".into() }, offset: 0, under: None },
        ];
        for action in keymap::ACTIONS {
            let answers: Vec<Intent> = cursors
                .iter()
                .map(|focus| dispatch(action.to_string(), focus.clone(), workspace.clone()))
                .collect();
            assert!(
                answers.iter().any(|intent| acts(intent.clone())),
                "`{action}` is in the keymap and resolves to nothing: {answers:?}"
            );
        }
    }

    /// The path a frontend opens is the path under the cursor, whatever buffer
    /// the cursor is in.
    ///
    /// @tests REQ-ACT.focus_is_carried
    #[test]
    fn opening_follows_the_cursor_not_the_buffer() {
        let from_file = dispatch("file.open".into(), in_file(Some("src/lib.rs")), Workspace::default());
        let from_listing =
            dispatch("file.open".into(), in_listing(Some("src/lib.rs")), Workspace::default());
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
            dispatch("file.open".into(), in_listing(None), Workspace::default()),
            Intent::Refuse {
                why: Blocked::NeedsTarget { action: "file.open".into(), what: "a path".into() }
            }
        );
        // A file that is not in the workspace cannot be deleted with a witness,
        // and a delete without one has no inverse.
        assert_eq!(
            dispatch("file.delete".into(), in_file(None), Workspace::default()),
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
        let intent = dispatch("file.explode".into(), in_file(None), Workspace::default());
        assert!(!acts(intent.clone()));
        assert_eq!(
            intent,
            Intent::Refuse {
                why: Blocked::UnknownAction { action: "file.explode".into() }
            }
        );
    }

    /// Everything that changes the workspace does so through a command, so undo
    /// is never a special case and provenance is never missing.
    ///
    /// @tests REQ-ACT.edits_are_commands
    #[test]
    fn every_change_to_the_workspace_is_a_command() {
        let mut workspace = Workspace::default();
        workspace.files.insert("a.rs".to_string(), "x".to_string());
        let edits: Vec<Intent> = ["file.new", "file.rename", "file.delete"]
            .iter()
            .map(|action| dispatch(action.to_string(), in_file(Some("b.rs")), workspace.clone()))
            .collect();
        for intent in edits {
            assert!(
                matches!(intent, Intent::Edit { .. }),
                "an edit that is not a command: {intent:?}"
            );
        }
    }
}
