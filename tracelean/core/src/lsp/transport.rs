//! LSP framing and dispatch: JSON-RPC 2.0 over `Content-Length`-delimited
//! stdio.
//!
//! Written directly against the protocol rather than on a framework, because
//! the surface needed here is small and because owning the dispatch loop is
//! what makes "notifications are handled in order" a property of this code
//! rather than something to verify in someone else's.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum LspError {
    Spawn(String),
    Timeout(String),
    Died(String),
    /// The server answered with a JSON-RPC error.
    Server { code: i64, message: String },
    Protocol(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::Spawn(m) => write!(f, "could not start the language server: {m}"),
            LspError::Timeout(m) => write!(f, "the language server did not answer `{m}` in time"),
            LspError::Died(m) => write!(f, "the language server stopped: {m}"),
            LspError::Server { code, message } => write!(f, "language server error {code}: {message}"),
            LspError::Protocol(m) => write!(f, "language server protocol: {m}"),
        }
    }
}

/// One message off the wire.
#[derive(Debug, Clone)]
enum Incoming {
    Response { id: i64, result: Result<Value, LspError> },
    /// A notification from the server — diagnostics, progress, logs.
    Notification { method: String, params: Value },
    /// A request *from* the server. These must be answered or some servers
    /// stall waiting, so they are surfaced rather than dropped.
    Request { id: Value, method: String, params: Value },
}

/// A running language server.
pub struct LspClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicI64,
    /// Responses, keyed by request id, filled by the reader thread.
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, LspError>>>>>,
    /// Notifications, drained in arrival order by whoever cares.
    notifications: Arc<Mutex<Vec<(String, Value)>>>,
    stderr: Arc<Mutex<String>>,
    pub server_capabilities: Value,
}

impl LspClient {
    /// Spawn a server and complete the initialize handshake.
    pub fn start(
        command: &[String],
        root: &std::path::Path,
        initialization_options: Option<Value>,
        timeout: Duration,
    ) -> Result<LspClient, LspError> {
        let Some((program, args)) = command.split_first() else {
            return Err(LspError::Spawn("empty command".into()));
        };

        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                LspError::Spawn(format!(
                    "{program}: {e}. Install it and make sure it is on PATH."
                ))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| LspError::Spawn("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| LspError::Spawn("no stdout".into()))?;

        let pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, LspError>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(String::new()));

        if let Some(mut handle) = child.stderr.take() {
            let sink = stderr.clone();
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = handle.read_to_string(&mut buf);
                if let Ok(mut guard) = sink.lock() {
                    guard.push_str(&buf);
                }
            });
        }

        let stdin = Arc::new(Mutex::new(stdin));
        spawn_reader(stdout, pending.clone(), notifications.clone(), stdin.clone());

        let mut client = LspClient {
            child,
            stdin,
            next_id: AtomicI64::new(1),
            pending,
            notifications,
            stderr,
            server_capabilities: Value::Null,
        };

        let root_uri = path_to_uri(root);
        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
            "capabilities": client_capabilities(),
        });
        if let Some(options) = initialization_options {
            params["initializationOptions"] = options;
        }

        let result = client.request("initialize", params, timeout)?;
        client.server_capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        client.notify("initialized", json!({}))?;

        Ok(client)
    }

    /// Send a request and wait for its response.
    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|e| LspError::Protocol(e.to_string()))?
            .insert(id, tx);

        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        match rx.recv_timeout(timeout) {
            // A death reported by the reader thread says only that stdout
            // closed; the *reason* is on stderr, and attaching it here is the
            // difference between "the language server stopped" and a message
            // someone can act on.
            Ok(Err(LspError::Died(reason))) => Err(LspError::Died(self.death_note(&reason))),
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().ok().and_then(|mut p| p.remove(&id));
                Err(LspError::Timeout(method.to_string()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(LspError::Died(self.death_note("stdout closed")))
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&self, message: &Value) -> Result<(), LspError> {
        let body = serde_json::to_string(message)
            .map_err(|e| LspError::Protocol(format!("encoding: {e}")))?;
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|e| LspError::Protocol(e.to_string()))?;
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .and_then(|_| stdin.flush())
            .map_err(|e| LspError::Died(format!("{e}: {}", self.stderr_text())))
    }

    /// Take everything the server has notified us about since the last drain,
    /// in arrival order.
    pub fn drain_notifications(&self) -> Vec<(String, Value)> {
        self.notifications
            .lock()
            .map(|mut n| std::mem::take(&mut *n))
            .unwrap_or_default()
    }

    pub fn stderr_text(&self) -> String {
        self.stderr.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// How long to wait for the stderr reader before concluding a dying server
    /// had nothing to say. Bounded, so a server that keeps the pipe open cannot
    /// hang the caller.
    const STDERR_GRACE: Duration = Duration::from_millis(400);

    /// A death reason with whatever the server printed attached.
    ///
    /// Reading stderr the instant stdout closes is a race the busy machine
    /// wins: the process has died and its reader thread has not been scheduled,
    /// so the reason arrives empty exactly when it is most needed. Wait briefly
    /// for EOF first.
    fn death_note(&self, reason: &str) -> String {
        let deadline = std::time::Instant::now() + Self::STDERR_GRACE;
        loop {
            let text = self.stderr_text();
            if !text.trim().is_empty() {
                return format!("{reason}: {}", text.trim());
            }
            if std::time::Instant::now() >= deadline {
                return reason.to_string();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.request("shutdown", Value::Null, Duration::from_secs(2));
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Read frames and dispatch them.
///
/// Notifications are appended in arrival order and never reordered: a client
/// that handles them out of order reports diagnostics for a version of the
/// file that no longer exists.
fn spawn_reader(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, LspError>>>>>,
    notifications: Arc<Mutex<Vec<(String, Value)>>>,
    stdin: Arc<Mutex<ChildStdin>>,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let Some(body) = read_frame(&mut reader) else { break };
            let Ok(message) = serde_json::from_str::<Value>(&body) else { continue };

            match classify(&message) {
                Some(Incoming::Response { id, result }) => {
                    if let Ok(mut map) = pending.lock() {
                        if let Some(tx) = map.remove(&id) {
                            let _ = tx.send(result);
                        }
                    }
                }
                Some(Incoming::Notification { method, params }) => {
                    if let Ok(mut list) = notifications.lock() {
                        list.push((method, params));
                    }
                }
                Some(Incoming::Request { id, method, params }) => {
                    // Answering is not optional: a server that asks for its
                    // configuration and is ignored can stall indefinitely.
                    let result = answer_server_request(&method, &params);
                    let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                    if let Ok(body) = serde_json::to_string(&reply) {
                        if let Ok(mut handle) = stdin.lock() {
                            let _ = write!(handle, "Content-Length: {}\r\n\r\n{}", body.len(), body);
                            let _ = handle.flush();
                        }
                    }
                }
                None => {}
            }
        }

        // The server is gone; unblock everyone still waiting rather than
        // letting them sit until their timeout.
        if let Ok(mut map) = pending.lock() {
            for (_, tx) in map.drain() {
                let _ = tx.send(Err(LspError::Died("stdout closed".into())));
            }
        }
    });
}

fn classify(message: &Value) -> Option<Incoming> {
    let method = message.get("method").and_then(|m| m.as_str());
    let id = message.get("id");

    match (method, id) {
        (Some(method), Some(id)) => Some(Incoming::Request {
            id: id.clone(),
            method: method.to_string(),
            params: message.get("params").cloned().unwrap_or(Value::Null),
        }),
        (Some(method), None) => Some(Incoming::Notification {
            method: method.to_string(),
            params: message.get("params").cloned().unwrap_or(Value::Null),
        }),
        (None, Some(id)) => {
            let id = id.as_i64()?;
            let result = match message.get("error") {
                Some(error) => Err(LspError::Server {
                    code: error.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                    message: error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Incoming::Response { id, result })
        }
        (None, None) => None,
    }
}

/// Minimal answers to the requests servers actually make during startup.
fn answer_server_request(method: &str, params: &Value) -> Value {
    match method {
        // One null per requested section: "no configuration, use your defaults".
        "workspace/configuration" => {
            let count = params
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| a.len())
                .unwrap_or(1);
            Value::Array(vec![Value::Null; count])
        }
        // Dynamic registration is accepted and ignored; refusing it makes some
        // servers disable features entirely.
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        "window/workDoneProgress/create" => Value::Null,
        _ => Value::Null,
    }
}

fn read_frame(reader: &mut BufReader<ChildStdout>) -> Option<String> {
    let mut length: Option<usize> = None;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None; // EOF
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(value) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            length = value.trim().parse().ok();
        }
    }

    let length = length?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

/// What this client claims to support. Deliberately modest: claiming a
/// capability that is not implemented makes servers send messages that are
/// then dropped.
fn client_capabilities() -> Value {
    json!({
        "workspace": {
            // Not optional as far as Lean is concerned: its
            // `WorkspaceClientCapabilities` declares `applyEdit` as a plain
            // `Bool`, so omitting it makes `initialize` fail outright with
            // "Bool expected" and the server exits. The LSP specification says
            // this field is optional; Lean's decoder disagrees, and the server
            // that has to start wins the argument.
            //
            // It is also true: `lsp::edits::lower_workspace_edit` turns a
            // server's `WorkspaceEdit` into undoable commands, so we really do
            // apply them.
            "applyEdit": true,
            "configuration": true,
            "workspaceFolders": true,
            "didChangeConfiguration": { "dynamicRegistration": false }
        },
        "textDocument": {
            "synchronization": { "dynamicRegistration": false, "didSave": true },
            "hover": { "contentFormat": ["plaintext", "markdown"] },
            "definition": { "linkSupport": false },
            "references": {},
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": ["quickfix", "refactor", "refactor.extract", "source"]
                    }
                }
            },
            "publishDiagnostics": { "relatedInformation": false }
        },
        "window": { "workDoneProgress": true }
    })
}

/// `file://` URI for a path. Percent-encoding is limited to the characters
/// that actually break URI parsing, which keeps paths readable in logs.
pub fn path_to_uri(path: &std::path::Path) -> String {
    let mut out = String::from("file://");
    for ch in path.to_string_lossy().chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' | '/' | ':' => out.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for byte in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

/// Inverse of `path_to_uri`, for diagnostics that name a file by URI.
///
/// Decoding collects *bytes* and converts once at the end: a percent-escape
/// sequence is one byte of UTF-8, not one character, so decoding per character
/// mangles every non-ASCII path.
pub fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).ok().map(std::path::PathBuf::from)
}

/// A position, in LSP's zero-based line/UTF-16-column terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}
