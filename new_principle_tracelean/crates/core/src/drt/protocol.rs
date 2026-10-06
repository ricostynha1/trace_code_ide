//! The conformance protocol: line-delimited JSON over stdin and stdout.
//!
//! This is the entire coupling between TraceLean and the code it checks, which
//! is what lets one side be a compiled Lean binary and the other a compiled
//! Rust binary without anything above noticing.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// One generated case, sent to both sides.
///
/// @implements REQ-DRT-PROTO.line_delimited
/// @implements REQ-DRT-PROTO.op_dispatch
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub case: u64,
    /// Which entry point to exercise. Unique project-wide, so that a shared
    /// runner's dispatch cannot have two arms with the same name.
    pub op: String,
    pub input: serde_json::Value,
}

/// What a runner answered.
///
/// Exactly one of `output` and `error` is present. An error on one side against
/// an output on the other is a divergence, not a failed run: both sides
/// rejecting an input is agreement.
///
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
    /// Read a reply from a parsed line.
    ///
    /// Written by hand rather than derived because a derived `Option` cannot
    /// tell an absent field from a present `null`, and `null` is a perfectly
    /// ordinary answer — a model returning `none` produces exactly that. The
    /// derived version reported such a reply as carrying neither an output nor
    /// an error, which is the protocol's one invariant, so a legitimate answer
    /// looked like a runner that could not speak.
    ///
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

    /// Whether this reply is well-formed: exactly one of output and error.
    ///
    /// A reply carrying both, or neither, is not a weaker answer — it is a
    /// runner that is not speaking the protocol, and the comparison above must
    /// not treat it as data.
    ///
    /// @implements REQ-DRT-PROTO.reply_exclusive
    pub fn is_well_formed(&self) -> bool {
        self.output.is_some() != self.error.is_some()
    }
}

/// Why a line is not a reply.
///
/// Named rather than a message: two implementations cannot be expected to
/// phrase a JSON error the same way, and a report saying `notAnObject` is more
/// use than one saying `expected value at line 1 column 1`.
///
/// @implements ARCH-HONEST.named_findings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotAReply {
    /// The line is not JSON at all.
    NotJson,
    /// It parsed, but into something other than an object.
    NotAnObject,
    /// No `case` field, or one that is not a number.
    NoCaseNumber,
    /// An `error` field that is not a string.
    ErrorNotAString,
}

/// What reading one line from a runner concluded.
///
/// `WrongCase` is the conclusion the first version of this could not reach: it
/// never compared the number it got back with the number it asked, so a runner
/// one reply behind was read as answering the current case.
///
/// Serialised with every field present — including `null` for an absent output
/// — because that is what the model's derived encoding produces, and the two
/// have to be the same value for a differential test to mean anything.
///
/// @implements REQ-DRT-PROTO.case_echoed
/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Heard {
    /// A well-formed reply to the case that was asked.
    Answered {
        /// Spelled `caseNumber` rather than `case` because the model's language
        /// reserves the word, and a name escaped there is a name the grammar
        /// this project parses Lean with cannot read (ADR-0008).
        case_number: u64,
        output: Option<serde_json::Value>,
        error: Option<String>,
    },
    /// A well-formed reply to a different case.
    WrongCase { expected: u64, got: u64 },
    /// Not a reply.
    NotAReply { reason: NotAReply },
    /// Both an output and an error, or neither.
    NotExclusive { case_number: u64 },
}

/// Whether a reply carries exactly one of an output and an error.
///
/// `null` is an ordinary answer — a model returning `None` produces exactly
/// that — so presence is about the field being there, not about it being
/// non-null.
///
/// @implements REQ-DRT-PROTO.reply_exclusive
pub fn is_exclusive(output: &Option<serde_json::Value>, error: &Option<String>) -> bool {
    output.is_some() != error.is_some()
}

/// Read one line of a runner's output, given the case that was asked.
///
/// @implements REQ-DRT-PROTO.line_delimited
/// @implements REQ-DRT-PROTO.case_echoed
/// @implements REQ-DRT-PROTO.reply_exclusive
/// @implements REQ-DRT-PROTO.failure_named
/// @drt REQ-DRT-PROTO.line_delimited
/// @drt REQ-DRT-PROTO.case_echoed
/// @drt REQ-DRT-PROTO.reply_exclusive
/// @drt REQ-DRT-PROTO.failure_named
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
    if is_exclusive(&output, &error) {
        Heard::Answered { case_number: got, output, error }
    } else {
        Heard::NotExclusive { case_number: got }
    }
}

/// The ops that appear more than once, sorted and without repeats.
///
/// One runner process serves every binding of its language, so its dispatch is
/// a table keyed by op. Two bindings with the same op means the first arm wins
/// and the second requirement is silently answered by the wrong function.
///
/// @implements REQ-DRT-PROTO.op_dispatch
/// @implements REQ-DRT-PROTO.runner_shared
/// @drt REQ-DRT-PROTO.op_dispatch
/// @drt REQ-DRT-PROTO.runner_shared
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

/// Two replies agree when they are the same answer.
///
/// An error on one side against an output on the other is a divergence, not an
/// aborted run. Both sides rejecting an input is *agreement*: the model
/// refusing what the implementation also refuses is the two behaving alike, and
/// treating it as a failed run would make every partial function untestable.
///
/// The error text is not compared. Two implementations cannot be expected to
/// phrase a refusal the same way, and the phrasing is not the behaviour.
///
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

/// The level a differential run establishes.
///
/// A run that found no disagreement is evidence at L3 and never higher. It has
/// failed to falsify, which is not the same as having proved: the next case
/// might have disagreed, and the ladder has a separate rung for a proof
/// precisely so that the difference stays visible.
///
/// @implements REQ-DRT.falsification_only
/// @drt REQ-DRT.falsification_only
pub fn drt_level(agreed: bool) -> crate::evidence::Level {
    if agreed {
        crate::evidence::Level::L3
    } else {
        crate::evidence::Level::L1
    }
}

/// Why a runner could not answer.
///
/// The four ways mean different things — a missing toolchain is not a timeout,
/// and neither is a crash on a particular input, which is a result.
///
/// @implements REQ-DRT-PROTO.failure_named
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerError {
    /// The command could not be started at all.
    Spawn(String),
    /// No reply within the per-case timeout.
    Timeout,
    /// The process exited or closed its pipe.
    Died(String),
    /// A line came back that is not a reply.
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

    /// A model returning `none` answers `null`, which is an answer.
    ///
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
}
