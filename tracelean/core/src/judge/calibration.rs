//! The calibration suite: how a prompt change is known not to have made the
//! judge worse.
//!
//! A prompt is part of the evidence chain, so changing its wording changes what
//! every verdict means. The suite pins that down with pairs whose right answer
//! is known — including models deliberately mutated away from their clause.
//!
//! The gate is deliberately asymmetric: **zero false `agrees` on mutated
//! pairs**. A missed drift is silent and corrodes trust in every green badge; a
//! false alarm is merely annoying and gets resolved by a human in a minute.

use serde::{Deserialize, Serialize};

use super::verdict::Verdict;

/// One calibration case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub name: String,
    pub req_id: String,
    pub clause_key: Option<String>,
    pub clause_text: String,
    pub model_source: String,
    /// What a correct judge must say.
    pub expect: Expectation,
    /// How the model was mutated away from the clause, for the report.
    #[serde(default)]
    pub mutation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expectation {
    /// The model faithfully formalizes the clause.
    Agrees,
    /// The model has drifted; any drift verdict is acceptable.
    Drifts,
    /// The clause cannot be modeled at all.
    Unmodelable,
}

impl Expectation {
    pub fn satisfied_by(&self, verdict: Verdict) -> bool {
        match self {
            Expectation::Agrees => verdict == Verdict::Agrees,
            Expectation::Drifts => verdict.is_drift(),
            // Abstaining on an unmodelable clause is acceptable; claiming drift
            // about something the model never tried to say is not.
            Expectation::Unmodelable => {
                matches!(verdict, Verdict::Unmodelable | Verdict::Unclear)
            }
        }
    }
}

/// One fixture's outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureResult {
    pub name: String,
    pub expected: Expectation,
    pub actual: Verdict,
    pub passed: bool,
    /// The failure that matters: the judge said "agrees" about a model that had
    /// been deliberately broken.
    pub false_agreement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub prompt_version: String,
    pub model_id: String,
    pub results: Vec<FixtureResult>,
}

impl CalibrationReport {
    pub fn false_agreements(&self) -> usize {
        self.results.iter().filter(|r| r.false_agreement).count()
    }

    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    /// The gate. Only false agreement fails the suite.
    pub fn is_acceptable(&self) -> bool {
        self.false_agreements() == 0
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "# Judge calibration\n\nPrompt: `{}`  ·  Model: `{}`\n\n\
             {} of {} fixtures matched; **{} false agreements** \
             (the only failure that fails the suite).\n\n\
             | Fixture | Expected | Actual | Result |\n|---|---|---|---|\n",
            self.prompt_version,
            self.model_id,
            self.passed(),
            self.results.len(),
            self.false_agreements(),
        );
        for r in &self.results {
            out.push_str(&format!(
                "| {} | {:?} | {} | {} |\n",
                r.name,
                r.expected,
                r.actual.as_str(),
                if r.false_agreement {
                    "**FALSE AGREEMENT**"
                } else if r.passed {
                    "ok"
                } else {
                    "mismatch"
                }
            ));
        }
        out
    }
}

/// Score one fixture.
pub fn score(fixture: &Fixture, actual: Verdict) -> FixtureResult {
    let passed = fixture.expect.satisfied_by(actual);
    FixtureResult {
        name: fixture.name.clone(),
        expected: fixture.expect,
        actual,
        passed,
        false_agreement: fixture.expect == Expectation::Drifts && actual == Verdict::Agrees,
    }
}

/// The built-in fixtures, each a mutation of a clause/model pair that agrees.
///
/// Kept in code rather than on disk so a prompt change cannot be shipped with a
/// suite that quietly went missing.
pub fn builtin() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "threshold-agrees".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("pre".into()),
            clause_text: "Passwords must be at least 8 characters.".into(),
            model_source: "structure Password where\n  val : String\n  min_length : val.length ≥ 8"
                .into(),
            expect: Expectation::Agrees,
            mutation: None,
        },
        Fixture {
            name: "threshold-changed".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("pre".into()),
            clause_text: "Passwords must be at least 12 characters.".into(),
            model_source: "structure Password where\n  val : String\n  min_length : val.length ≥ 8"
                .into(),
            expect: Expectation::Drifts,
            mutation: Some("clause tightened to 12; model still says 8".into()),
        },
        Fixture {
            name: "comparison-flipped".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("post".into()),
            clause_text: "The account is locked after 5 consecutive failed attempts.".into(),
            model_source: "def locked (failures : Nat) : Bool := failures ≥ 3".into(),
            expect: Expectation::Drifts,
            mutation: Some("threshold 5 in the clause, 3 in the model".into()),
        },
        Fixture {
            name: "error-case-dropped".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("err".into()),
            clause_text: "Every failure is one of: weak password, invalid credentials, \
                          account locked, or rate limited."
                .into(),
            model_source: "inductive AuthError where\n  | weakPassword\n  | invalidCredentials"
                .into(),
            expect: Expectation::Drifts,
            mutation: Some("two of four error cases missing from the model".into()),
        },
        Fixture {
            name: "quantifier-flipped".into(),
            req_id: "REQ-SEARCH-02".into(),
            clause_key: Some("post".into()),
            clause_text: "Every returned result contains the query string.".into(),
            model_source: "def valid (results : List String) (q : String) : Bool :=\n  \
                           results.any (fun r => q.isPrefixOf r)"
                .into(),
            expect: Expectation::Drifts,
            mutation: Some("∀ in the clause, ∃ in the model".into()),
        },
        Fixture {
            name: "condition-negated".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("post".into()),
            clause_text: "A token is returned only when the credentials are valid.".into(),
            model_source: "def login (ok : Bool) : Option String :=\n  if ok then none else some \"t\""
                .into(),
            expect: Expectation::Drifts,
            mutation: Some("branches swapped".into()),
        },
        Fixture {
            name: "side-effect-unmodelable".into(),
            req_id: "REQ-AUTH-03".into(),
            clause_key: Some("log".into()),
            clause_text: "Failed login attempts are logged.".into(),
            model_source: "def login (c : Credentials) : Except AuthError AuthToken := \
                           if valid c then .ok ⟨\"t\"⟩ else .error .invalidCredentials"
                .into(),
            expect: Expectation::Unmodelable,
            mutation: None,
        },
    ]
}
