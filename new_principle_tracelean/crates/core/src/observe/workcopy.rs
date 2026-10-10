//! Sandbox sessions on disk: making the copy, finding it again, ending it.
//!
//! A shell. The copy is `workspace::write_into` of a snapshot, so it holds
//! exactly the mirrored files (`.git` and `.tracelean` are protected and never
//! copied); what the command line looks like is `sandbox::launch_args`.
//!
//! Sessions live under `.tracelean/sessions/<id>/work`, so they survive the
//! editor being closed and reopened, and one session per project is live: the
//! newest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::history::command::Workspace;
use crate::observe::sandbox::{brief, brief_path, launcher, launcher_path, Host, Session, HOME_READABLE, HOME_WRITABLE};
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
    write_base(&sessions_dir(&root).join(&id), holds)?;
    copy_state(&root.join(".tracelean"), &work.join(".tracelean"))?;
    Ok(Session {
        id,
        root: root.to_string_lossy().to_string(),
        work: work.to_string_lossy().to_string(),
    })
}

/// Where a session keeps what its copy started with: beside the copy, where
/// the agent cannot rewrite it.
fn base_path(session_dir: &Path) -> PathBuf {
    session_dir.join("base.json")
}

/// Record each file the copy started with, by the hash of its text, so what
/// the agent changed can be told from what the project changed since.
///
/// @implements REQ-OBS.only_what_the_tool_changed
fn write_base(session_dir: &Path, holds: &Workspace) -> std::io::Result<()> {
    let hashes: BTreeMap<&String, String> =
        holds.files.iter().map(|(path, text)| (path, crate::trace::hash::text(text))).collect();
    let text = serde_json::to_string_pretty(&hashes).map_err(std::io::Error::other)?;
    std::fs::write(base_path(session_dir), text)
}

/// What a session's copy started with; `None` for a session made before
/// this was kept, whose changes cannot be told from the project's.
pub fn base(session: &Session) -> Option<BTreeMap<String, String>> {
    let dir = Path::new(&session.work).parent()?;
    serde_json::from_str(&std::fs::read_to_string(base_path(dir)).ok()?).ok()
}

/// What `.tracelean` holds that is one person's, not the project's: the
/// sessions themselves, and what the editor had open.
const NOT_COPIED: &[&str] = &["sessions", "editor.json"];

/// Copy the project's own state directory into the session, so a tool in it
/// reads the bindings and evidence the project has (`tracelean-trace` does)
/// and may write there. Protected, so nothing written comes back.
///
/// @implements REQ-OBS.workspace_is_a_copy
fn copy_state(from: &Path, to: &Path) -> std::io::Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else { return Ok(()) };
    std::fs::create_dir_all(to)?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if NOT_COPIED.iter().any(|skip| name.to_str() == Some(*skip)) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(&name))?;
        } else {
            std::fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Write the session's launcher, so the command a person copies is short.
/// Written again each time, because what the host has can change.
pub fn write_launcher(session: &Session, host: &Host) -> std::io::Result<()> {
    std::fs::write(brief_path(session), brief(host))?;
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
        // The checker beside the running editor (or one directory up, for an
        // example's binary), else the one on the host's `PATH`.
        tools: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .into_iter()
            .flat_map(|dir| [dir.clone(), dir.parent().map(Path::to_path_buf).unwrap_or(dir)])
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))
            .find(|dir| dir.join("tracelean-trace").is_file())
            .and_then(|dir| dir.canonicalize().ok())
            .map(|dir| (dir.display().to_string(), std::env::var("PATH").unwrap_or_default())),
        // The skills shipped with this TraceLean, where it was built from.
        skills: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills"))
            .filter(|dir| dir.join("README.md").is_file())
            .and_then(|dir| dir.canonicalize().ok())
            .map(|dir| dir.display().to_string()),
        // `CLAUDE_CONFIG_DIR` reaches the sandbox with the rest of the
        // environment; what it names has to reach it too.
        claude_config: std::env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .filter(|dir| Path::new(dir).is_dir())
            .and_then(|dir| Path::new(&dir).canonicalize().ok())
            .map(|dir| dir.display().to_string()),
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
        std::fs::create_dir_all(root.join(".tracelean/pins")).unwrap();
        std::fs::write(root.join(".tracelean/drt.json"), "{}").unwrap();
        std::fs::write(root.join(".tracelean/pins/p.json"), "[]").unwrap();
        std::fs::write(root.join(".tracelean/editor.json"), "mine").unwrap();

        let session = create(&root, &snapshot(&root)).unwrap();
        let work = Path::new(&session.work);
        assert_eq!(std::fs::read_to_string(work.join("src/a.rs")).unwrap(), "a");
        // Version control is shown read-only by the launch, not copied.
        assert!(!work.join(".git").exists(), "version control was copied");
        // The project's state is there for a tool to read; one person's is not,
        // and nor are the sessions, which would copy the copy into itself.
        assert_eq!(std::fs::read_to_string(work.join(".tracelean/drt.json")).unwrap(), "{}");
        assert!(work.join(".tracelean/pins/p.json").exists());
        assert!(!work.join(".tracelean/editor.json").exists());
        assert!(!work.join(".tracelean/sessions").exists());

        // And the copy law holds of what the tool is given.
        use crate::observe::effects::run_violations;
        use crate::observe::sandbox::bound;
        let seen = bound(&Host { git: true, ..Host::default() });
        let law = run_violations(crate::observe::workspace::witness(&root, work, &seen, vec![]));
        assert_eq!(law, vec![], "a session broke the copy law");
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
