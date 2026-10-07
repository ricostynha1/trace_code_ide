//! Executing the judge's witness — what separates this from "ask a model if it
//! looks right".
//!
//! The judge's reasoning about *English* is trusted; its claims about *Lean*
//! are not. A drift verdict names a concrete input and asserts what the model
//! does with it. That assertion is run against the compiled model, and a
//! verdict whose claim is falsified is discarded rather than reported.

use std::time::Duration;

use crate::drt::protocol::{values_agree, Case, Reply, Runner, RunnerSpec};

use super::verdict::Witness;

/// The result of checking a witness against the model.
#[derive(Debug, Clone, PartialEq)]
pub enum WitnessCheck {
    /// The model behaved as the judge said. The disagreement with the
    /// requirement is real, and the input is worth keeping forever.
    Confirmed { model_reply: serde_json::Value },
    /// The model did something else. The judge misread the Lean.
    Falsified {
        claimed: serde_json::Value,
        actual: serde_json::Value,
    },
    /// The witness could not be run at all — no model runner configured, the
    /// input does not decode, the claim was prose only. Never counted as drift.
    Inexecutable(String),
}

impl WitnessCheck {
    pub fn is_confirmed(&self) -> bool {
        matches!(self, WitnessCheck::Confirmed { .. })
    }

    /// How the comparison was made, recorded in the evidence so a verdict can
    /// be re-read later without guessing what was compared.
    pub fn comparison_mode(&self) -> &'static str {
        match self {
            WitnessCheck::Confirmed { .. } | WitnessCheck::Falsified { .. } => "structural-json",
            WitnessCheck::Inexecutable(_) => "none",
        }
    }
}

/// Run a witness against the model runner.
pub fn check(
    witness: &Witness,
    model: Option<&RunnerSpec>,
    timeout: Duration,
) -> WitnessCheck {
    let Some(claimed) = witness.model_produces_json.clone() else {
        // A prose-only claim cannot gate an error-severity finding.
        return WitnessCheck::Inexecutable(
            "the witness gave no `model_produces_json`, so the claim about the model \
             cannot be checked by running it"
                .into(),
        );
    };

    let Some(spec) = model else {
        return WitnessCheck::Inexecutable(
            "no model runner is configured for this requirement, so the witness cannot be \
             executed"
                .into(),
        );
    };

    let mut runner = match Runner::spawn(spec) {
        Ok(r) => r,
        Err(e) => return WitnessCheck::Inexecutable(format!("model runner would not start: {e}")),
    };

    let case = Case {
        case: 0,
        op: witness.op.clone(),
        input: witness.input.clone(),
    };

    let reply = match runner.ask(&case, timeout) {
        Ok(reply) => reply,
        Err(e) => return WitnessCheck::Inexecutable(format!("model runner: {e}")),
    };

    let actual = reply_value(&reply);

    if values_agree(&claimed, &actual) {
        WitnessCheck::Confirmed { model_reply: actual }
    } else {
        WitnessCheck::Falsified { claimed, actual }
    }
}

/// A reply as a single comparable value: the output, or the error rendered so
/// that "the model rejects this" can be claimed and checked like any other
/// outcome.
fn reply_value(reply: &Reply) -> serde_json::Value {
    match (&reply.output, &reply.error) {
        (Some(output), _) => output.clone(),
        (None, Some(error)) => serde_json::json!({ "error": error }),
        (None, None) => serde_json::Value::Null,
    }
}
