//! Persistence: serialize command log to `.tracelean/commands/`.
//! On startup, replay from last checkpoint.

use crate::commands::Command;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const TRACELEAN_DIR: &str = ".tracelean";
const COMMANDS_DIR: &str = "commands";
const CHECKPOINT_FILE: &str = "checkpoint.json";
const LOG_FILE: &str = "command_log.json";

/// Checkpoint: full state snapshot for fast startup
#[derive(Debug, Serialize, Deserialize)]
pub struct Checkpoint {
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
    let json = serde_json::to_string_pretty(commands)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}

/// Load command log from disk
pub fn load_command_log(project_root: &Path) -> std::io::Result<Vec<Command>> {
    let path = commands_dir(project_root).join(LOG_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = fs::read_to_string(path)?;
    let commands: Vec<Command> = serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(commands)
}

/// Save a full state checkpoint
pub fn save_checkpoint(project_root: &Path, state: &AppState) -> std::io::Result<()> {
    ensure_dirs(project_root)?;
    let checkpoint = Checkpoint {
        state: state.clone(),
        command_index: state.command_log().len(),
    };
    let path = commands_dir(project_root).join(CHECKPOINT_FILE);
    let json = serde_json::to_string_pretty(&checkpoint)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}

/// Load checkpoint from disk
pub fn load_checkpoint(project_root: &Path) -> std::io::Result<Option<Checkpoint>> {
    let path = commands_dir(project_root).join(CHECKPOINT_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(path)?;
    let checkpoint: Checkpoint = serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(Some(checkpoint))
}

/// Restore state: load checkpoint + replay remaining commands
pub fn restore_state(project_root: &Path) -> std::io::Result<AppState> {
    // Try checkpoint first
    if let Some(checkpoint) = load_checkpoint(project_root)? {
        let all_commands = load_command_log(project_root)?;
        let mut state = checkpoint.state;
        // Replay commands after checkpoint
        let remaining = &all_commands[checkpoint.command_index..];
        for cmd in remaining {
            state.apply(cmd.clone());
        }
        return Ok(state);
    }

    // No checkpoint: replay all commands
    let commands = load_command_log(project_root)?;
    let mut state = AppState::new();
    state.replay(commands);
    Ok(state)
}
