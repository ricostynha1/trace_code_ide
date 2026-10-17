//! Lean's own language server, asked what the proof state is at a position.
//!
//! Reading a proof is watching its goals change as the cursor moves, which only
//! the Lean server can say. This is the project's own toolchain, the way the
//! differential runners are — `lake env lean --server` in the Lean project the
//! file belongs to — and never an agent: it is started to be asked one question
//! and says nothing the person did not ask for.
//!
//! Asking never waits. A request is sent and the answer is picked up by a later
//! `poll`, so the editor stays as quick as the keys, and a server that is slow
//! (Lean elaborates the file before it answers) costs a stale panel rather
//! than a frozen window. What is pure — the framing, the position, the reading
//! of an answer — is separate from what starts a process, and tested without one.
//!
//! @implements REQ-LSP.proof_state_shown

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

use serde_json::{json, Value};

/// One message as the language-server protocol frames it.
pub fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// A file's address as a server names it.
pub fn file_uri(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", absolute.display().to_string().replace(' ', "%20"))
}

/// A character column as the server counts it: in UTF-16 units, so a line with
/// `𝔸` before the cursor is one unit further on than it looks.
pub fn utf16_column(line: &str, column: usize) -> u32 {
    line.chars().take(column).map(|c| c.len_utf16() as u32).sum()
}

/// What a `plainGoal` answer says, as lines to show: each goal, a blank line
/// between them, or that there is none where the cursor is.
pub fn goal_text(answer: &Value) -> String {
    if let Some(error) = answer.get("error") {
        let said = error.get("message").and_then(Value::as_str).unwrap_or("the server refused");
        return format!("the Lean server said: {said}");
    }
    match answer.get("result") {
        None | Some(Value::Null) => "no goals here".to_string(),
        Some(result) => match result.get("goals").and_then(Value::as_array) {
            Some(goals) if !goals.is_empty() => {
                goals.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n")
            }
            Some(_) => "no goals".to_string(),
            None => result.get("rendered").and_then(Value::as_str).unwrap_or("no goals here").to_string(),
        },
    }
}

/// The Lean project a file belongs to: the nearest folder at or above it, and
/// no higher than `root`, that has a `lean-toolchain` or a lakefile.
pub fn project_of(file: &Path, root: &Path) -> Option<PathBuf> {
    let mut at = file.parent();
    while let Some(dir) = at {
        let marked = ["lean-toolchain", "lakefile.lean", "lakefile.toml"].iter().any(|m| dir.join(m).exists());
        if marked {
            return Some(dir.to_path_buf());
        }
        if dir == root {
            return None;
        }
        at = dir.parent();
    }
    None
}

fn read_frames(source: impl Read, replies: Sender<Value>) {
    let mut reader = BufReader::new(source);
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(n) = header.strip_prefix("Content-Length:") {
                length = n.trim().parse::<usize>().ok();
            }
        }
        let Some(length) = length else { continue };
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&body) {
            if replies.send(value).is_err() {
                return;
            }
        }
    }
}

/// A running Lean server and the one question it is being asked.
pub struct LeanServer {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Value>,
    next: u64,
    /// The version of each file the server was last told, so a change is told as one.
    versions: BTreeMap<String, i64>,
    /// The request whose answer is wanted; an older one's is ignored.
    waiting: Option<u64>,
}

impl LeanServer {
    /// Start the server in `project`, or say why it cannot be.
    pub fn start(project: &Path) -> Result<Self, String> {
        let mut child = Command::new("lake")
            .args(["env", "lean", "--server"])
            .current_dir(project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("`lake env lean --server` did not start ({error}); is Lean installed?"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("the Lean server has no pipes".to_string());
        };
        let (sender, replies) = channel();
        std::thread::spawn(move || read_frames(stdout, sender));
        let mut server = LeanServer { child, stdin, replies, next: 1, versions: BTreeMap::new(), waiting: None };
        let id = server.take_id();
        server.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "initialize",
            "params": { "processId": null, "rootUri": file_uri(project), "capabilities": {} }
        }))?;
        server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))?;
        Ok(server)
    }

    fn take_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        self.stdin
            .write_all(&frame(message))
            .and_then(|()| self.stdin.flush())
            .map_err(|error| format!("the Lean server is gone ({error})"))
    }

    /// Ask for the goals at `line` and `column` (characters, from 0) of `text`,
    /// the file's contents now. The answer comes from `poll`.
    pub fn ask(&mut self, file: &Path, text: &str, line: usize, column: usize) -> Result<(), String> {
        let uri = file_uri(file);
        let version = self.versions.get(&uri).copied().map_or(1, |v| v + 1);
        let first = !self.versions.contains_key(&uri);
        self.versions.insert(uri.clone(), version);
        if first {
            self.send(&json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "languageId": "lean4", "version": version, "text": text } }
            }))?;
        } else {
            self.send(&json!({
                "jsonrpc": "2.0", "method": "textDocument/didChange",
                "params": { "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }
            }))?;
        }
        let at = text.split('\n').nth(line).unwrap_or_default();
        let id = self.take_id();
        self.waiting = Some(id);
        self.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "$/lean/plainGoal",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": utf16_column(at, column) }
            }
        }))
    }

    /// The answer to the last question, if it has come: what to show. Anything
    /// else the server says (diagnostics, progress) is not what was asked.
    pub fn poll(&mut self) -> Option<String> {
        while let Ok(message) = self.replies.try_recv() {
            if message.get("id").and_then(Value::as_u64) == self.waiting && self.waiting.is_some() {
                self.waiting = None;
                return Some(goal_text(&message));
            }
        }
        None
    }
}

impl Drop for LeanServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-LSP.proof_state_shown
    #[test]
    fn a_message_is_framed_by_its_length_in_bytes() {
        let framed = String::from_utf8(frame(&json!({ "a": "λ" }))).unwrap();
        let body = r#"{"a":"λ"}"#;
        assert_eq!(framed, format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        assert!(body.len() > body.chars().count(), "the length is bytes, not characters");
    }

    /// @tests REQ-LSP.proof_state_shown
    #[test]
    fn a_column_counts_utf16_units() {
        assert_eq!(utf16_column("ab𝔸c", 3), 4);
        assert_eq!(utf16_column("abc", 10), 3);
    }

    /// @tests REQ-LSP.proof_state_shown
    #[test]
    fn goals_are_read_from_the_answer() {
        let one = json!({ "id": 3, "result": { "goals": ["x : Nat\n⊢ x = x"], "rendered": "r" } });
        assert_eq!(goal_text(&one), "x : Nat\n⊢ x = x");
        let two = json!({ "result": { "goals": ["a", "b"] } });
        assert_eq!(goal_text(&two), "a\n\nb");
        assert_eq!(goal_text(&json!({ "result": { "goals": [] } })), "no goals");
        assert_eq!(goal_text(&json!({ "result": null })), "no goals here");
        assert!(goal_text(&json!({ "error": { "message": "bad" } })).contains("bad"));
    }

    /// @tests REQ-LSP.proof_state_shown
    #[test]
    fn the_project_is_the_nearest_folder_with_a_toolchain() {
        let root = std::env::temp_dir().join(format!("tracelean-lean-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("formal/Sub")).unwrap();
        std::fs::write(root.join("formal/lean-toolchain"), "x").unwrap();
        let file = root.join("formal/Sub/A.lean");
        assert_eq!(project_of(&file, &root), Some(root.join("formal")));
        assert_eq!(project_of(&root.join("src/b.lean"), &root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
