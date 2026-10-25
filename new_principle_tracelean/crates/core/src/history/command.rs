use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// @implements ARCH-CORE-SHELL.decision_total
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(with = "crate::wire::pairs")]
    pub files: BTreeMap<String, String>,
}

impl Workspace {
    pub fn new() -> Workspace {
        Workspace::default()
    }

    pub fn with(files: &[(&str, &str)]) -> Workspace {
        Workspace {
            files: files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    pub fn get(&self, path: &str) -> Option<&String> {
        self.files.get(path)
    }
}

/// @implements REQ-CMD.witness_carried
/// @implements REQ-CMD.inverse_exists
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Command {
    Insert { file: String, offset: usize, text: String },
    Delete { file: String, offset: usize, deleted: String },
    CreateFile { path: String },
    DeleteFile { path: String, content: String },
    RenameFile {
        #[serde(rename = "from_")]
        from: String,
        to: String,
    },
    Batch { commands: Vec<Command> },
}

/// @implements REQ-CMD.total_or_refused
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    NoSuchFile { path: String },
    FileExists { path: String },
    OffsetOutOfRange { path: String, offset: usize, len: usize },
    WitnessMismatch { path: String, offset: usize },
    NotACharBoundary { path: String, offset: usize },
}

/// @implements REQ-CMD.single_path
/// @implements REQ-CMD.total_or_refused
/// @drt REQ-CMD.round_trip
pub fn apply(workspace: &Workspace, command: &Command) -> Result<Workspace, Refusal> {
    let mut next = workspace.clone();
    apply_in_place(&mut next, command)?;
    Ok(next)
}

fn apply_in_place(workspace: &mut Workspace, command: &Command) -> Result<(), Refusal> {
    match command {
        Command::Insert { file, offset, text } => {
            let content = workspace
                .files
                .get_mut(file)
                .ok_or_else(|| Refusal::NoSuchFile { path: file.clone() })?;
            check_boundary(file, content, *offset)?;
            content.insert_str(*offset, text);
            Ok(())
        }
        Command::Delete { file, offset, deleted } => {
            let content = workspace
                .files
                .get_mut(file)
                .ok_or_else(|| Refusal::NoSuchFile { path: file.clone() })?;
            let end = offset + deleted.len();
            if end > content.len() {
                return Err(Refusal::OffsetOutOfRange {
                    path: file.clone(),
                    offset: end,
                    len: content.len(),
                });
            }
            check_boundary(file, content, *offset)?;
            check_boundary(file, content, end)?;
            if &content[*offset..end] != deleted {
                return Err(Refusal::WitnessMismatch { path: file.clone(), offset: *offset });
            }
            content.replace_range(*offset..end, "");
            Ok(())
        }
        Command::CreateFile { path } => {
            if workspace.files.contains_key(path) {
                return Err(Refusal::FileExists { path: path.clone() });
            }
            workspace.files.insert(path.clone(), String::new());
            Ok(())
        }
        Command::DeleteFile { path, content } => {
            match workspace.files.get(path) {
                None => return Err(Refusal::NoSuchFile { path: path.clone() }),
                Some(actual) if actual != content => {
                    return Err(Refusal::WitnessMismatch { path: path.clone(), offset: 0 })
                }
                Some(_) => {}
            }
            workspace.files.remove(path);
            Ok(())
        }
        Command::RenameFile { from, to } => {
            if !workspace.files.contains_key(from) {
                return Err(Refusal::NoSuchFile { path: from.clone() });
            }
            if workspace.files.contains_key(to) {
                return Err(Refusal::FileExists { path: to.clone() });
            }
            let content = workspace.files.remove(from).expect("checked");
            workspace.files.insert(to.clone(), content);
            Ok(())
        }
        Command::Batch { commands } => {
            let mut scratch = workspace.clone();
            for command in commands {
                apply_in_place(&mut scratch, command)?;
            }
            *workspace = scratch;
            Ok(())
        }
    }
}

fn check_boundary(path: &str, content: &str, offset: usize) -> Result<(), Refusal> {
    if offset > content.len() {
        return Err(Refusal::OffsetOutOfRange {
            path: path.to_string(),
            offset,
            len: content.len(),
        });
    }
    if !content.is_char_boundary(offset) {
        return Err(Refusal::NotACharBoundary { path: path.to_string(), offset });
    }
    Ok(())
}

/// @implements REQ-CMD.inverse_exists
/// @implements REQ-CMD.batch_reverses
pub fn inverse(command: &Command) -> Command {
    match command {
        Command::Insert { file, offset, text } => Command::Delete {
            file: file.clone(),
            offset: *offset,
            deleted: text.clone(),
        },
        Command::Delete { file, offset, deleted } => Command::Insert {
            file: file.clone(),
            offset: *offset,
            text: deleted.clone(),
        },
        Command::CreateFile { path } => {
            Command::DeleteFile { path: path.clone(), content: String::new() }
        }
        Command::DeleteFile { path, content } => Command::Batch {
            commands: vec![
                Command::CreateFile { path: path.clone() },
                Command::Insert { file: path.clone(), offset: 0, text: content.clone() },
            ],
        },
        Command::RenameFile { from, to } => {
            Command::RenameFile { from: to.clone(), to: from.clone() }
        }
        Command::Batch { commands } => Command::Batch {
            commands: commands.iter().rev().map(inverse).collect(),
        },
    }
}

impl Refusal {
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::NoSuchFile { .. } => "noSuchFile",
            Refusal::FileExists { .. } => "fileExists",
            Refusal::OffsetOutOfRange { .. } => "offsetOutOfRange",
            Refusal::WitnessMismatch { .. } => "witnessMismatch",
            Refusal::NotACharBoundary { .. } => "notACharBoundary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Ok { w: Workspace },
    Refused { kind: String },
}

impl From<Result<Workspace, Refusal>> for Outcome {
    fn from(result: Result<Workspace, Refusal>) -> Outcome {
        match result {
            Ok(w) => Outcome::Ok { w },
            Err(refusal) => Outcome::Refused { kind: refusal.kind().to_string() },
        }
    }
}

/// @implements REQ-CMD.round_trip
/// @drt REQ-CMD.round_trip
pub fn round_trip_outcome(workspace: Workspace, command: Command) -> Outcome {
    round_trip(workspace, command).into()
}

/// @implements REQ-CMD.single_path
/// @drt REQ-CMD.total_or_refused
/// @drt REQ-CMD.single_path
pub fn apply_outcome(workspace: Workspace, command: Command) -> Outcome {
    apply(&workspace, &command).into()
}

/// @implements REQ-CMD.batch_reverses
/// @drt REQ-CMD.batch_reverses
/// @drt REQ-CMD.inverse_exists
/// @drt REQ-CMD.witness_carried
pub fn inverse_of(command: Command) -> Command {
    inverse(&command)
}

/// @implements REQ-CMD.round_trip
/// @drt REQ-CMD.round_trip
pub fn round_trip(workspace: Workspace, command: Command) -> Result<Workspace, Refusal> {
    let after = apply(&workspace, &command)?;
    apply(&after, &inverse(&command))
}

pub fn describe(command: &Command) -> String {
    fn quoted(text: &str) -> String {
        let shown: String = text.chars().take(40).map(|c| if c == '\n' { '⏎' } else { c }).collect();
        let more = if text.chars().count() > 40 { "…" } else { "" };
        format!("\"{shown}{more}\"")
    }
    match command {
        Command::Insert { file, text, .. } => format!("typed {} in {file}", quoted(text)),
        Command::Delete { file, deleted, .. } => format!("deleted {} in {file}", quoted(deleted)),
        Command::CreateFile { path } => format!("made {path}"),
        Command::DeleteFile { path, .. } => format!("removed {path}"),
        Command::RenameFile { from, to } => format!("renamed {from} to {to}"),
        Command::Batch { commands } => {
            let mut files: Vec<String> = commands
                .iter()
                .map(|c| match c {
                    Command::Insert { file, .. } | Command::Delete { file, .. } => file.clone(),
                    Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.clone(),
                    Command::RenameFile { to, .. } => to.clone(),
                    Command::Batch { .. } => "…".to_string(),
                })
                .collect();
            files.dedup();
            format!("{} changes to {}", commands.len(), files.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_described_in_a_short_row() {
        let typed = Command::Insert { file: "a.rs".into(), offset: 0, text: "fn\nmain".into() };
        assert_eq!(describe(&typed), "typed \"fn⏎main\" in a.rs");
        let long = Command::Delete { file: "a.rs".into(), offset: 0, deleted: "x".repeat(50) };
        assert!(describe(&long).ends_with("…\" in a.rs"));
        let batch = Command::Batch { commands: vec![Command::CreateFile { path: "b".into() }, typed] };
        assert_eq!(describe(&batch), "2 changes to b, a.rs");
    }

    fn ws() -> Workspace {
        Workspace::with(&[("a.rs", "hello world"), ("b.rs", "")])
    }

    fn commands() -> Vec<Command> {
        vec![
            Command::Insert { file: "a.rs".into(), offset: 5, text: ",".into() },
            Command::Insert { file: "a.rs".into(), offset: 0, text: "x".into() },
            Command::Insert { file: "a.rs".into(), offset: 11, text: "!".into() },
            Command::Delete { file: "a.rs".into(), offset: 0, deleted: "hello".into() },
            Command::Delete { file: "a.rs".into(), offset: 6, deleted: "world".into() },
            Command::CreateFile { path: "c.rs".into() },
            Command::DeleteFile { path: "b.rs".into(), content: "".into() },
            Command::DeleteFile { path: "a.rs".into(), content: "hello world".into() },
            Command::RenameFile { from: "a.rs".into(), to: "z.rs".into() },
            Command::Batch {
                commands: vec![
                    Command::CreateFile { path: "c.rs".into() },
                    Command::Insert { file: "c.rs".into(), offset: 0, text: "new".into() },
                ],
            },
        ]
    }

    /// @tests REQ-CMD.round_trip
    #[test]
    fn every_command_round_trips() {
        for command in commands() {
            let before = ws();
            let after = round_trip(before.clone(), command.clone())
                .unwrap_or_else(|e| panic!("{command:?} refused on the way back: {e:?}"));
            assert_eq!(before, after, "{command:?} did not round-trip");
        }
    }

    /// @tests REQ-CMD.batch_reverses
    #[test]
    fn a_batch_inverts_in_reverse_order() {
        let batch = Command::Batch {
            commands: vec![
                Command::CreateFile { path: "c.rs".into() },
                Command::Insert { file: "c.rs".into(), offset: 0, text: "hi".into() },
            ],
        };
        match inverse(&batch) {
            Command::Batch { commands } => {
                assert!(matches!(commands[0], Command::Delete { .. }));
                assert!(matches!(commands[1], Command::DeleteFile { .. }));
            }
            other => panic!("{other:?}"),
        }
    }

    /// @tests REQ-CMD.total_or_refused
    #[test]
    fn a_refused_batch_leaves_nothing_behind() {
        let before = ws();
        let batch = Command::Batch {
            commands: vec![
                Command::Insert { file: "a.rs".into(), offset: 0, text: "x".into() },
                Command::Insert { file: "missing.rs".into(), offset: 0, text: "y".into() },
            ],
        };
        assert!(apply(&before, &batch).is_err());
        assert_eq!(apply(&before, &batch).err().is_some(), true);
        assert_eq!(before.get("a.rs").unwrap(), "hello world");
    }

    /// @tests REQ-CMD.witness_carried
    #[test]
    fn a_delete_whose_witness_does_not_match_is_refused() {
        let result = apply(
            &ws(),
            &Command::Delete { file: "a.rs".into(), offset: 0, deleted: "HELLO".into() },
        );
        assert_eq!(
            result.unwrap_err(),
            Refusal::WitnessMismatch { path: "a.rs".into(), offset: 0 }
        );
    }

    #[test]
    fn an_offset_inside_a_character_is_refused() {
        let workspace = Workspace::with(&[("a.rs", "é")]);
        let result = apply(
            &workspace,
            &Command::Insert { file: "a.rs".into(), offset: 1, text: "x".into() },
        );
        assert_eq!(
            result.unwrap_err(),
            Refusal::NotACharBoundary { path: "a.rs".into(), offset: 1 }
        );
    }

    #[test]
    fn renaming_onto_an_existing_file_is_refused() {
        let result =
            apply(&ws(), &Command::RenameFile { from: "a.rs".into(), to: "b.rs".into() });
        assert_eq!(result.unwrap_err(), Refusal::FileExists { path: "b.rs".into() });
    }

    #[test]
    fn encodes_the_way_the_model_does() {
        let w = Workspace::with(&[("a.rs", "hi")]);
        assert_eq!(serde_json::to_string(&w).unwrap(), r#"{"files":[["a.rs","hi"]]}"#);
        assert_eq!(
            serde_json::to_string(&insert_cmd()).unwrap(),
            r#"{"insert":{"file":"a.rs","offset":2,"text":"x"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Command::CreateFile { path: "b.rs".into() }).unwrap(),
            r#"{"createFile":{"path":"b.rs"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Outcome::Refused { kind: "noSuchFile".into() }).unwrap(),
            r#"{"refused":{"kind":"noSuchFile"}}"#
        );
    }

    fn insert_cmd() -> Command {
        Command::Insert { file: "a.rs".into(), offset: 2, text: "x".into() }
    }

    /// @tests REQ-CMD.single_path
    #[test]
    fn applying_never_mutates_the_input() {
        let before = ws();
        let _ = apply(&before, &Command::Insert {
            file: "a.rs".into(),
            offset: 0,
            text: "x".into(),
        });
        assert_eq!(before.get("a.rs").unwrap(), "hello world");
    }
}
