use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

use serde_json::{json, Value};

pub fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

pub fn file_uri(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", absolute.display().to_string().replace(' ', "%20"))
}

pub fn utf16_column(line: &str, column: usize) -> u32 {
    line.chars().take(column).map(|c| c.len_utf16() as u32).sum()
}

/// @implements REQ-LSP.proof_state_shown
pub fn goals_shown(answered: bool, goals: Option<Vec<String>>, rendered: Option<String>) -> String {
    if !answered {
        return "no goals here".to_string();
    }
    match goals {
        Some(goals) if goals.is_empty() => "no goals".to_string(),
        Some(goals) => goals.join("\n\n"),
        None => rendered.unwrap_or_else(|| "no goals here".to_string()),
    }
}

pub fn goal_text(answer: &Value) -> String {
    if let Some(error) = answer.get("error") {
        let said = error.get("message").and_then(Value::as_str).unwrap_or("the server refused");
        return format!("the Lean server said: {said}");
    }
    match answer.get("result") {
        None | Some(Value::Null) => goals_shown(false, None, None),
        Some(result) => goals_shown(
            true,
            result
                .get("goals")
                .and_then(Value::as_array)
                .map(|goals| goals.iter().filter_map(Value::as_str).map(str::to_string).collect()),
            result.get("rendered").and_then(Value::as_str).map(str::to_string),
        ),
    }
}

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

pub struct LeanServer {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Value>,
    next: u64,
    versions: BTreeMap<String, i64>,
    waiting: Option<u64>,
}

impl LeanServer {
    pub fn start(project: &Path, elan: Option<PathBuf>) -> Result<Self, String> {
        let spawn = |lake: &Path| {
            Command::new(lake)
                .args(["env", "lean", "--server"])
                .current_dir(project)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        };
        let mut child = spawn(Path::new("lake"))
            .or_else(|error| elan.as_deref().filter(|lake| lake.exists()).map_or(Err(error), spawn))
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
