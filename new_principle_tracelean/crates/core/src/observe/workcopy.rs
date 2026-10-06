//! Sandbox sessions on disk: making the copy, finding it again, ending it.
//!
//! A shell. The copy is `workspace::write_into` of a snapshot, so it holds
//! exactly the mirrored files (`.git` and `.tracelean` are protected and never
//! copied); what the command line looks like is `sandbox::launch_args`.
//!
//! Sessions live under `.tracelean/sessions/<id>/work`, so they survive the
//! editor being closed and reopened, and one session per project is live: the
//! newest.

use std::path::{Path, PathBuf};

use crate::history::command::Workspace;
use crate::observe::sandbox::{launcher, launcher_path, Host, Session, HOME_READABLE, HOME_WRITABLE};
use crate::observe::workspace::{snapshot, write_into};

/// Where a project's sessions are kept.
pub fn sessions_dir(root: &Path) -> PathBuf {
    root.join(".tracelean").join("sessions")
}

fn absolute(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Make a session: a fresh copy of the project as the editor holds it.
///
/// What the editor holds rather than what is on disk: the agent should start
/// from what the person sees, and its changes are measured against that. A
/// copy of the disk while an edit was unsaved listed that edit's absence as
/// something the agent did.
///
/// @implements REQ-OBS.workspace_is_a_copy
pub fn create(root: &Path, holds: &Workspace) -> std::io::Result<Session> {
    let root = absolute(root);
    // Numbered rather than timed (`ARCH-DETERMINISM.no_ambient_time`): the next
    // number after every session the project holds, padded so that the names
    // sort in the order they were made.
    let next = list(&root)
        .iter()
        .filter_map(|s| s.id.trim_start_matches('s').parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1);
    let id = format!("s{next:04}");
    let work = sessions_dir(&root).join(&id).join("work");
    std::fs::create_dir_all(&work)?;
    write_into(holds, &work)?;
    Ok(Session {
        id,
        root: root.to_string_lossy().to_string(),
        work: work.to_string_lossy().to_string(),
    })
}

/// Write the session's launcher, so the command a person copies is short.
/// Written again each time, because what the host has can change.
pub fn write_launcher(session: &Session, host: &Host) -> std::io::Result<()> {
    std::fs::write(launcher_path(session), launcher(session, host))
}

/// Every session of a project, newest first.
pub fn list(root: &Path) -> Vec<Session> {
    let root = absolute(root);
    let Ok(entries) = std::fs::read_dir(sessions_dir(&root)) else { return Vec::new() };
    let mut found: Vec<Session> = entries
        .flatten()
        .filter(|e| e.path().join("work").is_dir())
        .map(|e| Session {
            id: e.file_name().to_string_lossy().to_string(),
            root: root.to_string_lossy().to_string(),
            work: e.path().join("work").to_string_lossy().to_string(),
        })
        .collect();
    found.sort_by(|a, b| b.id.cmp(&a.id));
    found
}

/// When a session began: its directory's creation, or failing that its
/// modification, time.
pub fn began(session: &Session) -> std::time::SystemTime {
    let dir = Path::new(&session.work);
    std::fs::metadata(dir)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// End a session: remove its copy.
pub fn destroy(session: &Session) -> std::io::Result<()> {
    match Path::new(&session.work).parent() {
        Some(dir) => std::fs::remove_dir_all(dir),
        None => Ok(()),
    }
}

/// Make a directory hold exactly `workspace`'s mirrored files: write every one,
/// and remove any mirrored file the workspace does not have.
///
/// How a rejected change is taken back out of a session's copy, and how an
/// accepted deletion reaches the real tree.
pub fn sync(dir: &Path, workspace: &Workspace) -> std::io::Result<()> {
    for path in snapshot(dir).files.keys() {
        if !workspace.files.contains_key(path) {
            std::fs::remove_file(dir.join(path))?;
        }
    }
    write_into(workspace, dir)
}

/// Make one file under `dir` hold `text`, or not exist when there is none:
/// how a single accepted or rejected change reaches one tree.
pub fn put(dir: &Path, path: &str, text: Option<&str>) -> std::io::Result<()> {
    let target = dir.join(path);
    match text {
        Some(text) => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, text)
        }
        None if target.exists() => std::fs::remove_file(target),
        None => Ok(()),
    }
}

/// What this machine has that the sandbox command needs.
pub fn host(root: &Path) -> Host {
    let home = std::env::var("HOME").ok();
    let present = |list: &[&str]| -> Vec<String> {
        match &home {
            Some(home) => list
                .iter()
                .filter(|rel| Path::new(home).join(rel).exists())
                .map(|rel| rel.to_string())
                .collect(),
            None => Vec::new(),
        }
    };
    Host {
        writable: present(HOME_WRITABLE),
        readable: present(HOME_READABLE),
        home,
        git: absolute(root).join(".git").exists(),
        runtime_dir: std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| Path::new(d).is_dir()),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session is a copy: the project's files, without its protected ones,
    /// and nothing written to the project itself.
    ///
    /// @tests REQ-OBS.workspace_is_a_copy
    #[test]
    fn a_session_copies_the_project_and_sync_takes_changes_back_out() {
        let root = std::env::temp_dir().join(format!("tracelean-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("src/a.rs"), "a").unwrap();
        std::fs::write(root.join(".git/HEAD"), "x").unwrap();

        let session = create(&root, &snapshot(&root)).unwrap();
        let work = Path::new(&session.work);
        assert_eq!(std::fs::read_to_string(work.join("src/a.rs")).unwrap(), "a");
        assert!(!work.join(".git").exists(), "a protected path was copied");
        assert_eq!(list(&root).first().map(|s| s.id.clone()), Some(session.id.clone()));

        // An agent writes in the copy; syncing to the original state undoes it.
        std::fs::write(work.join("src/a.rs"), "changed").unwrap();
        std::fs::write(work.join("src/new.rs"), "new").unwrap();
        sync(work, &snapshot(&root)).unwrap();
        assert_eq!(std::fs::read_to_string(work.join("src/a.rs")).unwrap(), "a");
        assert!(!work.join("src/new.rs").exists());

        destroy(&session).unwrap();
        assert!(list(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
