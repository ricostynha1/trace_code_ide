use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::Value;

use super::gen::{self, Rng};
use super::protocol::{self, Event, Reply, RunnerError};
use super::schema::Schema;

pub fn workers() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    std::env::var("TRACELEAN_JOBS").ok().and_then(|j| j.parse().ok()).unwrap_or(cores.saturating_sub(1)).max(1)
}

pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done: std::sync::Mutex<Vec<(usize, R)>> = std::sync::Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..workers().min(items.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let answer = f(item);
                done.lock().unwrap_or_else(|e| e.into_inner()).push((i, answer));
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(|e| e.into_inner());
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunnerSpec {
    pub cmd: Vec<String>,
    pub cwd: Option<PathBuf>,
}

pub const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Runner {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<std::io::Result<String>>,
}

impl Runner {
    pub fn start(spec: &RunnerSpec) -> Result<Runner, RunnerError> {
        let (program, args) = spec.cmd.split_first().ok_or_else(|| {
            RunnerError::Spawn("no command given".to_string())
        })?;
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let failed = protocol::outcome(Event::CouldNotStart { reason: e.to_string() });
                return Err(protocol::into_reply(failed, "").err().unwrap_or(RunnerError::Spawn(e.to_string())));
            }
        };
        let stdin = child.stdin.take().expect("piped");
        let mut stdout = BufReader::new(child.stdout.take().expect("piped"));
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            match stdout.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if send.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = send.send(Err(e));
                    break;
                }
            }
        });
        Ok(Runner { child, stdin: Some(stdin), lines })
    }

    pub fn ask(&mut self, case: u64, op: &str, input: &Value) -> Result<Reply, RunnerError> {
        let line = protocol::case_line(case, op.to_string(), input.clone());
        let written = match self.stdin.as_mut() {
            Some(stdin) => writeln!(stdin, "{line}").and_then(|()| stdin.flush()),
            None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the runner was let go")),
        };
        let (event, reply) = match written {
            Err(e) => (Event::PipeBroke { reason: e.to_string() }, String::new()),
            Ok(()) => match self.lines.recv_timeout(REPLY_TIMEOUT) {
                Ok(Ok(reply)) => (Event::Read { expected: case, got: Some(reply.clone()) }, reply),
                Ok(Err(e)) => (Event::PipeBroke { reason: e.to_string() }, String::new()),
                Err(RecvTimeoutError::Timeout) => (Event::NoReplyInTime, String::new()),
                Err(RecvTimeoutError::Disconnected) => (Event::Read { expected: case, got: None }, String::new()),
            },
        };
        protocol::into_reply(protocol::outcome(event), &reply)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        drop(self.stdin.take());
        for _ in 0..200 {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    pub input: Value,
    pub model: Reply,
    pub implementation: Reply,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrtResult {
    pub op: String,
    pub seed: u64,
    pub cases: u64,
    pub divergence: Option<Divergence>,
    pub schema: Schema,
    pub reached: Vec<super::coverage::Observed>,
    pub unreadable: u64,
}

fn unreadable(reply: &Reply) -> bool {
    reply.error.as_deref().is_some_and(|e| e.starts_with("argument `"))
}

impl DrtResult {
    /// @implements REQ-DRT.falsification_only
    pub fn agreed(&self) -> bool {
        self.divergence.is_none()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    pub seed: u64,
    pub cases: u64,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { seed: 1, cases: 500 }
    }
}

/// @implements REQ-DRT.error_is_an_answer
pub fn agree(model: &Reply, implementation: &Reply) -> bool {
    protocol::agree(model.output.clone(), implementation.output.clone())
}

/// @implements REQ-DRT.same_question
/// @implements REQ-DRT.divergence_reported
/// @implements REQ-DRT.case_count_stated
pub fn run(
    op: &str,
    schema: &Schema,
    model: &RunnerSpec,
    implementation: &RunnerSpec,
    options: RunOptions,
) -> Result<DrtResult, RunnerError> {
    let mut model_runner = Runner::start(model)?;
    let mut impl_runner = Runner::start(implementation)?;
    let mut rng = Rng::new(options.seed);
    let mut unread = 0;

    for case in 1..=options.cases {
        let input = gen::value(schema, &mut rng);
        let a = model_runner.ask(case, op, &input)?;
        let b = impl_runner.ask(case, op, &input)?;
        if unreadable(&b) {
            unread += 1;
        }
        if !agree(&a, &b) {
            let reduced = shrink_divergence(
                op,
                schema,
                &input,
                &mut model_runner,
                &mut impl_runner,
            )?;
            return Ok(DrtResult {
                op: op.to_string(),
                seed: options.seed,
                cases: case,
                divergence: Some(reduced),
                schema: schema.clone(),
                reached: super::classes::reached(schema.clone(), options.seed, case),
                unreadable: unread,
            });
        }
    }

    Ok(DrtResult {
        op: op.to_string(),
        seed: options.seed,
        cases: options.cases,
        divergence: None,
        schema: schema.clone(),
        reached: super::classes::reached(schema.clone(), options.seed, options.cases),
        unreadable: unread,
    })
}

/// @implements REQ-DRT-GEN.shrink_preserves
/// @implements REQ-DRT-GEN.shrink_minimal
fn shrink_divergence(
    op: &str,
    schema: &Schema,
    input: &Value,
    model: &mut Runner,
    implementation: &mut Runner,
) -> Result<Divergence, RunnerError> {
    let mut current = input.clone();
    let mut case = 1u64 << 50;

    'reducing: loop {
        for candidate in gen::shrink(schema, &current) {
            case += 1;
            let a = model.ask(case, op, &candidate)?;
            let b = implementation.ask(case, op, &candidate)?;
            if !agree(&a, &b) {
                current = candidate;
                continue 'reducing;
            }
        }
        break;
    }

    case += 1;
    let model_reply = model.ask(case, op, &current)?;
    let impl_reply = implementation.ask(case + 1, op, &current)?;
    Ok(Divergence { input: current, model: model_reply, implementation: impl_reply })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drt::protocol::Reply;

    /// @tests REQ-DRT.error_is_an_answer
    #[test]
    fn both_failing_is_agreement_and_one_failing_is_not() {
        let out = Reply::ok(1, serde_json::json!("L1"));
        let other = Reply::ok(1, serde_json::json!("L2"));
        let err = Reply::failed(1, "no");
        let err2 = Reply::failed(1, "different words, same refusal");

        assert!(agree(&out, &out.clone()));
        assert!(!agree(&out, &other));
        assert!(agree(&err, &err2), "both rejecting is agreement");
        assert!(!agree(&out, &err), "one rejecting is a divergence");
    }
}
