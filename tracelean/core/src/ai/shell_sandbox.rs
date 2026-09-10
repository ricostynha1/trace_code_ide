//! Agent shell sandboxing v2 (sandboxing_better.md).
//!
//! One mechanism does containment *and* diffing at once: the project is bound
//! into a bubblewrap mount namespace through an overlayfs whose upper dir is
//! persisted after the command exits. The real project (`lower`) is physically
//! untouched during the run — it *is* the pre-image — and walking the upper
//! dir once yields every create/modify/delete the command performed, which the
//! caller feeds back through the normal Command/undo machinery (R6).
//!
//! Backends, in preference order:
//! - **Overlay** (`bwrap --overlay-src`): full containment, exact diff.
//! - **Strace** fallback: no containment (writes hit the real project), but
//!   mutated paths are recovered from the syscall trace so they still become
//!   visible/undoable events where a pre-image is known.
//! - **None**: unsandboxed with a visible notice (mode `detect` only).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// Build/cache dirs bound straight through the overlay (writable, not diffed —
/// R3). Regenerable by design; trashing them is out of scope for undo.
pub const PROJECT_ALLOWLIST: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".tracelean/tmp",
];

/// Home-relative cache dirs bound writable so toolchains keep working under
/// the read-only root (package registries, compiler caches).
pub(crate) const HOME_ALLOWLIST: &[&str] = &[".cargo", ".rustup", ".cache", ".npm"];

/// Paths protected even where the project is writable (R4). Upper-dir entries
/// under these are discarded, never replayed onto the real tree.
pub(crate) const PROTECTED: &[&str] = &[".git", ".tracelean"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    Off,
    Detect,
    Strict,
}

impl SandboxMode {
    pub fn from_setting(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => SandboxMode::Off,
            "strict" => SandboxMode::Strict,
            _ => SandboxMode::Detect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    Deny,
    Ask,
    Allow,
}

impl NetworkPolicy {
    pub fn from_setting(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "allow" => NetworkPolicy::Allow,
            "deny" => NetworkPolicy::Deny,
            _ => NetworkPolicy::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    Created,
    Modified,
    Deleted,
}

/// One net file mutation performed by a shell command, project-relative.
#[derive(Debug, Clone)]
pub struct FsMutation {
    pub path: PathBuf,
    pub kind: MutationKind,
    /// Content before the command (None = file did not exist).
    pub pre: Option<String>,
    /// Content after the command (None = deleted).
    pub post: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Overlay ran the command against a copy-on-write view: the real tree is
    /// untouched and every mutation must be materialized by the caller.
    Overlay,
    /// Strace observed a command that wrote the real tree directly: mutations
    /// are already on disk; the caller only records them (buffers + undo).
    Strace,
    /// No sandbox — command ran directly against the real tree.
    None,
}

#[derive(Debug)]
pub struct SandboxRun {
    pub backend: Backend,
    pub success: bool,
    pub output: String,
    pub killed_reason: Option<String>,
    pub mutations: Vec<FsMutation>,
    /// Attempted writes to protected paths (.git, .tracelean) — discarded.
    pub blocked: Vec<String>,
    /// Changed paths that could not be captured as text mutations
    /// (binary/too large/pre-image unknown) — visible but not undoable.
    pub skipped: Vec<String>,
}

/// Files above this size are reported but not turned into undo commands.
pub(crate) const MAX_CAPTURE_BYTES: u64 = 4 * 1024 * 1024;

// ─── Availability probes ─────────────────────────────────────────────────────

/// Whether `bwrap` with `--overlay-src` support and a kernel that allows
/// unprivileged overlay-in-userns are present. Probed once with a real
/// micro-overlay mount, not just a --help grep.
pub fn overlay_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        if !cfg!(target_os = "linux") {
            return false;
        }
        let probe = || -> Option<bool> {
            let base = std::env::temp_dir().join(format!("tracelean-sbprobe-{}", uuid::Uuid::new_v4()));
            let src = base.join("src");
            let upper = base.join("upper");
            let work = base.join("work");
            std::fs::create_dir_all(&src).ok()?;
            std::fs::create_dir_all(&upper).ok()?;
            std::fs::create_dir_all(&work).ok()?;
            let status = Command::new("bwrap")
                .arg("--ro-bind").arg("/").arg("/")
                .arg("--overlay-src").arg(&src)
                .arg("--overlay").arg(&upper).arg(&work).arg(&src)
                .arg("--unshare-net")
                .arg("--die-with-parent")
                .arg("true")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok()?;
            let _ = std::fs::remove_dir_all(&base);
            Some(status.success())
        };
        probe().unwrap_or(false)
    })
}

pub fn strace_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        Command::new("strace")
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

// ─── T5: static command screening (cosmetic approval-UI annotations) ─────────

/// Heuristic warnings shown in the approval UI. Purely informational — nothing
/// here blocks execution.
pub fn screen_command(cmd: &str) -> Vec<String> {
    let mut warns = Vec::new();
    let lower = cmd.to_lowercase();
    let has = |pat: &str| lower.contains(pat);

    if has("rm -rf") || has("rm -fr") || has("rm -r") {
        warns.push("recursively deletes files".to_string());
    }
    if lower.split_whitespace().any(|w| w == "sudo" || w == "doas") {
        warns.push("requests elevated privileges".to_string());
    }
    if (has("curl") || has("wget")) && (has("| sh") || has("| bash") || has("|sh") || has("|bash")) {
        warns.push("pipes downloaded content into a shell".to_string());
    }
    if has("git push") {
        warns.push("pushes to a remote git repository".to_string());
    }
    if has("chmod -r") || has("chown -r") {
        warns.push("recursively changes file permissions/ownership".to_string());
    }
    if has("mkfs") || has("dd if=") || lower.contains("> /dev/") {
        warns.push("raw device operation".to_string());
    }
    if likely_needs_network(cmd) {
        warns.push("likely needs network access (package manager / fetch)".to_string());
    }
    warns
}

/// Package-manager / fetch pre-check (T4): commands that typically fail
/// without network, so the approval UI can suggest enabling it.
pub fn likely_needs_network(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    let has = |pat: &str| lower.contains(pat);
    has("npm install") || has("npm ci") || has("npm update")
        || has("pnpm install") || has("pnpm add")
        || has("yarn add") || has("yarn install")
        || has("pip install") || has("pip3 install") || has("uv pip") || has("uv add")
        || has("cargo add") || has("cargo install") || has("cargo update") || has("cargo fetch")
        || has("go get") || has("go mod download")
        || has("apt install") || has("apt-get install") || has("dnf install") || has("brew install")
        || has("git clone") || has("git fetch") || has("git pull") || has("git push")
        || has("curl ") || has("wget ")
}

// ─── Shared child-process runner (timeout + cancel) ──────────────────────────

pub struct ProcessOutcome {
    pub status_success: Option<bool>,
    pub output: String,
    pub killed_reason: Option<String>,
}

/// Spawn `cmd`, drain pipes on threads, poll for exit while honoring the
/// timeout and the hard-stop flag. Mirrors the semantics run_shell always had.
pub fn run_process(
    cmd: &mut Command,
    timeout_secs: u64,
    cancel_flag: Option<&Arc<AtomicBool>>,
) -> Result<ProcessOutcome, String> {
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Err(format!("spawn failed: {}", e)),
    };

    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        pipe.map(|mut p| {
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = Vec::new();
                let _ = p.read_to_end(&mut buf);
                buf
            })
        })
    };
    let stdout_reader = drain(child.stdout.take().map(|s| Box::new(s) as _));
    let stderr_reader = drain(child.stderr.take().map(|s| Box::new(s) as _));

    let started = std::time::Instant::now();
    let mut killed_reason: Option<String> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                let cancelled = cancel_flag
                    .map(|f| f.load(Ordering::SeqCst))
                    .unwrap_or(false);
                if cancelled {
                    killed_reason = Some("stopped by user".into());
                } else if started.elapsed().as_secs() >= timeout_secs {
                    killed_reason = Some(format!("timed out after {}s", timeout_secs));
                }
                if killed_reason.is_some() {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("wait failed: {}", e));
            }
        }
    };

    let collect = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
        h.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    let stdout = String::from_utf8_lossy(&collect(stdout_reader)).to_string();
    let stderr = String::from_utf8_lossy(&collect(stderr_reader)).to_string();
    let combined = if stderr.is_empty() {
        stdout
    } else {
        format!("{}\n--- stderr ---\n{}", stdout, stderr)
    };

    Ok(ProcessOutcome {
        status_success: status.map(|s| s.success()),
        output: combined,
        killed_reason,
    })
}

// ─── Overlay backend (T1 + T2) ───────────────────────────────────────────────

/// Run `command` inside a bwrap+overlay sandbox rooted at `project_root`.
/// The real project is read-only for the whole run; the returned mutations
/// are the net effect captured in the overlay upper dir and must be
/// materialized by the caller through the Command pipeline.
pub fn run_overlay(
    command: &str,
    project_root: &Path,
    timeout_secs: u64,
    cancel_flag: Option<&Arc<AtomicBool>>,
    allow_network: bool,
) -> Result<SandboxRun, String> {
    let project = project_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize project root: {}", e))?;

    let sb_base = std::env::temp_dir().join(format!("tracelean-sb-{}", uuid::Uuid::new_v4()));
    let upper = sb_base.join("upper");
    let work = sb_base.join("work");
    std::fs::create_dir_all(&upper).map_err(|e| format!("cannot create upper dir: {}", e))?;
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create work dir: {}", e))?;

    // .tracelean/tmp must exist to be bind-able (R3/R4: tmp is writable,
    // the rest of .tracelean is protected).
    let _ = std::fs::create_dir_all(project.join(".tracelean/tmp"));

    let mut cmd = Command::new("bwrap");
    cmd.arg("--ro-bind").arg("/").arg("/")
        .arg("--dev").arg("/dev")
        .arg("--proc").arg("/proc")
        .arg("--overlay-src").arg(&project)
        .arg("--overlay").arg(&upper).arg(&work).arg(&project);

    // Allowlisted build dirs bound straight through — writable, not diffed.
    for rel in PROJECT_ALLOWLIST {
        let dir = project.join(rel);
        if dir.is_dir() {
            cmd.arg("--bind").arg(&dir).arg(&dir);
        }
    }
    // Toolchain caches in $HOME.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in HOME_ALLOWLIST {
            let dir = home.join(rel);
            if dir.is_dir() {
                cmd.arg("--bind").arg(&dir).arg(&dir);
            }
        }
    }
    // Fresh scratch space. The overlay upper/work live in the REAL temp dir
    // but the kernel overlay mount holds their inodes, so a tmpfs at /tmp
    // doesn't disturb persistence — UNLESS the project itself lives under
    // /tmp, in which case the tmpfs would shadow the project we just bound.
    let tmp_root = std::env::temp_dir();
    if !project.starts_with(&tmp_root) {
        cmd.arg("--tmpfs").arg("/tmp");
    }

    if !allow_network {
        cmd.arg("--unshare-net");
    }
    cmd.arg("--die-with-parent")
        .arg("--chdir").arg(&project)
        .arg("sh").arg("-c").arg(command);

    let outcome = run_process(&mut cmd, timeout_secs, cancel_flag)?;

    // Walk the upper dir once: it *is* the diff.
    let mut run = SandboxRun {
        backend: Backend::Overlay,
        success: outcome.status_success.unwrap_or(false),
        output: outcome.output,
        killed_reason: outcome.killed_reason,
        mutations: Vec::new(),
        blocked: Vec::new(),
        skipped: Vec::new(),
    };
    collect_upper_mutations(&upper, &project, PathBuf::new(), &mut run);
    // Deterministic order for reports and Batch commands.
    run.mutations.sort_by(|a, b| a.path.cmp(&b.path));

    let _ = std::fs::remove_dir_all(&sb_base);
    Ok(run)
}

pub(crate) fn is_protected(rel: &Path) -> bool {
    PROTECTED.iter().any(|p| rel.starts_with(p))
}

pub(crate) fn is_allowlisted(rel: &Path) -> bool {
    PROJECT_ALLOWLIST.iter().any(|p| rel.starts_with(p))
}

/// Read a file as text for mutation capture. Ok(None) = missing.
/// Err(reason) = exists but not capturable (binary / too large / io error).
pub(crate) fn read_capture(path: &Path) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok(None),
        Ok(md) => {
            if !md.is_file() {
                return Err("not a regular file".into());
            }
            if md.len() > MAX_CAPTURE_BYTES {
                return Err(format!("too large ({} bytes)", md.len()));
            }
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            String::from_utf8(bytes).map(Some).map_err(|_| "binary content".into())
        }
    }
}

#[cfg(target_os = "linux")]
fn is_whiteout(md: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::fs::MetadataExt;
    md.file_type().is_char_device() && md.rdev() == 0
}

#[cfg(not(target_os = "linux"))]
fn is_whiteout(_md: &std::fs::Metadata) -> bool {
    false
}

/// Overlayfs marks a replaced directory with an opaque xattr; entries of the
/// lower dir not re-created in upper are then deleted (`rm -rf d && mkdir d`).
#[cfg(target_os = "linux")]
fn is_opaque_dir(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(cpath) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // Unprivileged (userns) overlay mounts use the `user.` prefix; privileged
    // ones use `trusted.`. Check both.
    for name in ["user.overlay.opaque\0", "trusted.overlay.opaque\0"] {
        let mut buf = [0u8; 4];
        let r = unsafe {
            libc::lgetxattr(
                cpath.as_ptr(),
                name.as_ptr() as *const libc::c_char,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if r > 0 && buf[0] == b'y' {
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "linux"))]
fn is_opaque_dir(_path: &Path) -> bool {
    false
}

/// Record every file under `lower_dir` (recursively) as deleted.
fn record_lower_tree_deleted(lower_dir: &Path, rel: &Path, run: &mut SandboxRun) {
    let Ok(entries) = std::fs::read_dir(lower_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let child_rel = rel.join(&name);
        if is_protected(&child_rel) || is_allowlisted(&child_rel) {
            continue;
        }
        let child_path = entry.path();
        if child_path.is_dir() {
            record_lower_tree_deleted(&child_path, &child_rel, run);
        } else {
            match read_capture(&child_path) {
                Ok(Some(pre)) => run.mutations.push(FsMutation {
                    path: child_rel,
                    kind: MutationKind::Deleted,
                    pre: Some(pre),
                    post: None,
                }),
                Ok(None) => {}
                Err(why) => run
                    .skipped
                    .push(format!("{} (deleted; {})", child_rel.display(), why)),
            }
        }
    }
}

/// T2: classify every upper-dir entry against the lower (real) tree.
fn collect_upper_mutations(upper_dir: &Path, lower_dir: &Path, rel: PathBuf, run: &mut SandboxRun) {
    let Ok(entries) = std::fs::read_dir(upper_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let child_rel = rel.join(&name);
        // Protected trees never propagate — the overlay absorbed the write and
        // the real .git/.tracelean were never touched (R4).
        if is_protected(&child_rel) {
            run.blocked.push(child_rel.display().to_string());
            continue;
        }
        // Allowlisted dirs are bound through, so they shouldn't appear here;
        // if one does (created fresh by the command), ignore it (R3).
        if is_allowlisted(&child_rel) {
            continue;
        }
        let upper_path = entry.path();
        let lower_path = lower_dir.join(&name);
        let Ok(md) = std::fs::symlink_metadata(&upper_path) else { continue };

        if is_whiteout(&md) {
            // Deleted file or directory.
            if lower_path.is_dir() {
                record_lower_tree_deleted(&lower_path, &child_rel, run);
            } else {
                match read_capture(&lower_path) {
                    Ok(Some(pre)) => run.mutations.push(FsMutation {
                        path: child_rel,
                        kind: MutationKind::Deleted,
                        pre: Some(pre),
                        post: None,
                    }),
                    Ok(None) => {} // whiteout over nothing — no net effect
                    Err(why) => run
                        .skipped
                        .push(format!("{} (deleted; {})", child_rel.display(), why)),
                }
            }
        } else if md.is_dir() {
            if is_opaque_dir(&upper_path) {
                // Whole dir replaced: lower entries missing from upper are gone.
                let upper_names: std::collections::HashSet<std::ffi::OsString> =
                    std::fs::read_dir(&upper_path)
                        .map(|it| it.flatten().map(|e| e.file_name()).collect())
                        .unwrap_or_default();
                if let Ok(lower_entries) = std::fs::read_dir(&lower_path) {
                    for le in lower_entries.flatten() {
                        if !upper_names.contains(&le.file_name()) {
                            let gone_rel = child_rel.join(le.file_name());
                            if is_protected(&gone_rel) || is_allowlisted(&gone_rel) {
                                continue;
                            }
                            let gone_path = le.path();
                            if gone_path.is_dir() {
                                record_lower_tree_deleted(&gone_path, &gone_rel, run);
                            } else {
                                match read_capture(&gone_path) {
                                    Ok(Some(pre)) => run.mutations.push(FsMutation {
                                        path: gone_rel,
                                        kind: MutationKind::Deleted,
                                        pre: Some(pre),
                                        post: None,
                                    }),
                                    Ok(None) => {}
                                    Err(why) => run.skipped.push(format!(
                                        "{} (deleted; {})",
                                        gone_rel.display(),
                                        why
                                    )),
                                }
                            }
                        }
                    }
                }
            }
            collect_upper_mutations(&upper_path, &lower_path, child_rel, run);
        } else if md.is_file() {
            let post = match read_capture(&upper_path) {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(why) => {
                    run.skipped
                        .push(format!("{} (changed; {})", child_rel.display(), why));
                    continue;
                }
            };
            match read_capture(&lower_path) {
                Ok(Some(pre)) => {
                    if pre != post {
                        run.mutations.push(FsMutation {
                            path: child_rel,
                            kind: MutationKind::Modified,
                            pre: Some(pre),
                            post: Some(post),
                        });
                    }
                }
                Ok(None) => run.mutations.push(FsMutation {
                    path: child_rel,
                    kind: MutationKind::Created,
                    pre: None,
                    post: Some(post),
                }),
                Err(why) => run
                    .skipped
                    .push(format!("{} (changed; pre-image {})", child_rel.display(), why)),
            }
        } else {
            run.skipped
                .push(format!("{} (unsupported file type)", child_rel.display()));
        }
    }
}

// ─── Strace fallback backend (T3) ────────────────────────────────────────────

/// Run `command` under `strace` file-syscall tracing. The command writes the
/// REAL tree (no containment); the trace recovers which project files were
/// mutated. Pre-images come from `pre_lookup` (the IDE's in-memory buffers,
/// which still hold pre-run content because the shell only touched disk) —
/// files unknown to it are reported but not undoable.
pub fn run_strace(
    command: &str,
    project_root: &Path,
    timeout_secs: u64,
    cancel_flag: Option<&Arc<AtomicBool>>,
    pre_lookup: &dyn Fn(&Path) -> Option<String>,
) -> Result<SandboxRun, String> {
    let project = project_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize project root: {}", e))?;
    let trace_file =
        std::env::temp_dir().join(format!("tracelean-strace-{}.log", uuid::Uuid::new_v4()));

    let mut cmd = Command::new("strace");
    cmd.arg("-f")
        .arg("-qq")
        .arg("-e")
        .arg("trace=open,openat,creat,unlink,unlinkat,rename,renameat,renameat2")
        .arg("-o")
        .arg(&trace_file)
        .arg("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&project);

    let outcome = run_process(&mut cmd, timeout_secs, cancel_flag)?;

    let trace = std::fs::read_to_string(&trace_file).unwrap_or_default();
    let _ = std::fs::remove_file(&trace_file);

    let mut run = SandboxRun {
        backend: Backend::Strace,
        success: outcome.status_success.unwrap_or(false),
        output: outcome.output,
        killed_reason: outcome.killed_reason,
        mutations: Vec::new(),
        blocked: Vec::new(),
        skipped: Vec::new(),
    };

    // path → was it ever unlinked last?
    #[derive(Clone, Copy, PartialEq)]
    enum Touch {
        Written,
        Unlinked,
    }
    let mut touched: BTreeMap<PathBuf, Touch> = BTreeMap::new();

    let resolve = |raw: &str| -> Option<PathBuf> {
        let p = PathBuf::from(raw);
        let abs = if p.is_absolute() { p } else { project.join(p) };
        // Normalize without resolving symlinks (the file may be gone).
        let rel = abs.strip_prefix(&project).ok()?.to_path_buf();
        if is_protected(&rel) || is_allowlisted(&rel) {
            return None;
        }
        Some(rel)
    };

    for line in trace.lines() {
        if line.contains("= -1") {
            continue; // failed syscall
        }
        if let Some((name, args)) = parse_syscall(line) {
            match name {
                "open" | "openat" | "creat" => {
                    let write = line.contains("O_WRONLY")
                        || line.contains("O_RDWR")
                        || line.contains("O_CREAT")
                        || line.contains("O_TRUNC")
                        || line.contains("O_APPEND")
                        || name == "creat";
                    if write {
                        if let Some(path) = args.first().and_then(|r| resolve(r)) {
                            touched.insert(path, Touch::Written);
                        }
                    }
                }
                "unlink" | "unlinkat" => {
                    if let Some(path) = args.first().and_then(|r| resolve(r)) {
                        touched.insert(path, Touch::Unlinked);
                    }
                }
                "rename" | "renameat" | "renameat2" => {
                    let mut it = args.iter();
                    if let Some(from) = it.next().and_then(|r| resolve(r)) {
                        touched.insert(from, Touch::Unlinked);
                    }
                    if let Some(to) = it.next().and_then(|r| resolve(r)) {
                        touched.insert(to, Touch::Written);
                    }
                }
                _ => {}
            }
        }
    }

    for (rel, touch) in touched {
        let disk_path = project.join(&rel);
        let pre = pre_lookup(&rel);
        let exists = disk_path.exists();
        match (touch, exists) {
            (_, true) => {
                let post = match read_capture(&disk_path) {
                    Ok(Some(p)) => p,
                    Ok(None) => continue,
                    Err(why) => {
                        run.skipped.push(format!("{} (changed; {})", rel.display(), why));
                        continue;
                    }
                };
                match pre {
                    Some(pre) if pre != post => run.mutations.push(FsMutation {
                        path: rel,
                        kind: MutationKind::Modified,
                        pre: Some(pre),
                        post: Some(post),
                    }),
                    Some(_) => {} // no net change
                    None => run.skipped.push(format!(
                        "{} (written; pre-image unknown — not undoable)",
                        rel.display()
                    )),
                }
            }
            (Touch::Unlinked, false) => match pre {
                Some(pre) => run.mutations.push(FsMutation {
                    path: rel,
                    kind: MutationKind::Deleted,
                    pre: Some(pre),
                    post: None,
                }),
                None => run.skipped.push(format!(
                    "{} (deleted; pre-image unknown — not undoable)",
                    rel.display()
                )),
            },
            (Touch::Written, false) => {} // written then removed within the run
        }
    }

    Ok(run)
}

/// Parse `openat(AT_FDCWD, "path", O_WRONLY|…) = 3` → ("openat", ["path"]).
/// Returns every quoted string argument, in order.
fn parse_syscall(line: &str) -> Option<(&str, Vec<String>)> {
    // strace -f prefixes a pid; the syscall name starts after optional
    // "12345 " or "[pid 12345] ".
    let mut s = line.trim_start();
    if s.starts_with("[pid") {
        s = s.splitn(2, ']').nth(1)?.trim_start();
    } else {
        let first = s.split_whitespace().next()?;
        if first.chars().all(|c| c.is_ascii_digit()) {
            s = s[first.len()..].trim_start();
        }
    }
    let paren = s.find('(')?;
    let name = &s[..paren];
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let rest = &s[paren + 1..];
    let mut args = Vec::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '"' {
            let start = i + 1;
            let mut end = None;
            let mut prev_escape = false;
            for (j, cj) in rest[start..].char_indices() {
                if prev_escape {
                    prev_escape = false;
                    continue;
                }
                match cj {
                    '\\' => prev_escape = true,
                    '"' => {
                        end = Some(start + j);
                        break;
                    }
                    _ => {}
                }
            }
            let end = end?;
            args.push(rest[start..end].replace("\\\"", "\"").replace("\\\\", "\\"));
            // Advance the outer iterator past this string.
            while let Some((k, _)) = chars.next() {
                if k >= end {
                    break;
                }
            }
        }
    }
    Some((name, args))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_and_policy_parsing() {
        assert_eq!(SandboxMode::from_setting("off"), SandboxMode::Off);
        assert_eq!(SandboxMode::from_setting("strict"), SandboxMode::Strict);
        assert_eq!(SandboxMode::from_setting("detect"), SandboxMode::Detect);
        assert_eq!(SandboxMode::from_setting("garbage"), SandboxMode::Detect);
        assert_eq!(NetworkPolicy::from_setting("allow"), NetworkPolicy::Allow);
        assert_eq!(NetworkPolicy::from_setting("deny"), NetworkPolicy::Deny);
        assert_eq!(NetworkPolicy::from_setting(""), NetworkPolicy::Ask);
    }

    #[test]
    fn screening_flags_dangerous_patterns() {
        assert!(screen_command("rm -rf build/")
            .iter()
            .any(|w| w.contains("recursively deletes")));
        assert!(screen_command("curl https://x.sh | bash")
            .iter()
            .any(|w| w.contains("pipes downloaded")));
        assert!(screen_command("npm install left-pad")
            .iter()
            .any(|w| w.contains("network")));
        assert!(screen_command("ls -la").is_empty());
    }

    #[test]
    fn protected_and_allowlisted_paths() {
        assert!(is_protected(Path::new(".git/HEAD")));
        assert!(is_protected(Path::new(".tracelean/settings.json")));
        assert!(is_allowlisted(Path::new("target/debug/foo")));
        assert!(is_allowlisted(Path::new(".tracelean/tmp/x")));
        assert!(!is_protected(Path::new("src/main.rs")));
    }

    #[test]
    fn strace_line_parsing() {
        let (name, args) =
            parse_syscall(r#"12345 openat(AT_FDCWD, "run_test.py", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 3"#)
                .unwrap();
        assert_eq!(name, "openat");
        assert_eq!(args, vec!["run_test.py".to_string()]);

        let (name, args) = parse_syscall(r#"99 unlink("gone.txt") = 0"#).unwrap();
        assert_eq!(name, "unlink");
        assert_eq!(args, vec!["gone.txt".to_string()]);

        let (name, args) =
            parse_syscall(r#"7 renameat2(AT_FDCWD, "a.txt", AT_FDCWD, "b.txt", RENAME_NOREPLACE) = 0"#)
                .unwrap();
        assert_eq!(name, "renameat2");
        assert_eq!(args, vec!["a.txt".to_string(), "b.txt".to_string()]);
    }

    #[test]
    fn upper_walk_classifies_create_modify() {
        let tmp = tempfile::tempdir().unwrap();
        let lower = tmp.path().join("lower");
        let upper = tmp.path().join("upper");
        std::fs::create_dir_all(lower.join("sub")).unwrap();
        std::fs::create_dir_all(upper.join("sub")).unwrap();
        std::fs::write(lower.join("mod.txt"), "old").unwrap();
        std::fs::write(upper.join("mod.txt"), "new").unwrap();
        std::fs::write(upper.join("sub/new.txt"), "created").unwrap();
        // Same-content upper copy → no mutation.
        std::fs::write(lower.join("same.txt"), "x").unwrap();
        std::fs::write(upper.join("same.txt"), "x").unwrap();

        let mut run = SandboxRun {
            backend: Backend::Overlay,
            success: true,
            output: String::new(),
            killed_reason: None,
            mutations: Vec::new(),
            blocked: Vec::new(),
            skipped: Vec::new(),
        };
        collect_upper_mutations(&upper, &lower, PathBuf::new(), &mut run);
        run.mutations.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(run.mutations.len(), 2);
        assert_eq!(run.mutations[0].path, PathBuf::from("mod.txt"));
        assert_eq!(run.mutations[0].kind, MutationKind::Modified);
        assert_eq!(run.mutations[0].pre.as_deref(), Some("old"));
        assert_eq!(run.mutations[0].post.as_deref(), Some("new"));
        assert_eq!(run.mutations[1].path, PathBuf::from("sub/new.txt"));
        assert_eq!(run.mutations[1].kind, MutationKind::Created);
    }

    #[test]
    fn upper_walk_blocks_protected_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let lower = tmp.path().join("lower");
        let upper = tmp.path().join("upper");
        std::fs::create_dir_all(upper.join(".git")).unwrap();
        std::fs::create_dir_all(&lower).unwrap();
        std::fs::write(upper.join(".git/HEAD"), "ref: broken").unwrap();

        let mut run = SandboxRun {
            backend: Backend::Overlay,
            success: true,
            output: String::new(),
            killed_reason: None,
            mutations: Vec::new(),
            blocked: Vec::new(),
            skipped: Vec::new(),
        };
        collect_upper_mutations(&upper, &lower, PathBuf::new(), &mut run);
        assert!(run.mutations.is_empty());
        assert_eq!(run.blocked, vec![".git".to_string()]);
    }

    /// Full end-to-end overlay run — only meaningful where bwrap+overlay work,
    /// so it silently no-ops elsewhere (CI containers often can't nest userns).
    #[test]
    fn overlay_end_to_end_when_available() {
        if !overlay_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("keep.txt"), "before").unwrap();
        std::fs::write(tmp.path().join("gone.txt"), "bye").unwrap();

        let run = run_overlay(
            "echo hi > new.txt && echo after > keep.txt && rm gone.txt",
            tmp.path(),
            30,
            None,
            false,
        )
        .unwrap();

        assert!(run.success);
        // Real tree untouched:
        assert_eq!(std::fs::read_to_string(tmp.path().join("keep.txt")).unwrap(), "before");
        assert!(tmp.path().join("gone.txt").exists());
        assert!(!tmp.path().join("new.txt").exists());
        // …but every mutation captured:
        let kinds: Vec<(String, MutationKind)> = run
            .mutations
            .iter()
            .map(|m| (m.path.display().to_string(), m.kind))
            .collect();
        assert!(kinds.contains(&("new.txt".to_string(), MutationKind::Created)));
        assert!(kinds.contains(&("keep.txt".to_string(), MutationKind::Modified)));
        assert!(kinds.contains(&("gone.txt".to_string(), MutationKind::Deleted)));
    }
}
