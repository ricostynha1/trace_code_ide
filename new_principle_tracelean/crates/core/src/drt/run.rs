//! Running a differential test: ask both sides the same cases, compare, shrink.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value;

use super::gen::{self, Rng};
use super::protocol::{self, Heard, Reply, RunnerError};
use super::schema::Schema;

/// How to start one side.
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerSpec {
    pub cmd: Vec<String>,
    pub cwd: Option<PathBuf>,
}

/// A live runner process.
pub struct Runner {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
        let mut child = command.spawn().map_err(|e| RunnerError::Spawn(e.to_string()))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = BufReader::new(child.stdout.take().expect("piped"));
        Ok(Runner { child, stdin, stdout })
    }

    /// Ask one case and read one reply.
    pub fn ask(&mut self, case: u64, op: &str, input: &Value) -> Result<Reply, RunnerError> {
        let line = serde_json::json!({"case": case, "op": op, "input": input});
        writeln!(self.stdin, "{line}").map_err(|e| RunnerError::Died(e.to_string()))?;
        self.stdin.flush().map_err(|e| RunnerError::Died(e.to_string()))?;

        let mut reply = String::new();
        match self.stdout.read_line(&mut reply) {
            Ok(0) => return Err(RunnerError::Died("closed its pipe".to_string())),
            Ok(_) => {}
            Err(e) => return Err(RunnerError::Died(e.to_string())),
        }
        // Every way this line can fail is decided by `hear`, which is a total
        // function and is differentially tested. What is left here is turning
        // its conclusion into this function's error type.
        match crate::drt::protocol::hear(case, reply.clone()) {
            Heard::Answered { case_number, output, error } => {
                Ok(Reply { case: case_number, output, error })
            }
            Heard::WrongCase { expected, got } => Err(RunnerError::Protocol(format!(
                "runner answered case {got} when case {expected} was asked"
            ))),
            Heard::NotAReply { reason } => Err(RunnerError::Protocol(format!(
                "{reason:?}: {}",
                reply.trim()
            ))),
            Heard::NotExclusive { .. } => Err(RunnerError::Protocol(
                "a reply carried both an output and an error, or neither".to_string(),
            )),
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
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
            });
        }
    }

    Ok(DrtResult {
        op: op.to_string(),
        seed: options.seed,
        cases: options.cases,
        divergence: None,
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
