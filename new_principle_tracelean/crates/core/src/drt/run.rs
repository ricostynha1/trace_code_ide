//! Running a differential test: ask both sides the same cases, compare, shrink.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::Value;

use super::gen::{self, Rng};
use super::protocol::{self, Event, Reply, RunnerError};
use super::schema::Schema;

/// How to start one side.
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerSpec {
    pub cmd: Vec<String>,
    pub cwd: Option<PathBuf>,
}

/// How long one case may take before its runner is declared timed out.
///
/// Generous: a model is compiled and a case costs microseconds, so anything
/// near this is a runner stuck in a loop, not a slow one.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

/// A live runner process.
///
/// Its output is read on a thread of its own and handed over line by line, so
/// that waiting for a reply can stop at `REPLY_TIMEOUT` instead of blocking
/// forever on a runner that never answers.
pub struct Runner {
    child: Child,
    /// Taken when the runner is let go, so that its input ends.
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
                // The end of its output: dropping the sender says so.
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

    /// Ask one case and read one reply.
    ///
    /// What happened is put into an `Event` and `protocol::outcome` decides
    /// what it means — a total function, differentially tested. What is left
    /// here is the IO and turning that conclusion into this function's result.
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
    /// Its input is closed first, so a runner that reads to the end exits on
    /// its own — an instrumented one writes its profile only then — and is
    /// killed only if it has not after two hundred short waits: a count, not a
    /// clock, so nothing here reads the time (`ARCH-DETERMINISM`).
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

/// One disagreement, already reduced.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    pub input: Value,
    pub model: Reply,
    pub implementation: Reply,
}

/// What a run established.
#[derive(Debug, Clone, PartialEq)]
pub struct DrtResult {
    pub op: String,
    pub seed: u64,
    pub cases: u64,
    pub divergence: Option<Divergence>,
    /// The shape the cases were drawn from, so the run can be replayed.
    pub schema: Schema,
    /// How many of the cases asked reached each class of the arguments
    /// (`classes::reached`): the coverage the run reached, beside its count
    /// and its seed.
    pub reached: Vec<super::coverage::Observed>,
}

impl DrtResult {
    /// Falsification, not proof: a clean run is evidence, never a guarantee.
    ///
    /// @implements REQ-DRT.falsification_only
    pub fn agreed(&self) -> bool {
        self.divergence.is_none()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    pub seed: u64,
    pub cases: u64,
    /// How many reduction rounds to attempt on a divergence.
    pub shrink_rounds: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { seed: 1, cases: 500, shrink_rounds: 200 }
    }
}

/// Two replies agree when they are the same answer.
///
/// An error on one side against an output on the other is a divergence; both
/// failing is agreement, because the model rejecting an input the implementation
/// also rejects is the two behaving alike.
///
/// @implements REQ-DRT.error_is_an_answer
pub fn agree(model: &Reply, implementation: &Reply) -> bool {
    // The decision itself lives in `protocol`, where it is modelled and
    // differentially tested; this is the projection from two replies onto it.
    protocol::agree(model.output.clone(), implementation.output.clone())
}

/// Ask both sides the same cases, in the same order, and reduce the first
/// disagreement.
///
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

    for case in 1..=options.cases {
        let input = gen::value(schema, &mut rng);
        let a = model_runner.ask(case, op, &input)?;
        let b = impl_runner.ask(case, op, &input)?;
        if !agree(&a, &b) {
            let reduced = shrink_divergence(
                op,
                schema,
                &input,
                &mut model_runner,
                &mut impl_runner,
                options.shrink_rounds,
            )?;
            return Ok(DrtResult {
                op: op.to_string(),
                seed: options.seed,
                cases: case,
                divergence: Some(reduced),
                schema: schema.clone(),
                reached: super::classes::reached(schema.clone(), options.seed, case),
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
    })
}

/// Reduce a diverging input while it still diverges.
///
/// A reduction that stops diverging has not found a smaller case — it has found
/// a different one — so only candidates that still disagree are accepted.
///
/// @implements REQ-DRT-GEN.shrink_preserves
fn shrink_divergence(
    op: &str,
    schema: &Schema,
    input: &Value,
    model: &mut Runner,
    implementation: &mut Runner,
    rounds: usize,
) -> Result<Divergence, RunnerError> {
    let mut current = input.clone();
    // A number far above any run's case count, so a reduction's cases cannot be
    // confused with the run's own — and one every runner can carry back
    // unchanged. It used to be `u64::MAX / 2`, which a JavaScript runner cannot
    // echo: its numbers are doubles, integers above 2^53 do not survive a round
    // trip, and the reply came back as a different case. That turned every
    // divergence against the TypeScript side into a protocol error instead of a
    // reduced counterexample — the reduction failing exactly when it was
    // needed. 2^50 is exact in a double and larger than any case count here.
    let mut case = 1u64 << 50;

    for _ in 0..rounds {
        let mut improved = false;
        for candidate in gen::shrink(schema, &current) {
            case += 1;
            let a = model.ask(case, op, &candidate)?;
            let b = implementation.ask(case, op, &candidate)?;
            if !agree(&a, &b) {
                current = candidate;
                improved = true;
                break;
            }
        }
        if !improved {
            break;
        }
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
