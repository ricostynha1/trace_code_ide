//! Amazon Bedrock provider — Chat Completions API on bedrock-mantle.
//!
//! Uses `bedrock-mantle.{region}.api.aws/v1/chat/completions`
//! with Bearer token authentication (Bedrock API key).
//!
//! Tool definitions are embedded as text in the system prompt (not the `tools` param)
//! because Bedrock's proxy does NOT cache the `tools` field for MiniMax models.
//! The model responds with tool calls using <tool_call> tags (Hermes format)
//! or [TOOL_CALLS] prefix (Mistral format). Both are parsed.

use super::provider::*;
use super::tracking::TokenUsage;
use super::bedrock_pricing::bedrock_pricing;
use serde::{Deserialize, Serialize};

/// Default region for Bedrock API.
pub const DEFAULT_REGION: &str = "eu-west-1";
/// Default model ID.
pub const DEFAULT_MODEL_ID: &str = "eu.amazon.nova-lite-v1:0";

pub struct BedrockProvider {
    bearer_token: String,
    region: String,
    client: reqwest::Client,
    max_retries: u32,
    pub verbose: bool,
}

impl BedrockProvider {
    pub fn new(bearer_token: String, region: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            bearer_token,
            region: region.unwrap_or_else(|| DEFAULT_REGION.into()),
            client,
            max_retries: 3,
            verbose: false,
        }
    }

    /// Chat Completions endpoint on bedrock-mantle.
    fn endpoint(&self) -> String {
        format!(
            "https://bedrock-mantle.{}.api.aws/v1/chat/completions",
            self.region
        )
    }
}

// ---------------------------------------------------------------------------
// Tool embedding: render tool schemas as text for the system prompt
// ---------------------------------------------------------------------------

/// Render tool schemas as a text block to embed in system prompt.
/// Uses a format close to MiniMax-M2.5 training data but with Hermes-style output tags.
/// The `<tools>` section matches MiniMax's internal chat template format for tool definitions.
fn render_tools_as_text(tools: &[ToolSchema]) -> String {
    let mut out = String::from("# Tools\n");
    out.push_str("You may call one or more tools to assist with the user query.\n\n");
    out.push_str("Available tools:\n\n");

    for tool in tools {
        let params_str = serde_json::to_string(&tool.function.parameters).unwrap_or_else(|_| "{}".into());
        out.push_str(&format!(
            "- **{}**: {} | Parameters: {}\n",
            tool.function.name, tool.function.description, params_str
        ));
    }

    out.push_str("\nTo call tools, respond with one or more <tool_call> blocks:\n\n");
    out.push_str("<tool_call>\n");
    out.push_str("{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/main.py\"}}\n");
    out.push_str("</tool_call>\n");
    out.push_str("<tool_call>\n");
    out.push_str("{\"name\": \"list_directory\", \"arguments\": {\"path\": \".\"}}\n");
    out.push_str("</tool_call>\n\n");
    out.push_str("Return ALL independent calls in ONE response. Do NOT call one tool then wait.\n");

    out
}

/// Result of parsing tool blocks: successful calls + error messages for malformed ones.
struct ParsedToolCalls {
    calls: Vec<ToolCallResponse>,
    /// Error descriptions for malformed tool calls (fed back to model)
    errors: Vec<String>,
}

/// Parse tool calls from model output. Supports:
/// 1. Hermes format: <tool_call>{"name": "...", "arguments": {...}}</tool_call>
/// 2. Mistral format: [TOOL_CALLS] [{"name": "...", "arguments": {...}}, ...]
///    or [TOOL_CALL] followed by JSON lines
/// 3. MiniMax native: <minimax:tool_call><invoke name="..."><parameter name="...">...</invoke></minimax:tool_call>
fn parse_tool_blocks(content: &str) -> ParsedToolCalls {
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    let mut idx = 0u32;

    // Strategy 1: <tool_call> ... </tool_call> tags (Hermes format)
    let mut search_from = 0;
    loop {
        let opener = if let Some(pos) = content[search_from..].find("<tool_call>") {
            search_from + pos
        } else {
            break;
        };

        let body_start = opener + "<tool_call>".len();

        let closer = if let Some(pos) = content[body_start..].find("</tool_call>") {
            body_start + pos
        } else {
            break;
        };

        let body = content[body_start..closer].trim();
        parse_json_body(body, &mut calls, &mut errors, &mut idx);
        search_from = closer + "</tool_call>".len();
    }

    // Strategy 2: [TOOL_CALLS] or [TOOL_CALL] prefix (Mistral-style)
    if calls.is_empty() && errors.is_empty() {
        // Case-insensitive search for [TOOL_CALL] or [TOOL_CALLS]
        let upper = content.to_uppercase();
        if let Some(pos) = upper.find("[TOOL_CALL") {
            // Skip past the tag and any trailing ] or S]
            let after_tag = content[pos..].find(']').map(|p| pos + p + 1).unwrap_or(pos + 11);
            let body = content[after_tag..].trim();

            // Try as JSON array first: [{"name": ...}, {"name": ...}]
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(body) {
                for item in &arr {
                    match extract_tool_call(item, &mut idx) {
                        Ok(tc) => calls.push(tc),
                        Err(e) => errors.push(e),
                    }
                }
            } else {
                // Try line-by-line JSON
                parse_json_body(body, &mut calls, &mut errors, &mut idx);
            }
        }
    }

    // Strategy 3: MiniMax native format — <minimax:tool_call><invoke name="...">...</invoke></minimax:tool_call>
    if calls.is_empty() && errors.is_empty() {
        parse_minimax_native(content, &mut calls, &mut errors, &mut idx);
    }

    ParsedToolCalls { calls, errors }
}

/// Parse a text body as JSON tool calls — single object or one per line.
fn parse_json_body(body: &str, calls: &mut Vec<ToolCallResponse>, errors: &mut Vec<String>, idx: &mut u32) {
    // Try as single JSON object
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
        match extract_tool_call(&parsed, idx) {
            Ok(tc) => calls.push(tc),
            Err(e) => errors.push(e),
        }
        return;
    }
    // Try each line
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(parsed) => match extract_tool_call(&parsed, idx) {
                Ok(tc) => calls.push(tc),
                Err(e) => errors.push(e),
            },
            Err(e) => {
                let truncated = if line.len() > 120 { format!("{}...", &line[..120]) } else { line.to_string() };
                errors.push(format!(
                    "Failed call: `{}` — invalid JSON: {}",
                    truncated, e
                ));
            }
        }
    }
}

/// Parse MiniMax native format:
/// <minimax:tool_call>
///   <invoke name="tool_name">
///     <parameter name="param1">value1</parameter>
///     <parameter name="param2">value2</parameter>
///   </invoke>
/// </minimax:tool_call>
fn parse_minimax_native(content: &str, calls: &mut Vec<ToolCallResponse>, errors: &mut Vec<String>, idx: &mut u32) {
    const OPEN_TAG: &str = "<minimax:tool_call>";
    const CLOSE_TAG: &str = "</minimax:tool_call>";

    let mut search_from = 0;
    loop {
        let opener = match content[search_from..].find(OPEN_TAG) {
            Some(pos) => search_from + pos,
            None => break,
        };
        let body_start = opener + OPEN_TAG.len();
        let closer = match content[body_start..].find(CLOSE_TAG) {
            Some(pos) => body_start + pos,
            None => break,
        };

        let block = &content[body_start..closer];

        // Parse each <invoke name="...">...</invoke> within the block
        let mut invoke_from = 0;
        loop {
            let invoke_start = match block[invoke_from..].find("<invoke") {
                Some(pos) => invoke_from + pos,
                None => break,
            };
            let invoke_end = match block[invoke_start..].find("</invoke>") {
                Some(pos) => invoke_start + pos,
                None => break,
            };

            let invoke_body = &block[invoke_start..invoke_end];

            // Extract name from <invoke name="tool_name">
            let func_name = extract_attribute_value(invoke_body, "name");
            if func_name.is_empty() {
                errors.push("MiniMax invoke: missing function name in <invoke> tag.".into());
                invoke_from = invoke_end + "</invoke>".len();
                continue;
            }

            // Extract parameters
            let mut arguments = serde_json::Map::new();
            let mut param_from = 0;
            loop {
                let param_start = match invoke_body[param_from..].find("<parameter") {
                    Some(pos) => param_from + pos,
                    None => break,
                };
                let param_end = match invoke_body[param_start..].find("</parameter>") {
                    Some(pos) => param_start + pos,
                    None => break,
                };

                let param_tag = &invoke_body[param_start..param_end];

                // Extract parameter name
                let pname = extract_attribute_value(param_tag, "name");
                // Extract parameter value (everything after the closing >)
                let value_start = match param_tag.find('>') {
                    Some(pos) => pos + 1,
                    None => {
                        param_from = param_end + "</parameter>".len();
                        continue;
                    }
                };
                let raw_value = param_tag[value_start..].trim();

                // Try to parse value as JSON (for arrays, objects, numbers, bools)
                let json_value = if raw_value.starts_with('[') || raw_value.starts_with('{') {
                    serde_json::from_str(raw_value).unwrap_or_else(|_| serde_json::Value::String(raw_value.to_string()))
                } else if raw_value == "true" {
                    serde_json::Value::Bool(true)
                } else if raw_value == "false" {
                    serde_json::Value::Bool(false)
                } else if raw_value == "null" {
                    serde_json::Value::Null
                } else if let Ok(n) = raw_value.parse::<i64>() {
                    serde_json::Value::Number(n.into())
                } else if let Ok(n) = raw_value.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::Value::String(raw_value.to_string())
                };

                if !pname.is_empty() {
                    arguments.insert(pname, json_value);
                }
                param_from = param_end + "</parameter>".len();
            }

            let id = format!("call_{}", idx);
            *idx += 1;
            calls.push(ToolCallResponse {
                id,
                call_type: "function".into(),
                function: ToolCallFunction {
                    name: func_name,
                    arguments: serde_json::to_string(&serde_json::Value::Object(arguments))
                        .unwrap_or_else(|_| "{}".into()),
                },
            });

            invoke_from = invoke_end + "</invoke>".len();
        }

        search_from = closer + CLOSE_TAG.len();
    }
}

/// Extract an attribute value from an XML-like tag: name="value" or name='value' or name=value
fn extract_attribute_value(tag: &str, attr: &str) -> String {
    let search = format!("{}=", attr);
    let pos = match tag.find(&search) {
        Some(p) => p + search.len(),
        None => return String::new(),
    };
    let rest = &tag[pos..];
    if rest.starts_with('"') {
        let end = rest[1..].find('"').map(|p| p + 1).unwrap_or(rest.len());
        rest[1..end].to_string()
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'').map(|p| p + 1).unwrap_or(rest.len());
        rest[1..end].to_string()
    } else {
        let end = rest.find(|c: char| c == '>' || c.is_whitespace()).unwrap_or(rest.len());
        rest[..end].to_string()
    }
}

/// Extract a ToolCallResponse from a parsed JSON value.
/// Returns Err(String) with a helpful message if the format is wrong.
fn extract_tool_call(parsed: &serde_json::Value, idx: &mut u32) -> Result<ToolCallResponse, String> {
    let name = parsed.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        let raw = serde_json::to_string(parsed).unwrap_or_else(|_| "???".into());
        let truncated = if raw.len() > 120 { format!("{}...", &raw[..120]) } else { raw };
        return Err(format!(
            "Failed call: `{}` — missing required \"name\" field.",
            truncated
        ));
    }

    let arguments = parsed.get("arguments")
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|| "{}".into());

    let id = format!("call_{}", idx);
    *idx += 1;
    Ok(ToolCallResponse {
        id,
        call_type: "function".into(),
        function: ToolCallFunction { name, arguments },
    })
}

/// Strip tool call blocks from content (both <tool_call> tags and [TOOL_CALL] sections).
fn strip_tool_blocks(content: &str) -> String {
    let mut result = content.to_string();

    // Strip <tool_call>...</tool_call> tags (Hermes format)
    loop {
        let opener = if let Some(pos) = result.find("<tool_call>") { pos } else { break };
        let closer = if let Some(pos) = result[opener..].find("</tool_call>") {
            opener + pos + "</tool_call>".len()
        } else {
            break;
        };
        // Also strip trailing newline
        let end = if closer < result.len() && result.as_bytes()[closer] == b'\n' { closer + 1 } else { closer };
        result.replace_range(opener..end, "");
    }

    // Strip <minimax:tool_call>...</minimax:tool_call> tags (MiniMax native format)
    loop {
        let opener = if let Some(pos) = result.find("<minimax:tool_call>") { pos } else { break };
        let closer = if let Some(pos) = result[opener..].find("</minimax:tool_call>") {
            opener + pos + "</minimax:tool_call>".len()
        } else {
            break;
        };
        let end = if closer < result.len() && result.as_bytes()[closer] == b'\n' { closer + 1 } else { closer };
        result.replace_range(opener..end, "");
    }

    // Strip [TOOL_CALL...] prefix and everything after it (it's always at the end)
    let upper = result.to_uppercase();
    if let Some(pos) = upper.find("[TOOL_CALL") {
        result.truncate(pos);
    }

    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Chat Completions request/response types (OpenAI-compatible)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMsg>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ChatMsg {
    role: String,
    content: String,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Option<Vec<ChatChoice>>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: Option<ChatChoiceMessage>,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ChatChoiceMessage {
    content: Option<String>,
    reasoning: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ChatUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize, Debug, Default)]
struct PromptTokensDetails {
    cached_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Provider implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl AiProvider for BedrockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        // Build messages: embed tools in system prompt, flatten tool_call/tool history
        let messages = self.build_messages(request);

        // Verbose: log the ACTUAL payload being sent to the API
        if self.verbose {
            eprintln!("\n--- [bedrock] ACTUAL PAYLOAD ({} messages) ---", messages.len());
            // Show which tools are available (embedded in system prompt)
            if let Some(ref tools) = request.tools {
                let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
                eprintln!("[tools: {}]", names.join(", "));
            }
            for (i, m) in messages.iter().enumerate() {
                let preview = if m.content.len() > 800 {
                    format!("{}… ({} chars)", &m.content[..800], m.content.len())
                } else {
                    m.content.clone()
                };
                eprintln!("[{}] role={}\n{}\n", i, m.role, preview);
            }
            eprintln!("--- [bedrock] END PAYLOAD ---\n");
        }

        let body = ChatRequest {
            model: request.model.model_id.clone(),
            messages,
            max_tokens: request.model.max_tokens,
            temperature: request.model.temperature,
            stop: request.stop.clone(),
        };

        let url = self.endpoint();

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let resp = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.bearer_token))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    let raw_text = r.text().await.unwrap_or_default();

                    if status == 429 {
                        last_err = Some(AiError {
                            kind: AiErrorKind::RateLimit,
                            message: format!("Throttled (attempt {})", attempt + 1),
                            retryable: true,
                        });
                        continue;
                    }

                    if status == 401 || status == 403 {
                        return Err(AiError {
                            kind: AiErrorKind::Authentication,
                            message: format!("Auth failed ({}): {}", status, &raw_text[..raw_text.len().min(300)]),
                            retryable: false,
                        });
                    }

                    if !status.is_success() {
                        let retryable = status.is_server_error();
                        last_err = Some(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("HTTP {}: {}", status, &raw_text[..raw_text.len().min(400)]),
                            retryable,
                        });
                        if !retryable { return Err(last_err.unwrap()); }
                        continue;
                    }

                    // Parse response
                    let parsed: ChatResponse = serde_json::from_str(&raw_text).map_err(|e| AiError {
                        kind: AiErrorKind::ProviderError,
                        message: format!("Parse error: {} | body: {}", e, &raw_text[..raw_text.len().min(400)]),
                        retryable: false,
                    })?;

                    let choice = parsed.choices.as_ref()
                        .and_then(|c| c.first())
                        .ok_or(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("No choices in response | body: {}", &raw_text[..raw_text.len().min(400)]),
                            retryable: false,
                        })?;

                    let message = choice.message.as_ref().ok_or(AiError {
                        kind: AiErrorKind::ProviderError,
                        message: "No message in choice".into(),
                        retryable: false,
                    })?;

                    let raw_content = message.content.clone().unwrap_or_default();
                    let truncated = choice.finish_reason.as_deref() == Some("length");

                    // Parse tool calls from content (<tool_call> or [TOOL_CALL] formats)
                    let tool_parsed = parse_tool_blocks(&raw_content);

                    // Strip tool blocks from content (leave only natural text)
                    let mut content = if !tool_parsed.calls.is_empty() || !tool_parsed.errors.is_empty() {
                        strip_tool_blocks(&raw_content)
                    } else {
                        raw_content
                    };

                    // If there were parse errors, append them to content so runtime
                    // sees them and can feed back to the model on next iteration
                    if !tool_parsed.errors.is_empty() {
                        let mut error_msg = String::from("\n\n[TOOL_CALL_ERROR]\n");
                        for err in &tool_parsed.errors {
                            error_msg.push_str(&format!("- {}\n", err));
                        }
                        error_msg.push_str("\nExpected format:\n");
                        error_msg.push_str("<tool_call>\n{\"name\": \"<tool_name>\", \"arguments\": {<params>}}\n</tool_call>\n");
                        content.push_str(&error_msg);
                    }

                    let tool_calls = tool_parsed.calls;

                    // Token usage
                    let usage = parsed.usage.as_ref();
                    let cached = usage
                        .and_then(|u| u.prompt_tokens_details.as_ref())
                        .and_then(|d| d.cached_tokens)
                        .unwrap_or(0);

                    // Count reasoning/thinking tokens if present
                    let thinking_tokens = message.reasoning.as_ref()
                        .map(|r| (r.len() as u32) / 4) // rough estimate: ~4 chars per token
                        .unwrap_or(0);

                    let token_usage = TokenUsage {
                        input_tokens: usage.and_then(|u| u.prompt_tokens).unwrap_or(0),
                        output_tokens: usage.and_then(|u| u.completion_tokens).unwrap_or(0),
                        thinking_tokens,
                        cached_tokens: cached,
                    };

                    return Ok(AiResponse {
                        content,
                        usage: token_usage,
                        raw_response: Some(raw_text),
                        truncated,
                        tool_calls,
                    });
                }
                Err(e) => {
                    let kind = if e.is_timeout() {
                        AiErrorKind::Timeout
                    } else {
                        AiErrorKind::Network
                    };
                    last_err = Some(AiError {
                        kind,
                        message: e.to_string(),
                        retryable: true,
                    });
                }
            }
        }

        Err(last_err.unwrap_or(AiError {
            kind: AiErrorKind::Network,
            message: "All retries exhausted".into(),
            retryable: false,
        }))
    }

    fn name(&self) -> &str {
        "Amazon Bedrock (Mantle)"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        let url = format!(
            "https://bedrock-mantle.{}.api.aws/v1/models",
            self.region
        );

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.bearer_token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AiError {
                kind: AiErrorKind::Network,
                message: e.to_string(),
                retryable: true,
            })?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(AiError {
                kind: AiErrorKind::Authentication,
                message: format!("Auth failed fetching models ({})", resp.status()),
                retryable: false,
            });
        }

        if !resp.status().is_success() {
            return Err(AiError {
                kind: AiErrorKind::ProviderError,
                message: format!("HTTP {} fetching models", resp.status()),
                retryable: false,
            });
        }

        let body: ListModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        let models = body.data.into_iter()
            .filter(|m| m.status.as_deref() != Some("unavailable"))
            .map(|m| {
                let (input_cost, output_cost) = bedrock_pricing(&m.id);
                let mut model = ModelConfig {
                    provider: ProviderKind::Bedrock,
                    model_id: m.id.clone(),
                    display_name: m.id.clone(),
                    max_tokens: 4096,
                    temperature: 0.3,
                    input_cost_per_m: input_cost,
                    output_cost_per_m: output_cost,
                    cached_input_cost_per_m: input_cost * 0.1,
                    extra_params: None,
                    coding_index: None,
                    coding_rank: None,
                    supports_caching: false,
                    supports_tools: true,
                };
                super::model_catalog::enrich(&mut model);
                model
            })
            .collect();

        Ok(models)
    }
}

// ---------------------------------------------------------------------------
// Message building: embed tools in system, flatten tool history as text
// ---------------------------------------------------------------------------

impl BedrockProvider {
    /// Build the message array for Chat Completions.
    /// - Tools are embedded as text at the END of the system prompt.
    /// - Assistant messages with tool_calls are rendered as text with ```tool blocks.
    /// - Tool result messages are rendered as user messages with tool output.
    fn build_messages(&self, request: &AiRequest) -> Vec<ChatMsg> {
        let tools_text = request.tools.as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| render_tools_as_text(t));

        let mut out: Vec<ChatMsg> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    // Append tool definitions to system prompt
                    let mut content = msg.content.clone();
                    if let Some(ref tt) = tools_text {
                        content.push_str("\n\n");
                        content.push_str(tt);
                    }
                    out.push(ChatMsg { role: "system".into(), content });
                }
                MessageRole::User => {
                    out.push(ChatMsg { role: "user".into(), content: msg.content.clone() });
                }
                MessageRole::Assistant => {
                    // Reconstruct assistant message: text + tool calls as <tool_call> tags
                    let mut content = msg.content.clone();
                    if !msg.tool_calls.is_empty() {
                        for tc in &msg.tool_calls {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str("<tool_call>\n");
                            let call_json = serde_json::json!({
                                "name": tc.function.name,
                                "arguments": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                    .unwrap_or(serde_json::Value::Object(Default::default()))
                            });
                            content.push_str(&serde_json::to_string(&call_json).unwrap_or_default());
                            content.push_str("\n</tool_call>");
                        }
                    }
                    out.push(ChatMsg { role: "assistant".into(), content });
                }
                MessageRole::Tool => {
                    // Tool results → user message with label
                    let tool_id = msg.tool_call_id.as_deref().unwrap_or("unknown");
                    let content = format!("[tool_result id={}]\n{}", tool_id, msg.content);
                    out.push(ChatMsg { role: "user".into(), content });
                }
            }
        }

        // If no system message was in the input but we have tools, prepend one
        if tools_text.is_some() && !request.messages.iter().any(|m| m.role == MessageRole::System) {
            if let Some(tt) = tools_text {
                out.insert(0, ChatMsg { role: "system".into(), content: tt });
            }
        }

        // Append batching reminder to the last user/tool message.
        // Positioned at the very end of context so the model sees it right before generating.
        if request.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) {
            if let Some(last) = out.iter_mut().rev().find(|m| m.role == "user") {
                last.content.push_str("\n\n[IMPORTANT: Return ALL independent tool calls in ONE response. Do NOT call one tool then wait.]");
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Model listing types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    status: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hermes_single() {
        let content = "Let me read that.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"/tmp/test.txt\"}}\n</tool_call>\nDone.";
        let parsed = parse_tool_blocks(content);
        assert_eq!(parsed.calls.len(), 1);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.calls[0].function.name, "read_file");
        assert_eq!(parsed.calls[0].id, "call_0");
    }

    #[test]
    fn test_parse_hermes_multiple() {
        let content = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"/a\"}}\n</tool_call>\n<tool_call>\n{\"name\": \"list_directory\", \"arguments\": {\"path\": \".\"}}\n</tool_call>";
        let parsed = parse_tool_blocks(content);
        assert_eq!(parsed.calls.len(), 2);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.calls[0].function.name, "read_file");
        assert_eq!(parsed.calls[1].function.name, "list_directory");
    }

    #[test]
    fn test_parse_mistral_array() {
        let content = "[TOOL_CALLS] [{\"name\": \"read_file\", \"arguments\": {\"path\": \"/a\"}}, {\"name\": \"run_shell\", \"arguments\": {\"command\": \"ls\"}}]";
        let parsed = parse_tool_blocks(content);
        assert_eq!(parsed.calls.len(), 2);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.calls[0].function.name, "read_file");
        assert_eq!(parsed.calls[1].function.name, "run_shell");
    }

    #[test]
    fn test_parse_tool_call_prefix_lines() {
        let content = "[TOOL_CALL]\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"/a\"}}\n{\"name\": \"run_shell\", \"arguments\": {\"command\": \"ls\"}}";
        let parsed = parse_tool_blocks(content);
        assert_eq!(parsed.calls.len(), 2);
        assert!(parsed.errors.is_empty());
    }

    #[test]
    fn test_parse_none() {
        let content = "Just a regular response.";
        let parsed = parse_tool_blocks(content);
        assert!(parsed.calls.is_empty());
        assert!(parsed.errors.is_empty());
    }

    #[test]
    fn test_parse_code_fence_not_tool() {
        let content = "```python\nprint('hello')\n```";
        let parsed = parse_tool_blocks(content);
        assert!(parsed.calls.is_empty());
        assert!(parsed.errors.is_empty());
    }

    #[test]
    fn test_parse_wrong_key() {
        let content = "<tool_call>\n{\"toolname\": \"read_file\", \"arguments\": {\"path\": \"/a\"}}\n</tool_call>";
        let parsed = parse_tool_blocks(content);
        assert!(parsed.calls.is_empty());
        assert_eq!(parsed.errors.len(), 1);
        assert!(parsed.errors[0].contains("missing required \"name\" field"));
    }

    #[test]
    fn test_parse_mixed_valid_invalid() {
        let content = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"/a\"}}\n</tool_call>\n<tool_call>\n{\"toolname\": \"bad\"}\n</tool_call>";
        let parsed = parse_tool_blocks(content);
        assert_eq!(parsed.calls.len(), 1);
        assert_eq!(parsed.errors.len(), 1);
    }

    #[test]
    fn test_strip_hermes() {
        let content = "Hello.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {}}\n</tool_call>\nBye.";
        let stripped = strip_tool_blocks(content);
        assert!(stripped.contains("Hello."));
        assert!(stripped.contains("Bye."));
        assert!(!stripped.contains("tool_call"));
        assert!(!stripped.contains("read_file"));
    }

    #[test]
    fn test_strip_mistral() {
        let content = "Thinking...\n[TOOL_CALLS] [{\"name\": \"x\", \"arguments\": {}}]";
        let stripped = strip_tool_blocks(content);
        assert_eq!(stripped, "Thinking...");
    }

    #[test]
    fn test_render_tools_as_text() {
        let tools = vec![ToolSchema {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            },
        }];
        let text = render_tools_as_text(&tools);
        assert!(text.contains("# Tools"));
        assert!(text.contains("read_file"));
        assert!(text.contains("<tool_call>"));
    }
}
