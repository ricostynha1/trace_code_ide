//! The conformance protocol: line-delimited JSON over stdin/stdout.
//!
//! Both sides of a differential test — the compiled Lean model and the real
//! implementation — are ordinary subprocesses reading one JSON object per line
//! and writing one back. That is the whole reason this bond is
//! language-independent: TraceLean never parses the implementation, it only
//! runs it, so `unsafe`, generics, FFI and third-party crates are all
//! irrelevant.
//!
//! ```text
//! → {"case":41,"op":"login","input":{"username":"ana","password":"hunter22"}}
//! ← {"case":41,"output":"weakPassword"}
//! ← {"case":42,"error":"panic: index out of bounds"}
//! ```

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// One generated case, sent to both sides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub case: u64,
    /// Which entry point to exercise. Declared in `.tracelean/drt.json`;
    /// `"default"` when a binding exposes only one.
    pub op: String,
    pub input: serde_json::Value,
}

/// What a runner answered. Exactly one of `output` / `error` is present: an
/// `error` on one side against an `output` on the other is a divergence, not a
/// failed run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub case: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    pub fn ok(case: u64, output: serde_json::Value) -> Self {
        Self { case, output: Some(output), error: None }
    }

    pub fn failed(case: u64, error: impl Into<String>) -> Self {
        Self { case, output: None, error: Some(error.into()) }
    }
}

/// Why a runner could not answer.
#[derive(Debug, Clone, PartialEq)]
pub enum RunnerError {
    /// The command could not be started at all.
    Spawn(String),
    /// No reply within the per-case timeout.
    Timeout,
    /// The process exited or closed its pipe.
    Died(String),
    /// A line came back that is not a `Reply`.
    Protocol(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Spawn(m) => write!(f, "could not start runner: {m}"),
            RunnerError::Timeout => write!(f, "runner did not answer in time"),
            RunnerError::Died(m) => write!(f, "runner died: {m}"),
            RunnerError::Protocol(m) => write!(f, "runner spoke nonsense: {m}"),
        }
    }
}

/// How to start one side of a differential test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerSpec {
    /// Argv; the first element is the program.
    pub cmd: Vec<String>,
    #[serde(default)]
    pub cwd: Option<std::path::PathBuf>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

/// A long-lived subprocess speaking the protocol.
///
/// Long-lived on purpose: spawning a process per case would dominate the cost
/// of a run that is meant to execute millions of them.
pub struct Runner {
    spec: RunnerSpec,
    child: Child,
    stdin: ChildStdin,
    /// Replies arrive on a channel so a hung runner can be abandoned without
    /// blocking the whole run on a read that never returns.
    rx: mpsc::Receiver<Result<String, String>>,
    /// Stderr is captured rather than discarded: it is the only place a panic
    /// message exists, and that message is what makes an `error` reply useful.
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
    /// Signalled once the stderr reader has drained the pipe to EOF.
    ///
    /// Without this, reading `stderr` the instant stdout closed was a race the
    /// machine won whenever it was busy: the child had died, its stderr thread
    /// had not yet been scheduled, and the death was reported with no reason
    /// attached — precisely when the reason matters most.
    stderr_drained: mpsc::Receiver<()>,
    restarts: u32,
}

impl Runner {
    pub fn spawn(spec: &RunnerSpec) -> Result<Runner, RunnerError> {
        let Some((program, args)) = spec.cmd.split_first() else {
            return Err(RunnerError::Spawn("empty command".into()));
        };

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            command.env(k, v);
        }

        let mut child = command.spawn().map_err(|e| RunnerError::Spawn(e.to_string()))?;
        let stdin = child.stdin.take().ok_or_else(|| RunnerError::Spawn("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| RunnerError::Spawn("no stdout".into()))?;
        let child_stderr = child.stderr.take();

        let rx = reader_thread(stdout);

        let stderr = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (drained_tx, stderr_drained) = mpsc::channel();
        if let Some(mut handle) = child_stderr {
            let sink = stderr.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = String::new();
                let _ = handle.read_to_string(&mut buf);
                if let Ok(mut guard) = sink.lock() {
                    guard.push_str(&buf);
                }
                let _ = drained_tx.send(());
            });
        }

        Ok(Runner { spec: spec.clone(), child, stdin, rx, stderr, stderr_drained, restarts: 0 })
    }

    /// Send one case and wait for its reply.
    ///
    /// Both the write and the read are flushed and bounded: Lean's stdout is
    /// block-buffered on a pipe, so an unflushed line deadlocks the very first
    /// case.
    pub fn ask(&mut self, case: &Case, timeout: Duration) -> Result<Reply, RunnerError> {
        let line = serde_json::to_string(case)
            .map_err(|e| RunnerError::Protocol(format!("encoding case: {e}")))?;

        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| RunnerError::Died(self.death_note(&e.to_string())))?;

        loop {
            match self.rx.recv_timeout(timeout) {
                Ok(Ok(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue; // blank lines are noise, not answers
                    }
                    let reply: Reply = serde_json::from_str(trimmed).map_err(|e| {
                        RunnerError::Protocol(format!("{e}: {}", truncate(trimmed, 200)))
                    })?;
                    if reply.case != case.case {
                        // Out-of-order or stale reply: keep reading rather than
                        // silently pairing the wrong answer with this case.
                        continue;
                    }
                    return Ok(reply);
                }
                Ok(Err(e)) => return Err(RunnerError::Died(self.death_note(&e))),
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(RunnerError::Timeout),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(RunnerError::Died(self.death_note("stdout closed")))
                }
            }
        }
    }

    /// Restart after a death or a timeout. Bounded, so a runner that dies on
    /// every case cannot spin forever.
    pub fn restart(&mut self, max_restarts: u32) -> Result<(), RunnerError> {
        if self.restarts >= max_restarts {
            return Err(RunnerError::Died(format!("gave up after {} restarts", self.restarts)));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let fresh = Runner::spawn(&self.spec)?;
        let restarts = self.restarts + 1;
        *self = fresh;
        self.restarts = restarts;
        Ok(())
    }

    /// Whatever the process wrote to stderr — usually the panic message.
    pub fn stderr_text(&self) -> String {
        self.stderr.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// How long to wait for the stderr reader after a death.
    ///
    /// Bounded on purpose: a child that dies while holding stderr open — say it
    /// spawned a grandchild that inherited the pipe — must not hang the run. A
    /// reason arriving late is worth half a second; a run that never finishes
    /// is not.
    const STDERR_GRACE: Duration = Duration::from_millis(500);

    fn death_note(&self, reason: &str) -> String {
        // Let the reader finish before deciding there was nothing to say.
        let _ = self.stderr_drained.recv_timeout(Self::STDERR_GRACE);
        let err = self.stderr_text();
        if err.trim().is_empty() {
            reason.to_string()
        } else {
            format!("{reason}: {}", truncate(err.trim(), 500))
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reader_thread(stdout: ChildStdout) -> mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let message = match line {
                Ok(l) => Ok(l),
                Err(e) => Err(e.to_string()),
            };
            if tx.send(message).is_err() {
                break; // the run was abandoned
            }
        }
    });
    rx
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Compare two replies for conformance.
///
/// Comparison is over parsed JSON, never over strings, and numbers are
/// canonicalized first: `serde_json` collapses a large `Nat` to `f64`, which
/// would otherwise make two genuinely different numbers compare equal.
pub fn replies_agree(a: &Reply, b: &Reply) -> bool {
    match (&a.output, &b.output) {
        (Some(x), Some(y)) => values_agree(x, y),
        // Both sides failing is agreement about failure; the messages are free
        // to differ, since one is Lean's and one is the implementation's.
        (None, None) => a.error.is_some() && b.error.is_some(),
        _ => false,
    }
}

/// Structural equality with two deliberate allowances, both of which follow
/// from how Lean derives `ToJson`: an absent optional field is `null`, and
/// numbers are compared by their canonical decimal form.
pub fn values_agree(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => canonical_number(x) == canonical_number(y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(i, j)| values_agree(i, j))
        }
        (Value::Object(x), Value::Object(y)) => {
            // A field Lean omits because it was `none` must match an explicit
            // `null` on the other side.
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            keys.into_iter().all(|k| {
                let l = x.get(k).unwrap_or(&Value::Null);
                let r = y.get(k).unwrap_or(&Value::Null);
                values_agree(l, r)
            })
        }
        _ => false,
    }
}

fn canonical_number(n: &serde_json::Number) -> String {
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    match n.as_f64() {
        // Trim a trailing `.0` so `1` and `1.0` are the same value.
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i128),
        Some(f) => format!("{f}"),
        None => n.to_string(),
    }
}
