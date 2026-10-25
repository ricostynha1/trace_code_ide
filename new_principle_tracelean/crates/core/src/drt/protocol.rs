use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// @implements REQ-DRT-PROTO.case_names_op
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub case: u64,
    pub op: String,
    pub input: serde_json::Value,
}

/// @implements REQ-DRT-PROTO.reply_exclusive
/// @implements REQ-DRT-PROTO.case_echoed
/// @implements REQ-DRT.error_is_an_answer
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub case: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    /// @implements REQ-DRT-PROTO.reply_exclusive
    pub fn from_json(value: &serde_json::Value) -> Result<Reply, String> {
        let object = value.as_object().ok_or("a reply must be a JSON object")?;
        let case = object
            .get("case")
            .and_then(|c| c.as_u64())
            .ok_or("a reply must carry the case number it answers")?;
        Ok(Reply {
            case,
            output: object.get("output").cloned(),
            error: object
                .get("error")
                .map(|e| e.as_str().unwrap_or("unreadable error").to_string()),
        })
    }

    pub fn ok(case: u64, output: serde_json::Value) -> Self {
        Self { case, output: Some(output), error: None }
    }

    pub fn failed(case: u64, error: impl Into<String>) -> Self {
        Self { case, output: None, error: Some(error.into()) }
    }

    /// @implements REQ-DRT-PROTO.reply_exclusive
    pub fn is_well_formed(&self) -> bool {
        self.output.is_some() != self.error.is_some()
    }
}

/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotAReply {
    NotJson,
    NotAnObject,
    NoCaseNumber,
    ErrorNotAString,
}

/// @implements REQ-DRT-PROTO.case_echoed
/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Heard {
    Answered {
        case_number: u64,
        output: Option<serde_json::Value>,
        error: Option<String>,
    },
    WrongCase { expected: u64, got: u64 },
    NotAReply { reason: NotAReply },
    NotExclusive { case_number: u64 },
}

/// @implements REQ-DRT-PROTO.reply_exclusive
/// @drt REQ-DRT-PROTO.reply_exclusive
pub fn is_exclusive(output: Option<serde_json::Value>, error: Option<String>) -> bool {
    output.is_some() != error.is_some()
}

/// @implements REQ-DRT-PROTO.reply_one_line
/// @implements REQ-DRT-PROTO.case_echoed
/// @implements REQ-DRT-PROTO.reply_exclusive
/// @drt REQ-DRT-PROTO.reply_one_line
/// @drt REQ-DRT-PROTO.case_echoed
pub fn hear(expected: u64, line: String) -> Heard {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return Heard::NotAReply { reason: NotAReply::NotJson };
    };
    let Some(fields) = value.as_object() else {
        return Heard::NotAReply { reason: NotAReply::NotAnObject };
    };
    let Some(got) = fields.get("case").and_then(|c| c.as_u64()) else {
        return Heard::NotAReply { reason: NotAReply::NoCaseNumber };
    };
    if got != expected {
        return Heard::WrongCase { expected, got };
    }
    let output = fields.get("output").cloned();
    let error = match fields.get("error") {
        None => None,
        Some(value) => match value.as_str() {
            None => return Heard::NotAReply { reason: NotAReply::ErrorNotAString },
            Some(message) => Some(message.to_string()),
        },
    };
    if is_exclusive(output.clone(), error.clone()) {
        Heard::Answered { case_number: got, output, error }
    } else {
        Heard::NotExclusive { case_number: got }
    }
}

/// @implements REQ-DRT-PROTO.ops_unique
/// @drt REQ-DRT-PROTO.ops_unique
pub fn duplicate_ops(ops: Vec<String>) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut dupes: BTreeSet<String> = BTreeSet::new();
    for op in ops {
        if !seen.insert(op.clone()) {
            dupes.insert(op);
        }
    }
    dupes.into_iter().collect()
}

/// @implements REQ-DRT-PROTO.case_one_line
/// @implements REQ-DRT-PROTO.case_names_op
/// @drt REQ-DRT-PROTO.case_one_line
/// @drt REQ-DRT-PROTO.case_names_op
pub fn case_line(number: u64, op: String, input: serde_json::Value) -> String {
    format!("{{\"case\":{number},\"op\":{},\"input\":{input}}}", serde_json::Value::from(op))
}

/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    CouldNotStart { reason: String },
    NoReplyInTime,
    PipeBroke { reason: String },
    Read { expected: u64, got: Option<String> },
}

/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    CannotStart { reason: String },
    TimedOut,
    Died { reason: String },
    Heard { conclusion: Heard },
}

/// @implements REQ-DRT-PROTO.failure_named
/// @drt REQ-DRT-PROTO.failure_named
pub fn outcome(event: Event) -> Outcome {
    match event {
        Event::CouldNotStart { reason } => Outcome::CannotStart { reason },
        Event::NoReplyInTime => Outcome::TimedOut,
        Event::PipeBroke { reason } => Outcome::Died { reason },
        Event::Read { got: None, .. } => Outcome::Died { reason: "closed its pipe".to_string() },
        Event::Read { expected, got: Some(line) } => Outcome::Heard { conclusion: hear(expected, line) },
    }
}

pub fn into_reply(outcome: Outcome, line: &str) -> Result<Reply, RunnerError> {
    match outcome {
        Outcome::CannotStart { reason } => Err(RunnerError::Spawn(reason)),
        Outcome::TimedOut => Err(RunnerError::Timeout),
        Outcome::Died { reason } => Err(RunnerError::Died(reason)),
        Outcome::Heard { conclusion } => match conclusion {
            Heard::Answered { case_number, output, error } => Ok(Reply { case: case_number, output, error }),
            Heard::WrongCase { expected, got } => Err(RunnerError::Protocol(format!(
                "runner answered case {got} when case {expected} was asked"
            ))),
            Heard::NotAReply { reason } => {
                Err(RunnerError::Protocol(format!("{reason:?}: {}", line.trim())))
            }
            Heard::NotExclusive { .. } => Err(RunnerError::Protocol(
                "a reply carried both an output and an error, or neither".to_string(),
            )),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub op: String,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    pub language: String,
    pub ops: Vec<String>,
}

/// @implements REQ-DRT-PROTO.runner_shared
/// @drt REQ-DRT-PROTO.runner_shared
pub fn shared_runners(placed: Vec<Placed>) -> Vec<Shared> {
    let mut by_language: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for p in placed {
        let ops = by_language.entry(p.language).or_default();
        if !ops.contains(&p.op) {
            ops.push(p.op);
        }
    }
    by_language.into_iter().map(|(language, ops)| Shared { language, ops }).collect()
}

/// @implements REQ-DRT.error_is_an_answer
/// @drt REQ-DRT.error_is_an_answer
pub fn agree(
    model: Option<serde_json::Value>,
    implementation: Option<serde_json::Value>,
) -> bool {
    match (model, implementation) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// @implements REQ-DRT.falsification_only
/// @drt REQ-DRT.falsification_only
pub fn drt_level(agreed: bool) -> crate::evidence::Level {
    if agreed {
        crate::evidence::Level::L3
    } else {
        crate::evidence::Level::L1
    }
}

/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerError {
    Spawn(String),
    Timeout,
    Died(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-DRT.falsification_only
    #[test]
    fn agreement_earns_testing_and_never_proof() {
        assert_eq!(drt_level(true), crate::evidence::Level::L3);
        assert_eq!(drt_level(false), crate::evidence::Level::L1);
    }

    /// @tests REQ-DRT-PROTO.reply_exclusive
    #[test]
    fn a_null_output_is_an_output() {
        let value = serde_json::json!({"case": 1, "output": null});
        let reply = Reply::from_json(&value).unwrap();
        assert!(reply.is_well_formed(), "null is an answer, not an absence");
        assert_eq!(reply.output, Some(serde_json::Value::Null));
    }

    /// @tests REQ-DRT-PROTO.reply_exclusive
    #[test]
    fn a_reply_with_neither_or_both_is_not_well_formed() {
        assert!(!Reply::from_json(&serde_json::json!({"case": 1})).unwrap().is_well_formed());
        assert!(!Reply::from_json(&serde_json::json!({"case": 1, "output": 1, "error": "x"}))
            .unwrap()
            .is_well_formed());
    }

    /// @tests REQ-DRT-PROTO.case_echoed
    #[test]
    fn a_reply_without_a_case_number_is_refused() {
        assert!(Reply::from_json(&serde_json::json!({"output": 1})).is_err());
    }

    /// @tests REQ-DRT-PROTO.case_one_line
    /// @tests REQ-DRT-PROTO.case_names_op
    #[test]
    fn a_case_is_one_line_naming_its_op() {
        let line = case_line(3, "REQ-X.c".into(), serde_json::json!({"t": "a\nb\r\n\t"}));
        assert!(!line.contains('\n') && !line.contains('\r'), "{line}");
        let read: Case = serde_json::from_str(&line).unwrap();
        assert_eq!(read.case, 3);
        assert_eq!(read.op, "REQ-X.c");
        assert_eq!(read.input, serde_json::json!({"t": "a\nb\r\n\t"}));
    }

    /// @tests REQ-DRT-PROTO.failure_named
    #[test]
    fn each_failure_is_named_and_none_is_an_answer() {
        let outcomes = [
            outcome(Event::CouldNotStart { reason: "no node".into() }),
            outcome(Event::NoReplyInTime),
            outcome(Event::Read { expected: 1, got: None }),
            outcome(Event::Read { expected: 1, got: Some("not json".into()) }),
        ];
        assert!(matches!(outcomes[0], Outcome::CannotStart { .. }));
        assert_eq!(outcomes[1], Outcome::TimedOut);
        assert!(matches!(outcomes[2], Outcome::Died { .. }));
        assert!(matches!(outcomes[3], Outcome::Heard { conclusion: Heard::NotAReply { .. } }));
        for o in outcomes {
            assert!(into_reply(o, "").is_err());
        }
        let answered = outcome(Event::Read { expected: 1, got: Some(r#"{"case":1,"output":2}"#.into()) });
        assert!(into_reply(answered, "").is_ok());
    }

    /// @tests REQ-DRT-PROTO.runner_shared
    #[test]
    fn one_runner_per_language_carries_every_op_of_it() {
        let placed = |op: &str, language: &str| Placed { op: op.into(), language: language.into() };
        let plan = shared_runners(vec![
            placed("b", "rust"),
            placed("a", "typescript"),
            placed("c", "rust"),
            placed("b", "rust"),
        ]);
        assert_eq!(
            plan,
            vec![
                Shared { language: "rust".into(), ops: vec!["b".into(), "c".into()] },
                Shared { language: "typescript".into(), ops: vec!["a".into()] },
            ]
        );
    }
}
