//! What the judge answers, and the rules that make the answer usable.
//!
//! The judge is asked one narrow question: does this Lean model still say what
//! this requirement clause says? Not whether the code is correct, not whether
//! the requirement is good, not whether the Lean is idiomatic. Keeping the
//! question narrow is most of what makes an LLM verdict trustworthy here — the
//! rest is refusing to take its word about Lean (see `witness.rs`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No drift found.
    Agrees,
    /// The model permits behaviour the clause forbids.
    UnderConstrained,
    /// The model forbids behaviour the clause permits.
    OverConstrained,
    Both,
    /// The clause cannot be expressed in this model at all — it is about
    /// logging, latency, deployment, or anything the model has no notion of.
    Unmodelable,
    /// Not enough information to say. A first-class answer: guessing is worse
    /// than abstaining, and this is how unmodelable or ambiguous clauses get
    /// discovered instead of being forced into a bad model.
    Unclear,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Agrees => "agrees",
            Verdict::UnderConstrained => "under_constrained",
            Verdict::OverConstrained => "over_constrained",
            Verdict::Both => "both",
            Verdict::Unmodelable => "unmodelable",
            Verdict::Unclear => "unclear",
        }
    }

    /// Verdicts that assert a disagreement, and therefore owe a witness.
    pub fn is_drift(&self) -> bool {
        matches!(
            self,
            Verdict::UnderConstrained | Verdict::OverConstrained | Verdict::Both
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    #[default]
    Medium,
    Low,
}

/// The concrete disagreement. Required for any drift verdict, because a judge
/// forced to exhibit a witness hallucinates far less than one allowed to assert
/// a conclusion — and because the witness is what gets executed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Witness {
    /// Entry point, matching the differential-testing binding.
    #[serde(default = "default_op")]
    pub op: String,
    /// The input, shaped to the binding's declared schema.
    pub input: serde_json::Value,
    /// What the clause demands, in prose — for the human reading the finding.
    pub clause_requires: String,
    /// What the model actually does, in prose — for the human.
    pub model_produces: String,
    /// The same claim as data, so it can be checked by execution rather than
    /// by reading. Without this the verdict cannot be confirmed.
    #[serde(default)]
    pub model_produces_json: Option<serde_json::Value>,
}

fn default_op() -> String {
    "default".into()
}

/// The judge's reply, as parsed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeReply {
    pub verdict: Verdict,
    #[serde(default)]
    pub witness: Option<Witness>,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub suggested_patch: Option<String>,
    #[serde(default)]
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// No JSON object found in the response at all.
    NoJson,
    /// JSON found but it does not match the expected shape.
    Shape(String),
    /// A drift verdict arrived without a witness, so it cannot be checked.
    DriftWithoutWitness,
    /// The provider stopped at max_tokens; the reply is a fragment.
    Truncated,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NoJson => write!(f, "no JSON object in the response"),
            ParseError::Shape(m) => write!(f, "reply did not match the expected shape: {m}"),
            ParseError::DriftWithoutWitness => write!(
                f,
                "a drift verdict must come with a witness — an input, what the clause requires, \
                 and what the model produces (including model_produces_json)"
            ),
            ParseError::Truncated => write!(f, "the response was cut off at the token limit"),
        }
    }
}

/// Parse a reply out of free model text.
///
/// The provider stack has no structured-output mode, so this strips code
/// fences and takes the first balanced JSON object rather than trusting the
/// model to emit bare JSON.
pub fn parse(text: &str, truncated: bool) -> Result<JudgeReply, ParseError> {
    if truncated {
        return Err(ParseError::Truncated);
    }
    let json = extract_object(text).ok_or(ParseError::NoJson)?;
    let reply: JudgeReply =
        serde_json::from_str(&json).map_err(|e| ParseError::Shape(e.to_string()))?;

    // Enforced in code rather than left to the prompt: a drift verdict that
    // cannot be executed is exactly the unverifiable assertion this design
    // exists to avoid.
    if reply.verdict.is_drift() && reply.witness.is_none() {
        return Err(ParseError::DriftWithoutWitness);
    }

    Ok(reply)
}

/// First balanced `{…}`, ignoring braces inside strings.
fn extract_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
