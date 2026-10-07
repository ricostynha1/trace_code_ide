//! The language-server client.
//!
//! No real language server is required: a small Python server speaking the
//! protocol stands in, which exercises the framing, id correlation,
//! notification ordering and server-initiated requests for real. What cannot
//! be faked — rust-analyzer's or Lean's actual behaviour — is out of scope for
//! a unit test anyway.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use tempfile::TempDir;

use tracelean_lib::lsp::{
    registry::{LspRegistry, ServerKind},
    transport::{path_to_uri, uri_to_path, LspClient, LspError, Position},
    QueryMode,
};

/// A server that answers `initialize`, echoes a couple of methods, and can be
/// told to misbehave in specific ways.
fn fake_server(dir: &TempDir, name: &str, extra: &str) -> Vec<String> {
    let path = dir.path().join(name);
    let source = format!(
        r#"import json, sys

def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode().strip()
        if line == "":
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":")[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode())

def send(obj):
    body = json.dumps(obj).encode()
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

def respond(msg, result):
    send({{"jsonrpc": "2.0", "id": msg["id"], "result": result}})

{extra}

while True:
    msg = read()
    if msg is None:
        break
    method = msg.get("method")
    if method == "initialize":
        handle_initialize(msg)
    elif method == "shutdown":
        respond(msg, None)
    elif method == "exit":
        break
    elif "id" in msg:
        handle_request(msg, method)
    else:
        handle_notification(msg, method)
"#
    );
    std::fs::write(&path, source).unwrap();
    vec!["python3".into(), path.to_string_lossy().into_owned()]
}

const PLAIN: &str = r#"
def handle_initialize(msg):
    respond(msg, {"capabilities": {"hoverProvider": True}})

def handle_request(msg, method):
    if method == "textDocument/hover":
        respond(msg, {"contents": "the hover text"})
    elif method == "$/lean/plainGoal":
        respond(msg, {"goals": ["⊢ True"]})
    elif method == "textDocument/codeAction":
        respond(msg, [
            {"title": "Try this: simp only [zero_add]", "kind": "quickfix"},
            {"title": "Extract function", "kind": "refactor.extract"},
        ])
    else:
        respond(msg, None)

def handle_notification(msg, method):
    if method == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
              "params": {"uri": uri, "diagnostics": [
                  {"range": {"start": {"line": 2, "character": 4},
                             "end": {"line": 2, "character": 9}},
                   "severity": 1, "message": "unknown identifier", "source": "fake"}
              ]}})
"#;

fn start(dir: &TempDir, command: &[String]) -> Result<LspClient, LspError> {
    LspClient::start(command, dir.path(), None, Duration::from_secs(10))
}

// --- framing and handshake ----------------------------------------------

#[test]
fn initialize_completes_and_reports_capabilities() {
    let dir = TempDir::new().unwrap();
    let command = fake_server(&dir, "plain.py", PLAIN);
    let client = start(&dir, &command).unwrap();
    assert_eq!(client.server_capabilities["hoverProvider"], json!(true));
}

#[test]
fn a_missing_server_explains_how_to_install_it() {
    let dir = TempDir::new().unwrap();
    let started = LspClient::start(
        &["definitely-not-a-language-server-xyz".to_string()],
        dir.path(),
        None,
        Duration::from_secs(2),
    );
    let Err(error) = started else { panic!("a nonexistent program must not start") };
    match error {
        LspError::Spawn(message) => assert!(message.contains("PATH"), "unhelpful: {message}"),
        other => panic!("expected a spawn failure, got {other}"),
    }
}

#[test]
fn requests_are_correlated_by_id() {
    let dir = TempDir::new().unwrap();
    let command = fake_server(&dir, "plain.py", PLAIN);
    let client = start(&dir, &command).unwrap();

    let hover = client
        .request(
            "textDocument/hover",
            json!({"textDocument": {"uri": "file:///x"}, "position": {"line": 0, "character": 0}}),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(hover["contents"], json!("the hover text"));

    let goal = client
        .request("$/lean/plainGoal", json!({}), Duration::from_secs(5))
        .unwrap();
    assert_eq!(goal["goals"][0], json!("⊢ True"));
}

#[test]
fn a_server_error_response_becomes_an_error_not_a_result() {
    let dir = TempDir::new().unwrap();
    let erroring = r#"
def handle_initialize(msg):
    respond(msg, {"capabilities": {}})

def handle_request(msg, method):
    send({"jsonrpc": "2.0", "id": msg["id"],
          "error": {"code": -32601, "message": "method not found"}})

def handle_notification(msg, method):
    pass
"#;
    let command = fake_server(&dir, "erroring.py", erroring);
    let client = start(&dir, &command).unwrap();
    match client.request("textDocument/hover", json!({}), Duration::from_secs(5)) {
        Err(LspError::Server { code, message }) => {
            assert_eq!(code, -32601);
            assert!(message.contains("not found"));
        }
        other => panic!("expected a server error, got {other:?}"),
    }
}

#[test]
fn a_silent_server_times_out_rather_than_hanging() {
    let dir = TempDir::new().unwrap();
    let silent = r#"
def handle_initialize(msg):
    respond(msg, {"capabilities": {}})

def handle_request(msg, method):
    pass   # deliberately never answers

def handle_notification(msg, method):
    pass
"#;
    let command = fake_server(&dir, "silent.py", silent);
    let client = start(&dir, &command).unwrap();
    match client.request("textDocument/hover", json!({}), Duration::from_millis(300)) {
        Err(LspError::Timeout(method)) => assert_eq!(method, "textDocument/hover"),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[test]
fn a_dying_server_unblocks_waiters_instead_of_making_them_wait() {
    let dir = TempDir::new().unwrap();
    let dying = r#"
import sys
def handle_initialize(msg):
    respond(msg, {"capabilities": {}})

def handle_request(msg, method):
    sys.exit(1)

def handle_notification(msg, method):
    pass
"#;
    let command = fake_server(&dir, "dying.py", dying);
    let client = start(&dir, &command).unwrap();
    let started = std::time::Instant::now();
    let result = client.request("textDocument/hover", json!({}), Duration::from_secs(30));
    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "death should not wait out the timeout"
    );
}

#[test]
fn a_server_request_is_answered_so_startup_does_not_stall() {
    // Some servers ask for configuration during initialize and wait for the
    // answer; a client that ignores them never finishes starting.
    let dir = TempDir::new().unwrap();
    let asking = r#"
pending = {}

def handle_initialize(msg):
    send({"jsonrpc": "2.0", "id": 9001, "method": "workspace/configuration",
          "params": {"items": [{"section": "a"}, {"section": "b"}]}})
    reply = read()
    # Record what came back so the test can assert on it.
    respond(msg, {"capabilities": {"configAnswer": reply.get("result")}})

def handle_request(msg, method):
    respond(msg, None)

def handle_notification(msg, method):
    pass
"#;
    let command = fake_server(&dir, "asking.py", asking);
    let client = start(&dir, &command).unwrap();
    assert_eq!(
        client.server_capabilities["configAnswer"],
        json!([null, null]),
        "one answer per requested section"
    );
}

#[test]
fn notifications_are_kept_in_arrival_order() {
    let dir = TempDir::new().unwrap();
    let chatty = r#"
def handle_initialize(msg):
    respond(msg, {"capabilities": {}})

def handle_request(msg, method):
    respond(msg, None)

def handle_notification(msg, method):
    if method == "textDocument/didOpen":
        for i in range(5):
            send({"jsonrpc": "2.0", "method": "window/logMessage",
                  "params": {"type": 3, "message": str(i)}})
"#;
    let command = fake_server(&dir, "chatty.py", chatty);
    let client = start(&dir, &command).unwrap();
    client
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": "file:///x", "languageId": "rust",
                                    "version": 1, "text": ""}}),
        )
        .unwrap();

    // Give the reader thread a moment; the ordering is the assertion, not the
    // timing.
    std::thread::sleep(Duration::from_millis(300));
    let notes = client.drain_notifications();
    let messages: Vec<String> = notes
        .iter()
        .filter(|(m, _)| m == "window/logMessage")
        .map(|(_, p)| p["message"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(messages, vec!["0", "1", "2", "3", "4"]);
}

#[test]
fn draining_notifications_empties_them() {
    let dir = TempDir::new().unwrap();
    let command = fake_server(&dir, "plain.py", PLAIN);
    let client = start(&dir, &command).unwrap();
    client
        .notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": "file:///x", "languageId": "rust",
                                    "version": 1, "text": ""}}),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!client.drain_notifications().is_empty());
    assert!(client.drain_notifications().is_empty(), "a drain takes them");
}

// --- URIs ----------------------------------------------------------------

#[test]
fn paths_round_trip_through_uris() {
    for path in ["/tmp/a.rs", "/tmp/with space/b.lean", "/tmp/ünïcödé/c.py"] {
        let uri = path_to_uri(Path::new(path));
        assert!(uri.starts_with("file://"));
        assert_eq!(uri_to_path(&uri).unwrap(), PathBuf::from(path));
    }
}

#[test]
fn a_non_file_uri_is_rejected() {
    assert!(uri_to_path("http://example.com/x").is_none());
}

// --- the registry --------------------------------------------------------

#[test]
fn languages_are_chosen_by_extension() {
    assert_eq!(ServerKind::for_path(Path::new("a.rs")), Some(ServerKind::Rust));
    assert_eq!(ServerKind::for_path(Path::new("a.lean")), Some(ServerKind::Lean));
    assert_eq!(ServerKind::for_path(Path::new("a.py")), Some(ServerKind::Python));
    assert_eq!(ServerKind::for_path(Path::new("a.txt")), None);
}

#[test]
fn a_file_with_no_server_says_so_rather_than_failing_obscurely() {
    let dir = TempDir::new().unwrap();
    let registry = LspRegistry::new(dir.path());
    let error = registry.open(Path::new("notes.txt"), "hello").unwrap_err();
    assert!(error.to_string().contains("no language server"));
}

#[test]
fn a_missing_server_is_reported_as_not_configured_with_a_hint() {
    // No rust-analyzer is expected on a bare machine; what matters is that the
    // absence reads as "not set up" rather than as a broken feature.
    let dir = TempDir::new().unwrap();
    let registry = LspRegistry::new(dir.path());
    let _ = registry.open(Path::new("a.rs"), "fn main() {}");

    let status = registry.status(ServerKind::Rust);
    if !status.running {
        assert!(status.problem.is_some());
        assert!(
            status.hint.as_deref().unwrap_or_default().contains("rust-analyzer"),
            "the hint should say how to install it"
        );
    }
}

#[test]
fn goal_state_is_refused_for_non_lean_files() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.py"), "x = 1\n").unwrap();
    let registry = LspRegistry::new(dir.path());
    let error = registry
        .query(Path::new("a.py"), Position { line: 0, character: 0 }, QueryMode::Goal)
        .unwrap_err();
    // Either the server is missing, or the request is refused — both are
    // honest; what must not happen is a goal being invented.
    let message = error.to_string();
    assert!(
        message.contains("Lean-only") || message.contains("language server"),
        "unexpected: {message}"
    );
}

#[test]
fn query_modes_parse_from_their_wire_names() {
    assert_eq!(QueryMode::parse("goal"), Some(QueryMode::Goal));
    assert_eq!(QueryMode::parse("code_actions"), Some(QueryMode::CodeActions));
    assert_eq!(QueryMode::parse("nonsense"), None);
}

// --- live Lean server ----------------------------------------------------

fn lean_available() -> bool {
    std::process::Command::new("lake")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn example_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("example");
    root.join("formal/Checkout.lean").is_file().then_some(root)
}

/// `lake serve` must start inside the *package*, not at the project root.
///
/// The example keeps its model in `example/formal/`, one directory below the
/// project. Keying servers by language alone started `lake serve` at the root,
/// where there is no lakefile, and every `import` in the model failed. The
/// registry now finds the nearest lakefile ancestor.
#[test]
fn the_lean_server_starts_in_the_package_not_the_project_root() {
    let Some(root) = example_root() else {
        eprintln!("no example project — skipping");
        return;
    };
    if !lean_available() {
        eprintln!("no Lean toolchain — skipping");
        return;
    }

    let registry = LspRegistry::new(&root);
    let file = Path::new("formal/Checkout.lean");
    let content = std::fs::read_to_string(root.join(file)).unwrap();
    registry.open(file, &content).expect("the Lean server should start");

    let status = registry.status(ServerKind::Lean);
    assert!(status.running, "status: {status:?}");
    // The program is resolved to an absolute path before the spawn, so a
    // toolchain that is installed but not on this process's PATH still starts.
    let command = status.command.clone().expect("a running server has a command");
    assert!(
        command[0].ends_with("lake") && command[1] == "serve",
        "a package with a lakefile must be served by `lake serve`, got {command:?}"
    );

    registry.shutdown_all();
}

/// The infoview's whole premise: a goal really does come back from
/// `$/lean/plainGoal` at a cursor inside a tactic block.
#[test]
fn a_tactic_position_answers_with_a_goal() {
    let Some(root) = example_root() else { return };
    if !lean_available() {
        eprintln!("no Lean toolchain — skipping");
        return;
    }

    let file = Path::new("formal/CheckoutProofs.lean");
    let full = root.join(file);
    if !full.is_file() {
        return;
    }
    let content = std::fs::read_to_string(&full).unwrap();

    // Put the cursor just after a tactic line inside the first proof.
    let needle = "unfold discountCents";
    let Some(byte) = content.find(needle) else {
        panic!("the fixture proof should contain `{needle}`");
    };
    let prefix = &content[..byte];
    let line = prefix.matches('\n').count() as u32;
    let character = needle.len() as u32;

    let registry = LspRegistry::new(&root);
    registry.open(file, &content).expect("server should start");

    let answer = registry.query(file, Position { line, character }, QueryMode::Goal);
    registry.shutdown_all();

    let value = answer.expect("plainGoal should answer");
    // Lean returns `{ "goals": [...] }` or null; either is a legitimate answer,
    // but a *null with no error* must never be rendered as "proof complete" —
    // that distinction is the infoview's central rule, and is enforced there.
    assert!(
        value.is_null() || value.get("goals").is_some() || value.get("rendered").is_some(),
        "unexpected plainGoal shape: {value}"
    );
}

// --- Finding the toolchain -------------------------------------------------
//
// The Lean panel reporting "the language server is not running" on a machine
// where Lean *is* installed was a PATH problem, not a missing toolchain: elan
// writes its PATH line into `~/.profile`, which only a login shell reads, so a
// GUI started from a desktop launcher never sees `~/.elan/bin`.

#[test]
fn resolve_program_finds_a_binary_on_path_and_returns_an_absolute_path() {
    let resolved = tracelean_core::lsp::registry::resolve_program("sh")
        .expect("sh is on PATH in any environment these tests run in");
    assert!(
        std::path::Path::new(&resolved).is_absolute(),
        "expected an absolute path so the spawn does not depend on the child's cwd, got {resolved}"
    );
}

#[test]
fn resolve_program_says_where_it_looked_when_a_program_is_missing() {
    // The message is the point: "not running" told the user nothing they could
    // act on, so a failure has to name both PATH and the toolchain directories
    // that were searched after it.
    let problem = tracelean_core::lsp::registry::resolve_program("definitely-not-a-real-binary")
        .expect_err("a nonexistent program cannot resolve");
    assert!(problem.contains("not found on PATH"), "{problem}");
    assert!(
        problem.contains(".elan/bin"),
        "the message must name where it looked: {problem}"
    );
}

#[test]
fn resolve_program_rejects_an_explicit_path_that_is_not_executable() {
    let problem = tracelean_core::lsp::registry::resolve_program("/etc/hostname")
        .expect_err("a readable but non-executable file is not a server");
    assert!(problem.contains("not an executable file"), "{problem}");
}
