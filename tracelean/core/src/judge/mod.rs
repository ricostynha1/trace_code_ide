//! The requirement ↔ model judge.
//!
//! Differential testing binds the model to the code by execution. Nothing can
//! bind the *requirement* to the model that way: there is no oracle for
//! English. So that bond gets a judge — and the judge gets an oracle bolted on,
//! because its claims about Lean are executed rather than believed
//! (`witness.rs`).
//!
//! What the judge is allowed to do is deliberately narrow:
//!
//! * `Agrees` writes L2 evidence and nothing more. Absence of a witness is not
//!   proof of absence, so it can never promote a link to conformant or proved.
//! * A drift verdict produces a finding **only** once its witness has been
//!   confirmed by execution.
//! * `Unmodelable` produces a proposal, never a mutation.

pub mod calibration;
pub mod prompt;
pub mod verdict;
pub mod witness;

use std::collections::BTreeMap;
use std::time::Duration;

use crate::ai::provider::{
    AiProvider, AiRequest, ChatMessage, MessageRole, ModelConfig,
};
use crate::drt::protocol::RunnerSpec;
use crate::trace::{Bond, EvidenceDetail, EvidenceKey, EvidenceRecord, Level};

pub use prompt::{PromptInput, VERSION};
pub use verdict::{Confidence, JudgeReply, ParseError, Verdict, Witness};
pub use witness::WitnessCheck;

/// Everything a single judgement produced.
#[derive(Debug, Clone)]
pub struct Judgement {
    pub reply: JudgeReply,
    /// `"api"` or `"pasted"` — see `EvidenceDetail::Judge::source`.
    pub source: String,
    pub check: WitnessCheck,
    /// True when no model runner was available, so the verdict could not be
    /// checked. Such a verdict never produces a drift finding.
    pub degraded: bool,
    pub model_id: String,
    pub prompt_version: String,
    pub cost_usd: f64,
    /// Two falsified witnesses in a row is a fact about the judge, not about
    /// the requirement.
    pub unreliable: bool,
}

impl Judgement {
    /// Whether this judgement establishes consistency.
    pub fn earns_l2(&self) -> bool {
        self.reply.verdict == Verdict::Agrees && !self.unreliable
    }

    /// Whether this should surface as confirmed drift.
    pub fn is_confirmed_drift(&self) -> bool {
        self.reply.verdict.is_drift() && self.check.is_confirmed() && !self.degraded
    }

    /// Map onto the evidence record the checker reads.
    pub fn to_evidence(
        &self,
        req_id: &str,
        clause: Option<&str>,
        input_hashes: BTreeMap<String, String>,
        node: Option<String>,
    ) -> EvidenceRecord {
        EvidenceRecord {
            key: EvidenceKey {
                req_id: req_id.to_string(),
                clause: clause.map(|c| c.to_string()),
                bond: Bond::RequirementModel,
            },
            // Only agreement earns L2; every other outcome records the fact
            // without claiming anything about consistency.
            level: if self.earns_l2() { Level::L2 } else { Level::L1 },
            input_hashes,
            detail: EvidenceDetail::Judge {
                verdict: self.reply.verdict.as_str().to_string(),
                prompt_version: self.prompt_version.clone(),
                model_id: self.model_id.clone(),
                witness_confirmed: match &self.check {
                    WitnessCheck::Confirmed { .. } => Some(true),
                    WitnessCheck::Falsified { .. } => Some(false),
                    WitnessCheck::Inexecutable(_) => None,
                },
                degraded: self.degraded,
                confidence: format!("{:?}", self.reply.confidence).to_lowercase(),
                comparison_mode: self.check.comparison_mode().to_string(),
                cost_usd: self.cost_usd,
                source: self.source.clone(),
            },
            at: chrono::Utc::now().to_rfc3339(),
            node,
        }
    }
}

/// Knobs a caller can set. Defaults are the reproducible ones.
pub struct JudgeOptions {
    /// Temperature 0 and a fixed token budget: a verdict is evidence, so it has
    /// to be reproducible.
    pub temperature: f32,
    pub max_tokens: u32,
    pub witness_timeout: Duration,
    /// Remaining spend, in USD. The provider does not enforce the session cap —
    /// it lives in the agent loop — so the judge checks it itself.
    pub budget_remaining: Option<f64>,
}

impl Default for JudgeOptions {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            max_tokens: 2048,
            witness_timeout: Duration::from_secs(10),
            budget_remaining: None,
        }
    }
}

#[derive(Debug)]
pub enum JudgeError {
    Provider(String),
    /// Parsing failed twice, including after a repair turn.
    Unparseable(String),
    /// The call would have exceeded the session spend cap.
    OverBudget { remaining: f64 },
}

impl std::fmt::Display for JudgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JudgeError::Provider(m) => write!(f, "provider: {m}"),
            JudgeError::Unparseable(m) => write!(f, "could not read the judge's reply: {m}"),
            JudgeError::OverBudget { remaining } => write!(
                f,
                "judging would exceed the session spend cap (${remaining:.4} left)"
            ),
        }
    }
}

/// Judge one clause.
///
/// Takes the provider rather than building one, so tests can supply a scripted
/// provider and the calibration suite can run without a network.
pub async fn judge_clause(
    provider: &dyn AiProvider,
    model: &ModelConfig,
    input: &PromptInput,
    model_runner: Option<&RunnerSpec>,
    options: &JudgeOptions,
) -> Result<Judgement, JudgeError> {
    if let Some(remaining) = options.budget_remaining {
        if remaining <= 0.0 {
            return Err(JudgeError::OverBudget { remaining });
        }
    }

    let mut config = model.clone();
    config.temperature = options.temperature;
    config.max_tokens = options.max_tokens;

    let mut messages = vec![
        ChatMessage {
            role: MessageRole::System,
            content: prompt::SYSTEM.to_string(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        },
        ChatMessage {
            role: MessageRole::User,
            content: prompt::render_user(input),
            tool_call_id: None,
            tool_calls: Vec::new(),
        },
    ];

    let mut cost = 0.0f64;
    let mut last_error: Option<ParseError> = None;
    let mut reply: Option<JudgeReply> = None;

    // At most one repair turn. The repair *appends the parse error*: retrying
    // the identical prompt at temperature 0 would reproduce the same text.
    for attempt in 0..2 {
        let request = AiRequest {
            model: config.clone(),
            messages: messages.clone(),
            stop: None,
            tools: None,
            dynamic_tools: None,
            cache_breakpoints: Vec::new(),
        };

        let response = provider
            .complete(&request)
            .await
            .map_err(|e| JudgeError::Provider(e.to_string()))?;

        cost += response
            .usage
            .estimate_cost(
                config.input_cost_per_m,
                config.output_cost_per_m,
                config.cached_input_cost_per_m,
                0.0,
            )
            .total_usd;

        match verdict::parse(&response.content, response.truncated) {
            Ok(parsed) => {
                reply = Some(parsed);
                break;
            }
            Err(e) => {
                last_error = Some(e.clone());
                if attempt == 0 {
                    messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: response.content.clone(),
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });
                    messages.push(ChatMessage {
                        role: MessageRole::User,
                        content: format!(
                            "That reply could not be used: {e}\n\nReply again with a single \
                             JSON object matching the schema, and nothing else."
                        ),
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });
                }
            }
        }
    }

    let Some(reply) = reply else {
        return Err(JudgeError::Unparseable(
            last_error.map(|e| e.to_string()).unwrap_or_default(),
        ));
    };

    // Only a drift claim has something to check.
    let check = match (&reply.witness, reply.verdict.is_drift()) {
        (Some(w), true) => witness::check(w, model_runner, options.witness_timeout),
        _ => WitnessCheck::Inexecutable("no witness to check".into()),
    };

    let degraded = model_runner.is_none() && reply.verdict.is_drift();

    Ok(Judgement {
        reply,
        source: "api".to_string(),
        check,
        degraded,
        model_id: config.model_id.clone(),
        prompt_version: VERSION.to_string(),
        cost_usd: cost,
        unreliable: false,
    })
}

/// Re-judge after a falsified witness.
///
/// A judge that misreads the Lean once may simply have been unlucky; twice in a
/// row is a fact about the judge on this input, and the right response is to say
/// so rather than to report a requirement problem that may not exist.
pub async fn judge_clause_checked(
    provider: &dyn AiProvider,
    model: &ModelConfig,
    input: &PromptInput,
    model_runner: Option<&RunnerSpec>,
    options: &JudgeOptions,
) -> Result<Judgement, JudgeError> {
    let first = judge_clause(provider, model, input, model_runner, options).await?;
    if !matches!(first.check, WitnessCheck::Falsified { .. }) {
        return Ok(first);
    }

    let mut second = judge_clause(provider, model, input, model_runner, options).await?;
    second.cost_usd += first.cost_usd;
    if matches!(second.check, WitnessCheck::Falsified { .. }) {
        second.unreliable = true;
    }
    Ok(second)
}

/// Build a provider from saved AI settings.
///
/// The agent loop has its own private builder tied to an `AgentContext`; the
/// judge runs outside that loop, so it needs its own — deliberately kept small
/// and returning a plain error rather than an agent error.
pub fn provider_from_settings(
    settings: &crate::AiSettings,
) -> Result<(ModelConfig, Box<dyn AiProvider + Send + Sync>), String> {
    let model = settings
        .selected_model
        .clone()
        .ok_or("No model selected. Pick one in the AI settings before judging.")?;

    let provider: Box<dyn AiProvider + Send + Sync> = match settings.active_provider {
        crate::ai::ProviderKind::OpenRouter => {
            let key = settings
                .openrouter_api_key
                .clone()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .ok_or("OpenRouter API key not set")?;
            Box::new(crate::ai::openrouter::OpenRouterProvider::new(key))
        }
        crate::ai::ProviderKind::Bedrock => {
            let token = settings
                .bedrock_api_key
                .clone()
                .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
                .ok_or("Bedrock bearer token not set")?;
            Box::new(crate::ai::bedrock::BedrockProvider::new(
                token,
                settings.bedrock_region.clone(),
            ))
        }
        crate::ai::ProviderKind::Mock => {
            let (p, _rx) = crate::ai::mock::MockProvider::new();
            Box::new(p)
        }
    };

    Ok((model, provider))
}

/// Assemble the prompt input for one clause from the trace index.
///
/// Pulls the clause text, the `@models` anchor's source, and the binding's
/// declared input schema — the last of which is what makes a witness
/// executable, and therefore what makes a drift verdict actionable.
pub fn prompt_input_for(
    root: &std::path::Path,
    index: &crate::trace::TraceIndex,
    req_id: &str,
    clause: Option<&str>,
) -> Result<(PromptInput, Option<RunnerSpec>), String> {
    let requirement = index
        .requirements
        .get(req_id)
        .ok_or_else(|| format!("No such requirement: {req_id}"))?;

    let clause_text = match clause {
        Some(key) => requirement
            .clauses
            .get(key)
            .cloned()
            .ok_or_else(|| format!("{req_id} has no clause `{key}`"))?,
        None => requirement.body.clone(),
    };

    let model_links: Vec<_> = index
        .links_for_clause(req_id, clause)
        .into_iter()
        .filter(|l| l.role == crate::trace::Role::Models)
        .collect();

    if model_links.is_empty() {
        return Err(format!(
            "Nothing carries `@models` for {req_id}{} — there is no formalization to judge.",
            clause.map(|c| format!(".{c}")).unwrap_or_default()
        ));
    }

    // Several definitions may jointly model a clause; show them all, in file
    // order, so the judge sees the whole formalization rather than a fragment.
    let mut sources = Vec::new();
    for link in &model_links {
        let path = root.join(&link.anchor.file);
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let text = slice_anchor(&content, link);
        sources.push(format!("-- {}\n{text}", link.anchor.file.display()));
    }

    let config = crate::drt::DrtConfig::load(root).unwrap_or_default();
    let binding = config.binding(req_id, clause);

    let previous_verdict = index
        .evidence
        .iter()
        .find(|r| {
            r.key.req_id == req_id
                && r.key.clause.as_deref() == clause
                && r.key.bond == Bond::RequirementModel
        })
        .and_then(|r| match &r.detail {
            EvidenceDetail::Judge { verdict, .. } => Some(verdict.clone()),
            _ => None,
        });

    let input = PromptInput {
        req_id: req_id.to_string(),
        clause_key: clause.map(|c| c.to_string()),
        clause_text,
        clause_text_before: None,
        model_source: sources.join("\n\n"),
        model_source_before: None,
        input_schema: binding
            .and_then(|b| serde_json::to_value(&b.input).ok()),
        op: binding.map(|b| b.op.clone()).unwrap_or_else(|| "default".into()),
        glossary: Vec::new(),
        previous_verdict,
    };

    Ok((input, binding.map(|b| b.model.clone())))
}

/// The source text an anchor covers.
fn slice_anchor(content: &str, link: &crate::trace::Link) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = link.anchor.start_line as usize;
    let end = (link.anchor.end_line as usize + 1).min(lines.len());
    if start >= lines.len() {
        return content.to_string();
    }
    lines[start..end].join("\n")
}
