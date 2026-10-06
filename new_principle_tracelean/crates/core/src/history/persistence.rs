//! Persistence and replay.
//!
//! A history that does not reproduce its own state is a log, not a record. The
//! decisions here are pure — parsing a log, replaying it, deciding what a
//! truncated record means — and writing bytes is the shell's job.

use serde::{Deserialize, Serialize};

use super::command::{apply, Command, Outcome, Refusal, Workspace};

/// A recorded history: a base state and the commands applied to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub workspace: Workspace,
    /// How many log entries this checkpoint already accounts for.
    pub entries: usize,
}

/// What a log yielded, and what was wrong with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    pub commands: Vec<Command>,
    /// Set when the record ends mid-entry — a process killed during a write.
    ///
    /// Replaying to the last complete entry and saying so is recoverable.
    /// Refusing to open the project is not, and silently accepting a half entry
    /// writes corruption over a good state.
    ///
    /// @implements REQ-PERSIST.truncated_is_reported
    pub truncated_after: Option<usize>,
}

/// Parse an append-only log of one JSON command per line.
///
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
                // The first unreadable entry ends the history: everything after
                // it was written after a failure and cannot be trusted to
                // follow from what came before.
                truncated_after = Some(index);
                break;
            }
        }
    }

    Log { commands, truncated_after }
}

/// Serialise a command as one log entry.
pub fn entry(command: &Command) -> String {
    let mut line = serde_json::to_string(command).unwrap_or_default();
    line.push('\n');
    line
}

/// Replay commands onto a base state.
///
/// @implements REQ-PERSIST.replay_exact
pub fn replay(base: Workspace, commands: Vec<Command>) -> Result<Workspace, Refusal> {
    let mut workspace = base;
    for command in &commands {
        workspace = apply(&workspace, command)?;
    }
    Ok(workspace)
}

/// Replay from a checkpoint, skipping the entries it already accounts for.
///
/// A checkpoint is a performance device, so it is specified as an equivalence
/// to the unoptimised path rather than as a feature: replaying from it must
/// reach the same state as replaying from the beginning.
///
/// @implements REQ-PERSIST.checkpoint_equivalent
pub fn replay_from(
    checkpoint: &Checkpoint,
    commands: &[Command],
) -> Result<Workspace, Refusal> {
    let rest = commands.iter().skip(checkpoint.entries).cloned().collect();
    replay(checkpoint.workspace.clone(), rest)
}

/// Take a checkpoint after `entries` commands.
pub fn checkpoint(base: Workspace, commands: &[Command], entries: usize) -> Result<Checkpoint, Refusal> {
    let taken = commands.iter().take(entries).cloned().collect();
    Ok(Checkpoint { workspace: replay(base, taken)?, entries })
}

/// What replaying a log produced, both ways.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    /// The commands the log yielded.
    pub commands: Vec<Command>,
    pub truncated_after: Option<usize>,
    /// Replaying the whole log from the base.
    pub direct: Outcome,
    /// Replaying from a checkpoint taken part-way through.
    pub via_checkpoint: Outcome,
}

/// Parse a log, then replay it twice: straight through, and from a checkpoint.
///
/// A checkpoint is a performance device, so the requirement states it as an
/// equivalence rather than as a feature. Putting both answers in one report
/// makes the equivalence a divergence when it fails, instead of something a
/// reader has to notice.
///
/// `lines` are joined with newlines because a generator over a flat string
/// would never produce a log at all, let alone one that ends mid-entry.
///
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
        // The same commands, written out and read back.
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
        // A process killed mid-write leaves half an entry.
        text.push_str("{\"command\":\"insert\",\"file\":\"a.r");

        let log = parse_log(&text);
        assert_eq!(log.truncated_after, Some(4));
        assert_eq!(log.commands.len(), 4);
        // And what survived still replays.
        assert!(replay(base, log.commands).is_ok());
    }

    /// Nothing after the first damaged entry is trusted: it was written after a
    /// failure and does not follow from what came before.
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
