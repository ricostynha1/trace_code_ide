//! Session lifecycle: a long-lived, reflink-copied working tree that a
//! session's `bwrap` shells are bound into, replacing the ephemeral
//! per-command overlay in `ai::shell_sandbox` for the "run an external tool
//! yourself, watch the effects" workflow. A session is nothing but a
//! directory plus a spec file — no daemon, no long-lived namespace — so any
//! number of shells can be spawned into the same session over its lifetime
//! (see `bin/tracelean-sandbox.rs`), which is what makes "open an external
//! terminal into it" possible at all: you cannot re-enter someone else's
//! unprivileged user namespace, so every entry is a fresh `bwrap` bound to
//! the same `work_dir`.

use crate::ai::shell_sandbox::{HOME_ALLOWLIST, PROJECT_ALLOWLIST, PROTECTED};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::OnceLock;

fn default_true() -> bool {
    true
}

/// Persisted description of one sandbox session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSpec {
    pub id: String,
    pub project_root: PathBuf,
    pub work_dir: PathBuf,
    /// Claude Code (and most agent tooling) needs the network — sessions
    /// default to allowing it, unlike the ephemeral agent-shell sandbox.
    #[serde(default = "default_true")]
    pub allow_network: bool,
    pub created: DateTime<Utc>,
}

fn sessions_dir(project_root: &Path) -> PathBuf {
    project_root.join(".tracelean").join("sessions")
}

fn session_dir(project_root: &Path, id: &str) -> PathBuf {
    sessions_dir(project_root).join(id)
}

fn spec_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session.json")
}

/// Whether `bwrap` is usable at all on this machine.
pub fn bwrap_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        StdCommand::new("bwrap")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Probe once whether reflink copies are supported for this project's
/// filesystem (btrfs/xfs). `cp --reflink=auto` falls back silently to a
/// full copy regardless — this only drives a UI hint about session-start
/// cost, never correctness.
pub fn reflink_supported(project_root: &Path) -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| probe_reflink(project_root))
}

fn probe_reflink(project_root: &Path) -> bool {
    let base = sessions_dir(project_root);
    if std::fs::create_dir_all(&base).is_err() {
        return false;
    }
    let src = base.join(".reflink-probe-src");
    let dst = base.join(".reflink-probe-dst");
    let _ = std::fs::write(&src, b"probe");
    let ok = StdCommand::new("cp")
        .arg("--reflink=always")
        .arg(&src)
        .arg(&dst)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    ok
}

/// Capability snapshot surfaced to the UI (settings hints, disabled
/// buttons), mirroring `ai::shell_sandbox`'s `overlay_available`.
#[derive(Debug, Clone, Serialize)]
pub struct SandboxCapabilities {
    pub bwrap_available: bool,
    pub reflink_available: bool,
}

pub fn capabilities(project_root: &Path) -> SandboxCapabilities {
    SandboxCapabilities {
        bwrap_available: bwrap_available(),
        reflink_available: reflink_supported(project_root),
    }
}

/// Create a new session: reflink-copy the project into a fresh work dir and
/// persist the spec. `PROJECT_ALLOWLIST` (`target`, `node_modules`, ...) and
/// `PROTECTED` (`.git`, `.tracelean`) are skipped — they are bind-mounted
/// straight from the real tree instead (see `bwrap_argv`), never copied or
/// diffed.
pub fn create_session(project_root: &Path, allow_network: bool) -> Result<SessionSpec, String> {
    let project_root = project_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize project root: {}", e))?;
    let id = uuid::Uuid::new_v4().to_string();
    let dir = session_dir(&project_root, &id);
    let work_dir = dir.join("work");
    std::fs::create_dir_all(&work_dir).map_err(|e| format!("cannot create session dir: {}", e))?;

    if let Err(e) = copy_project_into(&project_root, &work_dir) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }

    let spec = SessionSpec {
        id,
        project_root: project_root.clone(),
        work_dir,
        allow_network,
        created: Utc::now(),
    };
    save_spec(&dir, &spec)?;
    Ok(spec)
}

fn save_spec(dir: &Path, spec: &SessionSpec) -> Result<(), String> {
    let json = serde_json::to_string_pretty(spec).map_err(|e| e.to_string())?;
    std::fs::write(spec_path(dir), json).map_err(|e| e.to_string())
}

fn copy_project_into(project_root: &Path, work_dir: &Path) -> Result<(), String> {
    let skip: Vec<&str> = PROTECTED.iter().chain(PROJECT_ALLOWLIST.iter()).copied().collect();
    let entries = std::fs::read_dir(project_root).map_err(|e| format!("cannot read project root: {}", e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.contains(&name_str.as_ref()) {
            continue;
        }
        let dest = work_dir.join(&name);
        let status = StdCommand::new("cp")
            .arg("-a")
            .arg("--reflink=auto")
            .arg(entry.path())
            .arg(&dest)
            .status()
            .map_err(|e| format!("cp failed: {}", e))?;
        if !status.success() {
            return Err(format!("copying '{}' into session failed", name_str));
        }
    }
    Ok(())
}

/// Build the `bwrap` argv for entering `spec`'s workspace. The caller
/// appends the command to run (interactively, a shell — see
/// `bin/tracelean-sandbox.rs`).
pub fn bwrap_argv(spec: &SessionSpec) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "--ro-bind".into(), "/".into(), "/".into(),
        "--dev".into(), "/dev".into(),
        "--proc".into(), "/proc".into(),
    ];

    let home = std::env::var_os("HOME").map(PathBuf::from);

    // SECURITY: `--ro-bind / /` above makes the *entire* host filesystem
    // readable inside the sandbox, including every other file and project
    // under the user's home directory — a session for one project could
    // otherwise `cd ..` and read siblings, other repos, anything under
    // $HOME. That's fine for system paths (/usr, /etc, toolchains) but not
    // for personal files, which is the whole point of "a sandbox for this
    // project." Blank $HOME with an empty tmpfs *before* anything below is
    // bound (bind order matters in bwrap — later operations win at a given
    // path), then punch back exactly: the project itself, toolchain
    // caches, and the external agent's own config. Everything else under
    // $HOME is invisible from inside the sandbox.
    //
    // Deliberately NOT restored: shell rc files (.bashrc/.zshrc/...) — an
    // interactive shell in the sandbox gets a default environment, not the
    // user's customized one — and `.ssh` (private keys) — git operations
    // needing SSH auth aren't supported from inside a sandbox by default.
    // Both are easy to add back later if that tradeoff turns out wrong.
    if let Some(home) = &home {
        argv.push("--tmpfs".into());
        argv.push(home.to_string_lossy().to_string());
    }

    let project = spec.project_root.to_string_lossy().to_string();
    argv.push("--bind".into());
    argv.push(spec.work_dir.to_string_lossy().to_string());
    argv.push(project.clone());

    for rel in PROJECT_ALLOWLIST {
        let dir = spec.project_root.join(rel);
        if dir.is_dir() {
            bind(&mut argv, "--bind", &dir);
        }
    }

    // Unlike the ephemeral agent-shell sandbox (which discards all `.git`/
    // `.tracelean` writes, see `PROTECTED`), sessions bind the real `.git`
    // through — commits made inside the sandbox are meant to land for real.
    let git_dir = spec.project_root.join(".git");
    if git_dir.exists() {
        bind(&mut argv, "--bind", &git_dir);
    }
    let tracelean_dir = spec.project_root.join(".tracelean");
    if tracelean_dir.exists() {
        bind(&mut argv, "--ro-bind", &tracelean_dir);
        let tmp = tracelean_dir.join("tmp");
        let _ = std::fs::create_dir_all(&tmp);
        bind(&mut argv, "--bind", &tmp);
    }

    if let Some(home) = &home {
        for rel in HOME_ALLOWLIST {
            let dir = home.join(rel);
            if dir.is_dir() {
                bind(&mut argv, "--bind", &dir);
            }
        }
        // Claude Code's own state: session transcripts, auth, config —
        // needed both for the tool to work and for the transcript tail
        // (`sandbox::transcript`) to have something to read. Claude Code
        // honors `$CLAUDE_CONFIG_DIR` to relocate all of this off the
        // default `~/.claude` — bind that too (bwrap inherits the parent
        // shell's env, so if the user's shell sets it, it's already set
        // here). Binding both is harmless when they're the same or when
        // one doesn't exist.
        let mut claude_dirs = vec![home.join(".claude")];
        if let Some(custom) = std::env::var_os("CLAUDE_CONFIG_DIR") {
            claude_dirs.push(PathBuf::from(custom));
        }
        for dir in claude_dirs {
            if dir.exists() {
                bind(&mut argv, "--bind", &dir);
            }
        }
        let claude_json = home.join(".claude.json");
        if claude_json.exists() {
            bind(&mut argv, "--bind", &claude_json);
        }
        // Git identity (name/email/aliases) — read-only, so a sandboxed
        // `git commit` doesn't fail for lack of a configured author, but
        // nothing sandboxed can rewrite the user's global git config.
        let gitconfig = home.join(".gitconfig");
        if gitconfig.is_file() {
            bind(&mut argv, "--ro-bind", &gitconfig);
        }
    }
    // The user session bus / keyring socket typically lives here; bind
    // writable so credential lookups still work under the read-only root.
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(runtime_dir);
        if p.is_dir() {
            bind(&mut argv, "--bind", &p);
        }
    }

    let tmp_root = std::env::temp_dir();
    if !spec.project_root.starts_with(&tmp_root) {
        argv.push("--tmpfs".into());
        argv.push("/tmp".into());
    }

    if !spec.allow_network {
        argv.push("--unshare-net".into());
    }

    argv.push("--die-with-parent".into());
    argv.push("--chdir".into());
    argv.push(project);

    argv
}

fn bind(argv: &mut Vec<String>, flag: &str, path: &Path) {
    let s = path.to_string_lossy().to_string();
    argv.push(flag.into());
    argv.push(s.clone());
    argv.push(s);
}

pub fn destroy_session(spec: &SessionSpec) -> Result<(), String> {
    let dir = session_dir(&spec.project_root, &spec.id);
    std::fs::remove_dir_all(&dir).map_err(|e| format!("cannot remove session dir: {}", e))
}

pub fn load_session(project_root: &Path, id: &str) -> Result<SessionSpec, String> {
    let dir = session_dir(project_root, id);
    let json = std::fs::read_to_string(spec_path(&dir)).map_err(|e| format!("cannot read session spec: {}", e))?;
    serde_json::from_str(&json).map_err(|e| format!("cannot parse session spec: {}", e))
}

/// All sessions for a project, newest first.
pub fn list_sessions(project_root: &Path) -> Vec<SessionSpec> {
    let base = sessions_dir(project_root);
    let Ok(entries) = std::fs::read_dir(&base) else { return Vec::new() };
    let mut specs: Vec<SessionSpec> = entries
        .flatten()
        .filter_map(|e| load_session(project_root, &e.file_name().to_string_lossy()).ok())
        .collect();
    specs.sort_by(|a, b| b.created.cmp(&a.created));
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fake_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/out.bin"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        dir
    }

    #[test]
    fn bwrap_argv_binds_network_by_default_and_git_through() {
        let dir = fake_project();
        let spec = create_session(dir.path(), true).unwrap();
        let argv = bwrap_argv(&spec);
        assert!(!argv.iter().any(|a| a == "--unshare-net"), "network should be allowed by default");
        let git_str = spec.project_root.join(".git").to_string_lossy().to_string();
        assert!(argv.contains(&git_str), ".git should be bound through");
        let _ = destroy_session(&spec);
    }

    #[test]
    fn bwrap_argv_binds_custom_claude_config_dir() {
        // Regression: Claude Code honors $CLAUDE_CONFIG_DIR to relocate its
        // whole state dir off ~/.claude. If that's set (as it commonly is)
        // and we don't bind it, Claude Code sees a read-only filesystem
        // there under `--ro-bind / /` and its transcript writes fail.
        let dir = fake_project();
        let custom_claude = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("CLAUDE_CONFIG_DIR");
        std::env::set_var("CLAUDE_CONFIG_DIR", custom_claude.path());
        let spec = create_session(dir.path(), true).unwrap();
        let argv = bwrap_argv(&spec);
        match prev {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        let custom_str = custom_claude.path().to_string_lossy().to_string();
        assert!(argv.contains(&custom_str), "custom CLAUDE_CONFIG_DIR should be bound");
        let _ = destroy_session(&spec);
    }

    #[test]
    fn sandbox_hides_home_directory_siblings_but_keeps_the_project_visible() {
        // SECURITY regression: `--ro-bind / /` makes the whole host
        // filesystem readable inside the sandbox by default. Without the
        // $HOME tmpfs lockdown, a session for one project could `cd ..`
        // and read every other personal file/project under $HOME — this
        // proves that's actually closed, with a real bwrap run, not just
        // an argv shape assertion.
        if !bwrap_available() {
            eprintln!("skipping: bwrap not on PATH");
            return;
        }

        let fake_home = tempfile::tempdir().unwrap();
        std::fs::write(fake_home.path().join("private_diary.txt"), "top secret sibling").unwrap();
        let project_dir = fake_home.path().join("myproject");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("a.txt"), "hello").unwrap();

        let prev_home = std::env::var_os("HOME");
        std::env::set_var("HOME", fake_home.path());
        let spec = create_session(&project_dir, true);
        let argv = spec.as_ref().ok().map(bwrap_argv);
        if let Some(v) = prev_home {
            std::env::set_var("HOME", v);
        }
        let spec = spec.unwrap();
        let mut argv = argv.unwrap();

        argv.push("sh".into());
        argv.push("-c".into());
        argv.push(format!(
            "cat {}/private_diary.txt 2>&1; echo '---'; cat {}/a.txt",
            fake_home.path().display(),
            spec.project_root.display(),
        ));
        let out = StdCommand::new("bwrap").args(&argv).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);

        assert!(
            !stdout.contains("top secret sibling"),
            "sibling file under $HOME must not be readable from inside the sandbox, got: {stdout}"
        );
        assert!(
            stdout.contains("hello"),
            "the project's own file must still be readable, got: {stdout}"
        );

        let _ = destroy_session(&spec);
    }

    #[test]
    fn bwrap_argv_unshares_net_when_disabled() {
        let dir = fake_project();
        let spec = create_session(dir.path(), false).unwrap();
        let argv = bwrap_argv(&spec);
        assert!(argv.iter().any(|a| a == "--unshare-net"));
        let _ = destroy_session(&spec);
    }

    #[test]
    fn create_session_skips_allowlisted_and_protected_dirs() {
        let dir = fake_project();
        let spec = create_session(dir.path(), true).unwrap();
        assert!(!spec.work_dir.join("target").exists());
        assert!(!spec.work_dir.join(".git").exists());
        assert_eq!(std::fs::read_to_string(spec.work_dir.join("a.txt")).unwrap(), "hello");
        let _ = destroy_session(&spec);
    }

    #[test]
    fn bwrap_actually_runs_and_sees_the_work_copy() {
        if !bwrap_available() {
            eprintln!("skipping: bwrap not on PATH");
            return;
        }
        let dir = fake_project();
        let spec = create_session(dir.path(), true).unwrap();
        let mut argv = bwrap_argv(&spec);
        argv.push("cat".into());
        argv.push(spec.project_root.join("a.txt").to_string_lossy().to_string());
        let out = StdCommand::new("bwrap").args(&argv).output().unwrap();
        assert!(
            out.status.success(),
            "bwrap failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
        let _ = destroy_session(&spec);
    }

    #[test]
    fn load_session_roundtrips_create_session() {
        let dir = fake_project();
        let spec = create_session(dir.path(), true).unwrap();
        let loaded = load_session(&spec.project_root, &spec.id).unwrap();
        assert_eq!(loaded.id, spec.id);
        assert_eq!(loaded.work_dir, spec.work_dir);
        let _ = destroy_session(&spec);
    }
}
