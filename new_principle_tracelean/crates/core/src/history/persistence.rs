use serde::{Deserialize, Serialize};

use super::command::{apply, Command, Outcome, Refusal, Workspace};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub workspace: Workspace,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    pub commands: Vec<Command>,
    /// @implements REQ-PERSIST.truncated_is_reported
    pub truncated_after: Option<usize>,
}

/// @implements REQ-PERSIST.append_only
pub fn parse_log(text: &str) -> Log {
    let mut commands = Vec::new();
    let mut truncated_after = None;

    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Command>(line) {
            Ok(command) => commands.push(command),
            Err(_) => {
                truncated_after = Some(index);
                break;
            }
        }
    }

    Log { commands, truncated_after }
}

pub fn entry(command: &Command) -> String {
    let mut line = serde_json::to_string(command).unwrap_or_default();
    line.push('\n');
    line
}

/// @implements REQ-PERSIST.replay_exact
pub fn replay(base: Workspace, commands: Vec<Command>) -> Result<Workspace, Refusal> {
    let mut workspace = base;
    for command in &commands {
        workspace = apply(&workspace, command)?;
    }
    Ok(workspace)
}

/// @implements REQ-PERSIST.checkpoint_equivalent
pub fn replay_from(
    checkpoint: &Checkpoint,
    commands: &[Command],
) -> Result<Workspace, Refusal> {
    let rest = commands.iter().skip(checkpoint.entries).cloned().collect();
    replay(checkpoint.workspace.clone(), rest)
}

pub fn checkpoint(base: Workspace, commands: &[Command], entries: usize) -> Result<Checkpoint, Refusal> {
    let taken = commands.iter().take(entries).cloned().collect();
    Ok(Checkpoint { workspace: replay(base, taken)?, entries })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pub commands: Vec<Command>,
    pub truncated_after: Option<usize>,
    pub direct: Outcome,
    pub via_checkpoint: Outcome,
}

/// @implements REQ-PERSIST.replay_exact
/// @implements ARCH-DETERMINISM.replay_exact
/// @implements REQ-PERSIST.checkpoint_equivalent
/// @implements REQ-PERSIST.truncated_is_reported
/// @drt REQ-PERSIST.replay_exact
/// @drt ARCH-DETERMINISM.replay_exact
/// @drt REQ-PERSIST.checkpoint_equivalent
/// @drt REQ-PERSIST.truncated_is_reported
/// @drt REQ-PERSIST.append_only
pub fn replay_report(base: Workspace, lines: Vec<String>, checkpoint_at: usize) -> ReplayReport {
    let log = parse_log(&lines.join("\n"));

    let direct = Outcome::from(replay(base.clone(), log.commands.clone()));
    let taken = checkpoint_at.min(log.commands.len());
    let via_checkpoint = match checkpoint(base, &log.commands, taken) {
        Ok(point) => Outcome::from(replay_from(&point, &log.commands)),
        Err(refusal) => Outcome::from(Err(refusal)),
    };

    ReplayReport {
        commands: log.commands,
        truncated_after: log.truncated_after,
        direct,
        via_checkpoint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(file: &str, offset: usize, text: &str) -> Command {
        Command::Insert { file: file.into(), offset, text: text.into() }
    }

    fn history() -> (Workspace, Vec<Command>) {
        (
            Workspace::with(&[("a.rs", "")]),
            vec![
                insert("a.rs", 0, "one"),
                insert("a.rs", 3, "two"),
                Command::CreateFile { path: "b.rs".into() },
                insert("b.rs", 0, "three"),
            ],
        )
    }

    /// @tests REQ-PERSIST.replay_exact
    #[test]
    fn replay_reaches_the_state_it_was_recorded_from() {
        let (base, commands) = history();
        let direct = replay(base.clone(), commands.clone()).unwrap();
        let text: String = commands.iter().map(entry).collect();
        let log = parse_log(&text);
        assert_eq!(log.truncated_after, None);
        assert_eq!(replay(base, log.commands).unwrap(), direct);
    }

    /// @tests REQ-PERSIST.checkpoint_equivalent
    #[test]
    fn a_checkpoint_cannot_change_an_answer() {
        let (base, commands) = history();
        let full = replay(base.clone(), commands.clone()).unwrap();
        for entries in 0..=commands.len() {
            let point = checkpoint(base.clone(), &commands, entries).unwrap();
            assert_eq!(replay_from(&point, &commands).unwrap(), full, "at {entries}");
        }
    }

    /// @tests REQ-PERSIST.truncated_is_reported
    #[test]
    fn a_truncated_log_replays_to_the_last_complete_entry_and_says_so() {
        let (base, commands) = history();
        let mut text: String = commands.iter().map(entry).collect();
        text.push_str("{\"command\":\"insert\",\"file\":\"a.r");

        let log = parse_log(&text);
        assert_eq!(log.truncated_after, Some(4));
        assert_eq!(log.commands.len(), 4);
        assert!(replay(base, log.commands).is_ok());
    }

    #[test]
    fn entries_after_the_damage_are_not_replayed() {
        let text = format!(
            "{}{}{}",
            entry(&insert("a.rs", 0, "one")),
            "{ not json\n",
            entry(&insert("a.rs", 0, "later"))
        );
        let log = parse_log(&text);
        assert_eq!(log.commands.len(), 1);
        assert_eq!(log.truncated_after, Some(1));
    }

    /// @tests REQ-PERSIST.portable
    /// @structural REQ-PERSIST.portable reason="a claim that a journal names nothing machine-specific, which is an absence in what the record type can hold"
    #[test]
    fn a_log_carries_no_machine_identity() {
        let text: String = history().1.iter().map(entry).collect();
        for marker in ["/home/", "/tmp/", "C:\\"] {
            assert!(!text.contains(marker), "the log embedded {marker}");
        }
    }

    #[test]
    fn a_command_round_trips_through_the_log() {
        for command in history().1 {
            let text = entry(&command);
            assert_eq!(parse_log(&text).commands, vec![command]);
        }
    }
}
