//! One server per language, started on demand.
//!
//! A missing server is a first-class, explained state rather than a silent
//! absence of features — the difference between "this is broken" and "this is
//! not set up yet", which is the same distinction the Lean toolchain check
//! makes in differential testing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::transport::{path_to_uri, uri_to_path, LspClient, LspError, Position};
use super::{CodeAction, Diagnostic, QueryMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Rust,
    Lean,
    Python,
}

impl ServerKind {
    pub fn for_path(path: &Path) -> Option<ServerKind> {
        match path.extension()?.to_str()? {
            "rs" => Some(ServerKind::Rust),
            "lean" => Some(ServerKind::Lean),
            "py" => Some(ServerKind::Python),
            _ => None,
        }
    }

    pub fn language_id(&self) -> &'static str {
        match self {
            ServerKind::Rust => "rust",
            ServerKind::Lean => "lean4",
            ServerKind::Python => "python",
        }
    }

    /// Candidate commands, in preference order.
    ///
    /// For Rust, `verus-analyzer` is preferred when present: it is a
    /// rust-analyzer fork that also understands Verus, so the same integration
    /// serves both.
    fn candidates(&self, root: &Path) -> Vec<Vec<String>> {
        match self {
            ServerKind::Rust => vec![
                vec!["verus-analyzer".into()],
                vec!["rust-analyzer".into()],
            ],
            ServerKind::Lean => {
                // `lake serve` inside a lake project, so imports resolve;
                // a bare `lean --server` only works for a standalone file.
                if has_lakefile(root) {
                    vec![
                        vec!["lake".into(), "serve".into()],
                        vec!["lean".into(), "--server".into()],
                    ]
                } else {
                    vec![vec!["lean".into(), "--server".into()]]
                }
            }
            ServerKind::Python => vec![
                vec!["pyright-langserver".into(), "--stdio".into()],
                vec!["ruff".into(), "server".into()],
            ],
        }
    }

    fn install_hint(&self) -> &'static str {
        match self {
            ServerKind::Rust => {
                "Install rust-analyzer (`rustup component add rust-analyzer`), or \
                 verus-analyzer if this project uses Verus."
            }
            ServerKind::Lean => {
                "Install elan (https://github.com/leanprover/elan). If it is already \
                 installed, `lake` is in ~/.elan/bin and elan writes its PATH line into \
                 ~/.profile, which only a login shell reads — TraceLean looks there \
                 directly, so check `~/.elan/bin/lake --version` runs."
            }
            ServerKind::Python => "Install pyright (`npm i -g pyright`) or ruff.",
        }
    }
}

/// Directories a per-user toolchain installs itself into, searched when the
/// program is not on PATH.
fn toolchain_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".elan/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join("node_modules/.bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolve a program name to something spawnable, or say where it was looked for.
///
/// A GUI started from a desktop launcher inherits a PATH with none of the
/// per-user toolchain directories in it: elan writes its PATH line into
/// `~/.profile`, which only a *login* shell reads, and `~/.bashrc` — what a
/// desktop terminal sources — usually has no such line. The binary is installed,
/// the language server still will not start, and "Lean language server is not
/// running" is a diagnosis the user cannot act on. So when PATH does not have
/// it, look where these toolchains actually put themselves.
pub fn resolve_program(program: &str) -> Result<String, String> {
    if program.contains('/') {
        let path = Path::new(program);
        return if is_executable(path) {
            Ok(program.to_string())
        } else {
            Err(format!("{program}: not an executable file"))
        };
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if is_executable(&candidate) {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }

    for dir in toolchain_dirs() {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }

    let searched = toolchain_dirs()
        .iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "{program}: not found on PATH, nor in any of {searched}"
    ))
}

fn has_lakefile(root: &Path) -> bool {
    root.join("lakefile.lean").exists() || root.join("lakefile.toml").exists()
}

/// Whether a language is usable, and why not when it is not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub kind: ServerKind,
    pub running: bool,
    pub command: Option<Vec<String>>,
    /// Present when the server could not be started — shown to the user rather
    /// than swallowed.
    pub problem: Option<String>,
    pub hint: Option<String>,
}

struct Running {
    client: LspClient,
    command: Vec<String>,
    /// Document versions, so `didChange` is monotonic per file as LSP requires.
    versions: HashMap<PathBuf, i32>,
    /// Latest diagnostics per file, kept as notifications arrive.
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
}

/// Which server instance a file belongs to.
///
/// Lean is the reason this is not just `ServerKind`: `lake serve` has to run
/// *inside* the package it is serving or imports do not resolve, and a project
/// may hold several lake packages (this repository's own `example/` keeps its
/// model in `formal/`, one directory down). Keying by kind alone started one
/// server at the project root and then wondered why every `import` failed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ServerSlot {
    kind: ServerKind,
    workspace: PathBuf,
}

pub struct LspRegistry {
    root: PathBuf,
    servers: Mutex<HashMap<ServerSlot, Arc<Mutex<Running>>>>,
    /// Kinds that already failed to start, with the reason — so the failure is
    /// reported once rather than retried on every keystroke.
    failed: Mutex<HashMap<ServerKind, String>>,
    timeout: Duration,
}

impl LspRegistry {
    pub fn new(root: impl Into<PathBuf>) -> LspRegistry {
        LspRegistry {
            root: root.into(),
            servers: Mutex::new(HashMap::new()),
            failed: Mutex::new(HashMap::new()),
            timeout: Duration::from_secs(20),
        }
    }

    pub fn status(&self, kind: ServerKind) -> ServerStatus {
        // Any running instance of this kind answers "is this language usable".
        let running = self.servers.lock().ok().and_then(|servers| {
            servers
                .iter()
                .find(|(slot, _)| slot.kind == kind)
                .map(|(_, server)| server.clone())
        });

        if let Some(server) = running {
            let command = server.lock().ok().map(|s| s.command.clone());
            return ServerStatus {
                kind,
                running: true,
                command,
                problem: None,
                hint: None,
            };
        }

        let problem = self.failed.lock().ok().and_then(|f| f.get(&kind).cloned());
        ServerStatus {
            kind,
            running: false,
            command: None,
            hint: problem.as_ref().map(|_| kind.install_hint().to_string()),
            problem,
        }
    }

    /// The directory a server for `path` should run in.
    ///
    /// For Lean that is the nearest ancestor holding a lakefile, so `lake serve`
    /// starts inside the package and imports resolve. The search stops at the
    /// project root: walking past it would hand the server a directory the user
    /// never opened.
    fn workspace_for(&self, kind: ServerKind, path: &Path) -> PathBuf {
        if kind != ServerKind::Lean {
            return self.root.clone();
        }
        let absolute = self.root.join(path);
        let mut dir = absolute.parent();
        while let Some(current) = dir {
            if !current.starts_with(&self.root) {
                break;
            }
            if has_lakefile(current) {
                return current.to_path_buf();
            }
            if current == self.root {
                break;
            }
            dir = current.parent();
        }
        self.root.clone()
    }

    /// Start a server if it is not already running.
    fn ensure(&self, kind: ServerKind, workspace: &Path) -> Result<Arc<Mutex<Running>>, LspError> {
        let slot = ServerSlot { kind, workspace: workspace.to_path_buf() };
        if let Some(existing) = self.servers.lock().ok().and_then(|s| s.get(&slot).cloned()) {
            return Ok(existing);
        }

        if let Some(problem) = self.failed.lock().ok().and_then(|f| f.get(&kind).cloned()) {
            return Err(LspError::Spawn(problem));
        }

        let mut last: Option<LspError> = None;
        for command in kind.candidates(workspace) {
            // Resolve the program before spawning, so a toolchain installed
            // outside this process's PATH is still found and a missing one is
            // reported as "not found, looked in ..." rather than as an opaque
            // failure to start.
            let command = match resolve_program(&command[0]) {
                Ok(program) => {
                    let mut resolved = command;
                    resolved[0] = program;
                    resolved
                }
                Err(problem) => {
                    last = Some(LspError::Spawn(problem));
                    continue;
                }
            };
            let options = match kind {
                // Lean's server wants to know where the package is. It spawns
                // lake itself, so it needs the resolved path for the same
                // reason this registry does.
                ServerKind::Lean => {
                    let lake = resolve_program("lake").unwrap_or_else(|_| "lake".into());
                    Some(json!({ "lakePath": lake }))
                }
                _ => None,
            };
            match LspClient::start(&command, workspace, options, self.timeout) {
                Ok(client) => {
                    let running = Arc::new(Mutex::new(Running {
                        client,
                        command,
                        versions: HashMap::new(),
                        diagnostics: HashMap::new(),
                    }));
                    if let Ok(mut servers) = self.servers.lock() {
                        servers.insert(slot, running.clone());
                    }
                    return Ok(running);
                }
                Err(e) => last = Some(e),
            }
        }

        let problem = last
            .map(|e| e.to_string())
            .unwrap_or_else(|| "no candidate command".into());
        if let Ok(mut failed) = self.failed.lock() {
            failed.insert(kind, problem.clone());
        }
        Err(LspError::Spawn(problem))
    }

    /// Tell the server about a file's current content.
    pub fn open(&self, path: &Path, content: &str) -> Result<(), LspError> {
        let Some(kind) = ServerKind::for_path(path) else {
            return Err(LspError::Protocol(format!(
                "no language server is configured for {}",
                path.display()
            )));
        };
        let server = self.ensure(kind, &self.workspace_for(kind, path))?;
        let mut running = server.lock().map_err(|e| LspError::Protocol(e.to_string()))?;

        let uri = path_to_uri(&self.root.join(path));
        let version = running.versions.entry(path.to_path_buf()).or_insert(0);
        if *version == 0 {
            *version = 1;
            running.client.notify(
                "textDocument/didOpen",
                json!({ "textDocument": {
                    "uri": uri, "languageId": kind.language_id(),
                    "version": 1, "text": content
                }}),
            )?;
        } else {
            *version += 1;
            let v = *version;
            running.client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": v },
                    // Full sync: the editor already holds whole buffers, and
                    // incremental sync would be a second source of truth.
                    "contentChanges": [{ "text": content }]
                }),
            )?;
        }

        drain_into(&mut running, &self.root);
        Ok(())
    }

    /// Ask the server something at a position.
    pub fn query(
        &self,
        path: &Path,
        position: Position,
        mode: QueryMode,
    ) -> Result<Value, LspError> {
        let Some(kind) = ServerKind::for_path(path) else {
            return Err(LspError::Protocol(format!(
                "no language server is configured for {}",
                path.display()
            )));
        };
        let server = self.ensure(kind, &self.workspace_for(kind, path))?;
        let mut running = server.lock().map_err(|e| LspError::Protocol(e.to_string()))?;

        let uri = path_to_uri(&self.root.join(path));
        let document = json!({ "uri": uri });
        let at = json!({ "textDocument": document, "position": {
            "line": position.line, "character": position.character
        }});

        let result = match mode {
            QueryMode::Hover => running.client.request("textDocument/hover", at, self.timeout)?,
            QueryMode::Definition => {
                running.client.request("textDocument/definition", at, self.timeout)?
            }
            QueryMode::References => running.client.request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": position.line, "character": position.character },
                    "context": { "includeDeclaration": true }
                }),
                self.timeout,
            )?,
            QueryMode::Symbols => running.client.request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
                self.timeout,
            )?,
            QueryMode::CodeActions => running.client.request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": position.line, "character": position.character },
                        "end": { "line": position.line, "character": position.character }
                    },
                    "context": { "diagnostics": [] }
                }),
                self.timeout,
            )?,
            QueryMode::Goal => {
                if kind != ServerKind::Lean {
                    return Err(LspError::Protocol(
                        "goal state is a Lean-only question".into(),
                    ));
                }
                // Lean's own extension, not part of core LSP.
                running.client.request("$/lean/plainGoal", at, self.timeout)?
            }
            QueryMode::TermGoal => {
                if kind != ServerKind::Lean {
                    return Err(LspError::Protocol(
                        "term goals are a Lean-only question".into(),
                    ));
                }
                running.client.request("$/lean/plainTermGoal", at, self.timeout)?
            }
            QueryMode::Diagnostics => {
                // Diagnostics arrive as notifications from elaboration, not as
                // a response. Asking the server to confirm it has finished is
                // what stops "nothing yet" reading as "this file is fine".
                if kind == ServerKind::Lean {
                    let _ = running.client.request(
                        "textDocument/waitForDiagnostics",
                        json!({ "uri": uri, "version": running.versions.get(path).copied().unwrap_or(1) }),
                        self.timeout,
                    );
                }
                drain_into(&mut running, &self.root);
                let found = running
                    .diagnostics
                    .get(path)
                    .cloned()
                    .unwrap_or_default();
                serde_json::to_value(found).unwrap_or(Value::Null)
            }
        };

        drain_into(&mut running, &self.root);
        Ok(result)
    }

    /// Code actions at a position, flattened into the shape Myth binds to.
    pub fn code_actions(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<CodeAction>, LspError> {
        let raw = self.query(path, position, QueryMode::CodeActions)?;
        let Some(items) = raw.as_array() else { return Ok(Vec::new()) };

        Ok(items
            .iter()
            .filter_map(|item| {
                let title = item.get("title")?.as_str()?.to_string();
                Some(CodeAction {
                    title,
                    kind: item.get("kind").and_then(|k| k.as_str()).map(|s| s.to_string()),
                    edit: item.get("edit").cloned(),
                    command: item.get("command").cloned(),
                })
            })
            .collect())
    }

    pub fn shutdown_all(&self) {
        if let Ok(mut servers) = self.servers.lock() {
            for (_, server) in servers.drain() {
                if let Ok(mut running) = server.lock() {
                    running.client.shutdown();
                }
            }
        }
    }
}

/// Fold pending notifications into per-file diagnostics.
fn drain_into(running: &mut Running, root: &Path) {
    for (method, params) in running.client.drain_notifications() {
        if method != "textDocument/publishDiagnostics" {
            continue;
        }
        let Some(uri) = params.get("uri").and_then(|u| u.as_str()) else { continue };
        let Some(path) = uri_to_path(uri) else { continue };
        let list = params
            .get("diagnostics")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        let parsed: Vec<Diagnostic> = list
            .iter()
            .filter_map(|d| {
                let start = d.get("range")?.get("start")?;
                Some(Diagnostic {
                    file: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                    line: start.get("line")?.as_u64()? as u32,
                    character: start.get("character")?.as_u64().unwrap_or(0) as u32,
                    severity: match d.get("severity").and_then(|s| s.as_u64()) {
                        Some(1) => "error",
                        Some(2) => "warning",
                        Some(3) => "information",
                        Some(4) => "hint",
                        _ => "error",
                    }
                    .into(),
                    message: d.get("message")?.as_str()?.to_string(),
                    source: d.get("source").and_then(|s| s.as_str()).map(|s| s.into()),
                })
            })
            .collect();

        // Servers report absolute URIs; the rest of the system speaks paths
        // relative to the project root, so normalize here rather than at every
        // lookup.
        let key = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        running.diagnostics.insert(key, parsed);
    }
}
