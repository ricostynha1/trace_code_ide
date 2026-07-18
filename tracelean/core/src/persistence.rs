//! Persistence: serialize command log to `.tracelean/commands/`.
//! On startup, replay from last checkpoint.
//!
//! The log format is versioned. Logs from older versions (byte-offset
//! Insert/Delete commands) are discarded on load — replaying them under
//! char-index semantics would corrupt buffers.

use crate::commands::Command;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const TRACELEAN_DIR: &str = ".tracelean";
const COMMANDS_DIR: &str = "commands";
const CHECKPOINT_FILE: &str = "checkpoint.json";
const LOG_FILE: &str = "command_log.json";

/// Bump when Command semantics change incompatibly.
/// v2: char-indexed Replace primitive (2026-07).
pub const LOG_VERSION: u32 = 2;

/// Versioned on-disk command log.
#[derive(Debug, Serialize, Deserialize)]
struct CommandLogFile {
    version: u32,
    commands: Vec<Command>,
}

/// Checkpoint: full state snapshot for fast startup
#[derive(Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    #[serde(default)]
    pub version: u32,
    pub state: AppState,
    pub command_index: usize,
}

/// Get the .tracelean directory path for a project
pub fn tracelean_dir(project_root: &Path) -> PathBuf {
    project_root.join(TRACELEAN_DIR)
}

/// Get the commands directory
pub fn commands_dir(project_root: &Path) -> PathBuf {
    tracelean_dir(project_root).join(COMMANDS_DIR)
}

/// Ensure .tracelean/commands/ exists
pub fn ensure_dirs(project_root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(commands_dir(project_root))
}

/// Save command log to disk
pub fn save_command_log(project_root: &Path, commands: &[Command]) -> std::io::Result<()> {
    ensure_dirs(project_root)?;
    let path = commands_dir(project_root).join(LOG_FILE);
    let log = CommandLogFile {
        version: LOG_VERSION,
        commands: commands.to_vec(),
    };
    let json = serde_json::to_string_pretty(&log)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}

/// Load command log from disk. Incompatible or unparseable logs are discarded
/// (with a warning) rather than replayed wrongly.
pub fn load_command_log(project_root: &Path) -> std::io::Result<Vec<Command>> {
    let path = commands_dir(project_root).join(LOG_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = fs::read_to_string(path)?;
    match serde_json::from_str::<CommandLogFile>(&json) {
        Ok(log) if log.version == LOG_VERSION => Ok(log.commands),
        Ok(log) => {
            eprintln!(
                "Warning: command log version {} != {} — discarding history (fresh start)",
                log.version, LOG_VERSION
            );
            Ok(Vec::new())
        }
        Err(e) => {
            eprintln!(
                "Warning: command log unreadable ({}) — discarding history (fresh start)",
                e
            );
            Ok(Vec::new())
        }
    }
}

/// Save a full state checkpoint
pub fn save_checkpoint(project_root: &Path, state: &AppState) -> std::io::Result<()> {
    ensure_dirs(project_root)?;
    let checkpoint = Checkpoint {
        version: LOG_VERSION,
        state: state.clone(),
        command_index: state.command_log().len(),
    };
    let path = commands_dir(project_root).join(CHECKPOINT_FILE);
    let json = serde_json::to_string_pretty(&checkpoint)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}

/// Load checkpoint from disk. Incompatible checkpoints are discarded.
pub fn load_checkpoint(project_root: &Path) -> std::io::Result<Option<Checkpoint>> {
    let path = commands_dir(project_root).join(CHECKPOINT_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(path)?;
    match serde_json::from_str::<Checkpoint>(&json) {
        Ok(cp) if cp.version == LOG_VERSION => Ok(Some(cp)),
        Ok(cp) => {
            eprintln!(
                "Warning: checkpoint version {} != {} — ignoring checkpoint",
                cp.version, LOG_VERSION
            );
            Ok(None)
        }
        Err(e) => {
            eprintln!("Warning: checkpoint unreadable ({}) — ignoring checkpoint", e);
            Ok(None)
        }
    }
}

/// Restore state: load checkpoint + replay remaining commands
pub fn restore_state(project_root: &Path) -> std::io::Result<AppState> {
    // Try checkpoint first
    if let Some(checkpoint) = load_checkpoint(project_root)? {
        let all_commands = load_command_log(project_root)?;
        let mut state = checkpoint.state;
        // Replay commands after checkpoint (log may have been discarded on
        // version mismatch — guard the slice)
        if all_commands.len() >= checkpoint.command_index {
            let remaining = all_commands[checkpoint.command_index..].to_vec();
            state.replay(remaining);
        }
        return Ok(state);
    }

    // No checkpoint: replay all commands
    let commands = load_command_log(project_root)?;
    let mut state = AppState::new();
    state.replay(commands);
    Ok(state)
}
