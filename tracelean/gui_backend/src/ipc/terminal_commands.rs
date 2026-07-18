//! P12: project test runner IPC — run a command in the project root and
//! return output + parsed failures.

use crate::AppStateWrapper;
use tauri::State;
use tracelean_core::testrun::{self, CommandOutput};

/// The configured (or auto-detected) test command for the open project.
#[tauri::command]
pub fn get_test_command(state: State<'_, AppStateWrapper>) -> Result<String, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().ok_or("no project open")?;
    Ok(testrun::test_command(&root))
}

/// Persist the test command to {project}/.tracelean/test_command.
#[tauri::command]
pub fn set_test_command(
    state: State<'_, AppStateWrapper>,
    command: String,
) -> Result<(), String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().ok_or("no project open")?;
    testrun::save_test_command(&root, &command)
}

/// Run a shell command in the project root (5-minute cap) and parse test
/// failures from the combined output.
#[tauri::command]
pub async fn run_project_command(
    state: State<'_, AppStateWrapper>,
    command: String,
) -> Result<CommandOutput, String> {
    let root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("no project open")?
    };
    if command.trim().is_empty() {
        return Err("empty command".into());
    }

    let started = std::time::Instant::now();
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(&root)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(300), child)
        .await
        .map_err(|_| "command timed out after 300s".to_string())?
        .map_err(|e| format!("failed to run '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    Ok(CommandOutput {
        command,
        stdout,
        stderr,
        exit_code: output.status.code().unwrap_or(-1),
        duration_ms: started.elapsed().as_millis() as u64,
        failures: testrun::parse_test_failures(&combined),
    })
}
