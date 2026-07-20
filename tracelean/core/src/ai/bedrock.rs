//! Amazon Bedrock provider — Chat Completions API on bedrock-mantle.
//!
//! Uses `bedrock-mantle.{region}.api.aws/v1/chat/completions`
//! with Bearer token authentication (Bedrock API key).
//!
//! Tool passing follows the model's `ToolPassing` strategy (P9a):
//! - `NativeParam` (default): tools go in the request `tools` field, history keeps
//!   structured `tool_calls` / `role: "tool"` messages, and native response
//!   `tool_calls` take precedence (text parsing stays as fallback).
//! - `SystemPromptEmbed` (Bedrock+MiniMax): tools are rendered as text into the
//!   system prompt in the model's trained dialect (`ToolCallFormat`, P2) and
//!   calls are parsed from response text in that dialect.

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

/// Render tool schemas as a text block to embed in system prompt (D2.1).
/// The `<tools>` section carries the full JSON schema per tool (matching the
/// MiniMax chat template); the invocation instructions follow the model's
/// trained dialect.
fn render_tools_as_text(tools: &[ToolSchema], format: ToolCallFormat) -> String {
    let mut out = String::from("# Tools\n");
    out.push_str("You may call one or more tools to assist with the user query.\n");
    out.push_str("Here are the tools available in JSONSchema format:\n\n");
    out.push_str("<tools>\n");
    for tool in tools {
        let schema_json = serde_json::json!({
            "name": tool.function.name,
            "description": tool.function.description,
            "parameters": tool.function.parameters,
        });
        out.push_str("<tool>");
        out.push_str(&serde_json::to_string(&schema_json).unwrap_or_else(|_| "{}".into()));
        out.push_str("</tool>\n");
    }
    out.push_str("</tools>\n\n");

    match format {
        ToolCallFormat::MiniMaxXml => {
            out.push_str("When making tool calls, use XML format to invoke tools and pass parameters:\n\n");
            out.push_str("<minimax:tool_call>\n");
            out.push_str("<invoke name=\"tool-name\">\n");
            out.push_str("<parameter name=\"param-key\">param-value</parameter>\n");
            out.push_str("...\n");
            out.push_str("</invoke>\n");
            out.push_str("</minimax:tool_call>\n");
        }
        ToolCallFormat::HermesJson => {
            out.push_str("To call tools, respond with one or more <tool_call> blocks:\n\n");
            out.push_str("<tool_call>\n");
            out.push_str("{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/main.py\"}}\n");
            out.push_str("</tool_call>\n");
            out.push_str("<tool_call>\n");
            out.push_str("{\"name\": \"list_directory\", \"arguments\": {\"path\": \".\"}}\n");
            out.push_str("</tool_call>\n");
        }
        ToolCallFormat::MistralBrackets => {
            out.push_str("To call tools, respond with a [TOOL_CALLS] line containing a JSON array:\n\n");
            out.push_str("[TOOL_CALLS] [{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/main.py\"}}]\n");
        }
    }

    out.push_str("\nReturn ALL independent calls in ONE response. Do NOT call one tool then wait.\n");
    out
}

/// Result of parsing tool blocks: successful calls + error messages for malformed ones.
struct ParsedToolCalls {
    calls: Vec<ToolCallResponse>,
    /// Error descriptions for malformed tool calls (fed back to model)
    errors: Vec<String>,
}

/// Map from tool name → its JSON-schema `parameters` object, for schema-driven
/// type coercion in the XML parser (D2.3).
type ToolSchemaMap<'a> = std::collections::HashMap<&'a str, &'a serde_json::Value>;

fn build_schema_map(tools: Option<&[ToolSchema]>) -> ToolSchemaMap<'_> {
    tools
        .map(|ts| {
            ts.iter()
                .map(|t| (t.function.name.as_str(), &t.function.parameters))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse tool calls from model output with the Hermes-first legacy ordering
/// and no schema coercion. Kept for callers/tests without model context.
#[cfg(test)]
fn parse_tool_blocks(content: &str) -> ParsedToolCalls {
    parse_tool_blocks_with(content, ToolCallFormat::HermesJson, &ToolSchemaMap::new())
}

/// P17: public replay entry — parse tool calls from raw model output exactly
/// as the runtime does (declared dialect first, other formats as fallback).
pub fn parse_tool_calls_from_text(
    content: &str,
    format: ToolCallFormat,
    tools: Option<&[ToolSchema]>,
) -> (Vec<ToolCallResponse>, Vec<String>) {
    let map = build_schema_map(tools);
    let parsed = parse_tool_blocks_with(content, format, &map);
    (parsed.calls, parsed.errors)
}

/// Parse tool calls from model output. The model's declared `ToolCallFormat`
/// is tried first (D2.2); the other formats stay as fallback so a model
/// answering in a different dialect still works. Supported dialects:
/// 1. Hermes format: <tool_call>{"name": "...", "arguments": {...}}</tool_call>
/// 2. Mistral format: [TOOL_CALLS] [{"name": "...", "arguments": {...}}, ...]
///    or [TOOL_CALL] followed by JSON lines
/// 3. MiniMax native: <minimax:tool_call><invoke name="..."><parameter name="...">...</invoke></minimax:tool_call>
fn parse_tool_blocks_with(
    content: &str,
    format: ToolCallFormat,
    schemas: &ToolSchemaMap,
) -> ParsedToolCalls {
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    let mut idx = 0u32;

    let order: [ToolCallFormat; 3] = match format {
        ToolCallFormat::MiniMaxXml => [
            ToolCallFormat::MiniMaxXml,
            ToolCallFormat::HermesJson,
            ToolCallFormat::MistralBrackets,
        ],
        ToolCallFormat::HermesJson => [
            ToolCallFormat::HermesJson,
            ToolCallFormat::MistralBrackets,
            ToolCallFormat::MiniMaxXml,
        ],
        ToolCallFormat::MistralBrackets => [
            ToolCallFormat::MistralBrackets,
            ToolCallFormat::HermesJson,
            ToolCallFormat::MiniMaxXml,
        ],
    };

    for strategy in order {
        match strategy {
            ToolCallFormat::MiniMaxXml => {
                parse_minimax_native(content, &mut calls, &mut errors, &mut idx, schemas)
            }
            ToolCallFormat::HermesJson => {
                parse_hermes_blocks(content, &mut calls, &mut errors, &mut idx)
            }
            ToolCallFormat::MistralBrackets => {
                parse_mistral_prefix(content, &mut calls, &mut errors, &mut idx)
            }
        }
        if !calls.is_empty() || !errors.is_empty() {
            break;
        }
    }

    ParsedToolCalls { calls, errors }
}

/// Hermes format: <tool_call> ... </tool_call> tags with JSON bodies.
fn parse_hermes_blocks(content: &str, calls: &mut Vec<ToolCallResponse>, errors: &mut Vec<String>, idx: &mut u32) {
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
        parse_json_body(body, calls, errors, idx);
        search_from = closer + "</tool_call>".len();
    }
}

/// Mistral format: [TOOL_CALLS] / [TOOL_CALL] prefix followed by a JSON array
/// or JSON lines.
fn parse_mistral_prefix(content: &str, calls: &mut Vec<ToolCallResponse>, errors: &mut Vec<String>, idx: &mut u32) {
    // Case-insensitive search for [TOOL_CALL] or [TOOL_CALLS]
    let upper = content.to_uppercase();
    if let Some(pos) = upper.find("[TOOL_CALL") {
        // Skip past the tag and any trailing ] or S]
        let after_tag = content[pos..].find(']').map(|p| pos + p + 1).unwrap_or(pos + 11);
        let body = content[after_tag..].trim();

        // Try as JSON array first: [{"name": ...}, {"name": ...}]
        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(body) {
            for item in &arr {
                match extract_tool_call(item, idx) {
                    Ok(tc) => calls.push(tc),
                    Err(e) => errors.push(e),
                }
            }
        } else {
            // Try line-by-line JSON
            parse_json_body(body, calls, errors, idx);
        }
    }
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

/// Strip at most one leading and one trailing newline. Preserves inner
/// whitespace and leading indentation — critical for `old_str`-style params.
fn strip_edge_newlines(s: &str) -> &str {
    let s = s.strip_prefix("\r\n").or_else(|| s.strip_prefix('\n')).unwrap_or(s);
    s.strip_suffix("\r\n").or_else(|| s.strip_suffix('\n')).unwrap_or(s)
}

/// Coerce a raw XML parameter value using its declared JSON-schema type
/// (D2.3). `declared: None` (unknown tool/param) falls back to guessing.
fn coerce_param_value(raw: &str, declared: Option<&str>) -> serde_json::Value {
    match declared {
        // Declared string: raw text verbatim (minus the template's edge
        // newlines). Never guessed into bool/number/JSON.
        Some("string") => serde_json::Value::String(strip_edge_newlines(raw).to_string()),
        Some("boolean") => match raw.trim() {
            "true" => serde_json::Value::Bool(true),
            "false" => serde_json::Value::Bool(false),
            other => serde_json::Value::String(other.to_string()),
        },
        Some("integer") => raw
            .trim()
            .parse::<i64>()
            .map(|n| serde_json::Value::Number(n.into()))
            .unwrap_or_else(|_| serde_json::Value::String(raw.trim().to_string())),
        Some("number") => raw
            .trim()
            .parse::<f64>()
            .map(|n| serde_json::json!(n))
            .unwrap_or_else(|_| serde_json::Value::String(raw.trim().to_string())),
        Some("array") | Some("object") => serde_json::from_str(raw.trim())
            .unwrap_or_else(|_| serde_json::Value::String(raw.trim().to_string())),
        // Unknown: legacy guessing heuristic.
        _ => {
            let trimmed = raw.trim();
            if trimmed.starts_with('[') || trimmed.starts_with('{') {
                serde_json::from_str(trimmed)
                    .unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
            } else if trimmed == "true" {
                serde_json::Value::Bool(true)
            } else if trimmed == "false" {
                serde_json::Value::Bool(false)
            } else if trimmed == "null" {
                serde_json::Value::Null
            } else if let Ok(n) = trimmed.parse::<i64>() {
                serde_json::Value::Number(n.into())
            } else if let Ok(n) = trimmed.parse::<f64>() {
                serde_json::json!(n)
            } else {
                serde_json::Value::String(trimmed.to_string())
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
///
/// Parameter values capture everything (newlines, quotes, braces) up to the
/// next `</parameter>` (D2.5). Documented limitation: a value containing the
/// literal string `</parameter>` cannot be represented.
fn parse_minimax_native(
    content: &str,
    calls: &mut Vec<ToolCallResponse>,
    errors: &mut Vec<String>,
    idx: &mut u32,
    schemas: &ToolSchemaMap,
) {
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
                let raw_value = &param_tag[value_start..];

                // Coerce using the declared JSON-schema type when known (D2.3)
                let declared = schemas
                    .get(func_name.as_str())
                    .and_then(|params| params.get("properties"))
                    .and_then(|props| props.get(pname.as_str()))
                    .and_then(|schema| schema.get("type"))
                    .and_then(|t| t.as_str());
                let json_value = coerce_param_value(raw_value, declared);

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

/// Strip `<think>…</think>` spans (D2.6): MiniMax emits them in its native
/// dialect. An unterminated `<think>` drops the rest of the content (it is
/// all thinking).
#[cfg(test)]
fn strip_think_tags(content: &str) -> String {
    extract_think_tags(content).0
}

/// Like `strip_think_tags`, but also returns the captured thinking text
/// (bugs.md Bug 1.8: surfaced in the chat UI instead of silently dropped).
fn extract_think_tags(content: &str) -> (String, Option<String>) {
    let mut result = content.to_string();
    let mut thinking = String::new();
    loop {
        let Some(open) = result.find("<think>") else { break };
        let inner_start = open + "<think>".len();
        match result[open..].find("</think>") {
            Some(rel) => {
                let end = open + rel + "</think>".len();
                if !thinking.is_empty() {
                    thinking.push('\n');
                }
                thinking.push_str(result[inner_start..open + rel].trim());
                result.replace_range(open..end, "");
            }
            None => {
                if !thinking.is_empty() {
                    thinking.push('\n');
                }
                thinking.push_str(result[inner_start..].trim());
                result.truncate(open);
                break;
            }
        }
    }
    let thinking = if thinking.trim().is_empty() { None } else { Some(thinking) };
    (result.trim().to_string(), thinking)
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
    /// Native tool passing (P9a) — only set for `ToolPassing::NativeParam`.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolSchema>>,
}

#[derive(Serialize)]
struct ChatMsg {
    role: String,
    content: String,
    /// Structured tool calls on assistant messages (native history, P9a).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallResponse>>,
    /// Set on `role: "tool"` result messages (native history, P9a).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMsg {
    fn text(role: &str, content: String) -> Self {
        Self { role: role.into(), content, tool_calls: None, tool_call_id: None }
    }
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
    /// Native tool calls (P9a). Previously not deserialized at all, which made
    /// every natively-tool-calling Bedrock model appear tool-less.
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallResponse>>,
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
            eprintln!(
                "[tool_passing: {:?} | tool_call_format: {:?}]",
                request.model.tool_passing, request.model.tool_call_format
            );
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

        // Native tool passing (P9a): tools go in the request field.
        // Dynamic tools (bugs.md Feature 3) merge into the param on the native
        // path — Chat Completions has no defer_loading, so this is the only
        // native option and carries the documented invalidation cost.
        let native = request.model.tool_passing == ToolPassing::NativeParam;
        let body = ChatRequest {
            model: request.model.model_id.clone(),
            messages,
            max_tokens: request.model.max_tokens,
            temperature: request.model.temperature,
            stop: request.stop.clone(),
            tools: if native {
                let mut all = request.tools.clone().unwrap_or_default();
                if let Some(dyn_tools) = &request.dynamic_tools {
                    all.extend(dyn_tools.iter().cloned());
                }
                if all.is_empty() { None } else { Some(all) }
            } else {
                None
            },
        };

        // bugs.md Bug 3: capture the exact wire body for the log's "copy raw
        // request" button — `body` is a locally-owned struct, so this is a
        // plain serialize with no extra request work.
        let raw_request = serde_json::to_string_pretty(&body).ok();

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

                    // Strip <think>…</think> before tool parsing and display
                    // (D2.6), capturing the thinking text for the UI (Bug 1.8).
                    let (raw_content, think_text) =
                        extract_think_tags(&message.content.clone().unwrap_or_default());
                    let thinking = message
                        .reasoning
                        .clone()
                        .filter(|r| !r.trim().is_empty())
                        .or(think_text);
                    let truncated = choice.finish_reason.as_deref() == Some("length");

                    // Native tool calls take precedence (D9a.2); text parsing
                    // stays as fallback so a model answering in dialect text
                    // still works.
                    let native_calls: Vec<ToolCallResponse> = message
                        .tool_calls
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut tc)| {
                            if tc.id.is_empty() {
                                tc.id = format!("call_{}", i);
                            }
                            if tc.call_type.is_empty() {
                                tc.call_type = "function".into();
                            }
                            tc
                        })
                        .collect();

                    let (tool_calls, mut content) = if !native_calls.is_empty() {
                        (native_calls, raw_content)
                    } else {
                        // Parse tool calls from text, dialect-first (P2)
                        let schemas = build_schema_map(request.tools.as_deref());
                        let tool_parsed = parse_tool_blocks_with(
                            &raw_content,
                            request.model.tool_call_format,
                            &schemas,
                        );

                        // Strip tool blocks from content (leave only natural text)
                        let mut content = if !tool_parsed.calls.is_empty()
                            || !tool_parsed.errors.is_empty()
                        {
                            strip_tool_blocks(&raw_content)
                        } else {
                            raw_content
                        };

                        // If there were parse errors, append them to content so runtime
                        // sees them and can feed back to the model on next iteration
                        if !tool_parsed.errors.is_empty() {
                            let registry = super::tool_registry::ToolRegistry::embedded();
                            for err in &tool_parsed.errors {
                                content.push_str("\n\n");
                                content.push_str(&super::tool_errors::format_tool_call_error(
                                    None,
                                    err,
                                    "tool call could not be parsed",
                                    registry,
                                    request.model.tool_call_format,
                                ));
                            }
                        }

                        (tool_parsed.calls, content)
                    };

                    // A thinking model can burn the whole output budget inside
                    // an unterminated <think> span; stripping then leaves
                    // nothing. Surface that instead of an empty answer.
                    if truncated && content.is_empty() && tool_calls.is_empty() {
                        content = "[No answer: the model used its entire output budget on internal thinking before being cut off. Retry, simplify the request, or raise max_tokens.]".to_string();
                    }

                    // Token usage
                    let usage = parsed.usage.as_ref();
                    let cached = usage
                        .and_then(|u| u.prompt_tokens_details.as_ref())
                        .and_then(|d| d.cached_tokens)
                        .unwrap_or(0);

                    // Count reasoning/thinking tokens if present (covers both
                    // the `reasoning` field and captured <think> spans)
                    let thinking_tokens = thinking.as_ref()
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
                        raw_request,
                        truncated,
                        tool_calls,
                        thinking,
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
            .map(|m| model_for_id(&m.id))
            .collect();

        Ok(models)
    }
}

/// Build a Bedrock model's `ModelConfig` from just its id — no network call.
/// Pricing/catalog data (`bedrock_pricing`, `provider_cache`, `model_catalog`)
/// is all compiled-in static data, so this is cheap to call anytime a
/// Bedrock model's up-to-date info is needed (e.g. re-deriving a persisted
/// settings selection on load, instead of trusting a stale saved snapshot —
/// bugs.md: MiniMax's cached price stayed wrong in a saved selection after
/// the pricing table was fixed).
pub fn model_for_id(model_id: &str) -> ModelConfig {
    let (input_cost, output_cost) = bedrock_pricing(model_id);
    let mut model = ModelConfig {
        provider: ProviderKind::Bedrock,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        max_tokens: 4096,
        temperature: 0.3,
        input_cost_per_m: input_cost,
        output_cost_per_m: output_cost,
        cached_input_cost_per_m: super::provider_cache::default_cached_price_per_m(model_id, input_cost),
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: true,
        ..Default::default()
    };
    super::model_catalog::enrich(&mut model);
    model
}

// ---------------------------------------------------------------------------
// Message building: embed tools in system, flatten tool history as text
// ---------------------------------------------------------------------------

impl BedrockProvider {
    /// Build the message array for Chat Completions per the model's
    /// tool-passing strategy (P9a).
    fn build_messages(&self, request: &AiRequest) -> Vec<ChatMsg> {
        match request.model.tool_passing {
            ToolPassing::NativeParam => self.build_messages_native(request),
            ToolPassing::SystemPromptEmbed => self.build_messages_embedded(request),
        }
    }

    /// Native path (D9a.2): history keeps real `tool_calls` and `role: "tool"`
    /// messages — no text flattening. Tools go in the request `tools` field.
    fn build_messages_native(&self, request: &AiRequest) -> Vec<ChatMsg> {
        let mut out: Vec<ChatMsg> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => out.push(ChatMsg::text("system", msg.content.clone())),
                MessageRole::User => out.push(ChatMsg::text("user", msg.content.clone())),
                MessageRole::Assistant => out.push(ChatMsg {
                    role: "assistant".into(),
                    content: msg.content.clone(),
                    tool_calls: if msg.tool_calls.is_empty() {
                        None
                    } else {
                        Some(msg.tool_calls.clone())
                    },
                    tool_call_id: None,
                }),
                MessageRole::Tool => out.push(ChatMsg {
                    role: "tool".into(),
                    content: msg.content.clone(),
                    tool_calls: None,
                    tool_call_id: msg.tool_call_id.clone(),
                }),
            }
        }

        self.append_batching_reminder(request, &mut out);
        out
    }

    /// Embedded path (P2, Bedrock+MiniMax):
    /// - Tools are rendered as text at the END of the system prompt, in the
    ///   model's tool-call dialect.
    /// - Assistant tool calls are re-rendered in the SAME dialect (D2.4) so
    ///   history matches what the model is told to emit and the cached prefix
    ///   stays byte-stable.
    /// - Tool result messages are flattened to user messages with an id label.
    fn build_messages_embedded(&self, request: &AiRequest) -> Vec<ChatMsg> {
        let format = request.model.tool_call_format;
        let tools_text = request.tools.as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| render_tools_as_text(t, format));

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
                    out.push(ChatMsg::text("system", content));
                }
                MessageRole::User => {
                    out.push(ChatMsg::text("user", msg.content.clone()));
                }
                MessageRole::Assistant => {
                    // Reconstruct assistant message: text + tool calls in dialect
                    let mut content = msg.content.clone();
                    for tc in &msg.tool_calls {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        let args: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(Default::default()));
                        content.push_str(&super::tool_errors::render_tool_call(
                            &tc.function.name,
                            &args,
                            format,
                        ));
                    }
                    out.push(ChatMsg::text("assistant", content));
                }
                MessageRole::Tool => {
                    // Tool results → user message with label
                    let tool_id = msg.tool_call_id.as_deref().unwrap_or("unknown");
                    let content = format!("[tool_result id={}]\n{}", tool_id, msg.content);
                    out.push(ChatMsg::text("user", content));
                }
            }
        }

        // If no system message was in the input but we have tools, prepend one
        if tools_text.is_some() && !request.messages.iter().any(|m| m.role == MessageRole::System) {
            if let Some(tt) = tools_text {
                out.insert(0, ChatMsg::text("system", tt));
            }
        }

        // bugs.md Feature 3: dynamically discovered tools are injected as a
        // trailing user message — the system prompt / static tools prefix stays
        // byte-stable, so the provider prompt cache is preserved.
        if let Some(dyn_tools) = request.dynamic_tools.as_ref().filter(|t| !t.is_empty()) {
            let mut txt = String::from(
                "[Additional tools loaded via discover_tools — callable exactly like the tools above]\n",
            );
            txt.push_str(&render_tools_as_text(dyn_tools, format));
            out.push(ChatMsg::text("user", txt));
        }

        self.append_batching_reminder(request, &mut out);
        out
    }

    /// Append the batching reminder to the last user message. Positioned at
    /// the very end of context (prefix-safe, D9b.4).
    fn append_batching_reminder(&self, request: &AiRequest, out: &mut [ChatMsg]) {
        if request.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) {
            if let Some(last) = out.iter_mut().rev().find(|m| m.role == "user") {
                last.content.push_str("\n\n[IMPORTANT: Return ALL independent tool calls in ONE response. Do NOT call one tool then wait.]");
            }
        }
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

    fn sample_tools() -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                },
            },
            ToolSchema {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "replace_str".into(),
                    description: "Replace a string in a file".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "old_str": {"type": "string"},
                            "new_str": {"type": "string"},
                            "count": {"type": "integer"},
                            "dry_run": {"type": "boolean"}
                        },
                        "required": ["path", "old_str", "new_str"]
                    }),
                },
            },
        ]
    }

    #[test]
    fn test_render_tools_as_text_hermes() {
        let text = render_tools_as_text(&sample_tools(), ToolCallFormat::HermesJson);
        assert!(text.contains("# Tools"));
        assert!(text.contains("<tools>"));
        assert!(text.contains("read_file"));
        assert!(text.contains("<tool_call>"));
        assert!(!text.contains("<minimax:tool_call>"));
    }

    #[test]
    fn test_render_tools_as_text_minimax() {
        let text = render_tools_as_text(&sample_tools(), ToolCallFormat::MiniMaxXml);
        assert!(text.contains("<tools>"));
        // Full JSON schema per tool inside <tool>
        assert!(text.contains("<tool>{\"name\":\"read_file\""));
        assert!(text.contains("<minimax:tool_call>"));
        assert!(text.contains("<parameter name=\"param-key\">param-value</parameter>"));
        assert!(!text.contains("<tool_call>\n{\"name\""));
    }

    // ─── P2 regression fixtures: MiniMax XML dialect ─────────────────────────

    /// The exact failure class from todo.md: a multi-line `old_str` with
    /// quotes, braces, and indentation that reliably broke JSON escaping.
    #[test]
    fn test_minimax_multiline_old_str_round_trips() {
        let old_str = "    let config = Config {\n        name: \"tracelean\",\n        version: \"0.1.0\",\n    };\n    println!(\"{:?}\", config);";
        let new_str = "    let config = Config::default();";
        let content = format!(
            "I'll fix that.\n<minimax:tool_call>\n<invoke name=\"replace_str\">\n<parameter name=\"path\">src/main.rs</parameter>\n<parameter name=\"old_str\">{}</parameter>\n<parameter name=\"new_str\">{}</parameter>\n</invoke>\n</minimax:tool_call>",
            old_str, new_str
        );
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(&content, ToolCallFormat::MiniMaxXml, &schemas);
        assert!(parsed.errors.is_empty(), "errors: {:?}", parsed.errors);
        assert_eq!(parsed.calls.len(), 1);
        let args: serde_json::Value =
            serde_json::from_str(&parsed.calls[0].function.arguments).unwrap();
        assert_eq!(args["path"], "src/main.rs");
        assert_eq!(args["old_str"], old_str, "old_str must survive verbatim");
        assert_eq!(args["new_str"], new_str);
    }

    #[test]
    fn test_minimax_cjk_emoji_values() {
        let content = "<minimax:tool_call>\n<invoke name=\"replace_str\">\n<parameter name=\"path\">日本語/ファイル.rs</parameter>\n<parameter name=\"old_str\">let greeting = \"こんにちは 🎉\";</parameter>\n<parameter name=\"new_str\">let greeting = \"你好 🚀\";</parameter>\n</invoke>\n</minimax:tool_call>";
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(content, ToolCallFormat::MiniMaxXml, &schemas);
        assert_eq!(parsed.calls.len(), 1);
        let args: serde_json::Value =
            serde_json::from_str(&parsed.calls[0].function.arguments).unwrap();
        assert_eq!(args["path"], "日本語/ファイル.rs");
        assert_eq!(args["old_str"], "let greeting = \"こんにちは 🎉\";");
    }

    /// A string-typed parameter whose value LOOKS like JSON must stay a string
    /// (schema-driven coercion, D2.3 — the old parser guessed it into an object).
    #[test]
    fn test_minimax_json_looking_string_stays_string() {
        let content = "<minimax:tool_call>\n<invoke name=\"replace_str\">\n<parameter name=\"path\">a.json</parameter>\n<parameter name=\"old_str\">{\"key\": \"value\"}</parameter>\n<parameter name=\"new_str\">{\"key\": \"new\"}</parameter>\n</invoke>\n</minimax:tool_call>";
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(content, ToolCallFormat::MiniMaxXml, &schemas);
        assert_eq!(parsed.calls.len(), 1);
        let args: serde_json::Value =
            serde_json::from_str(&parsed.calls[0].function.arguments).unwrap();
        assert!(args["old_str"].is_string());
        assert_eq!(args["old_str"], "{\"key\": \"value\"}");
    }

    /// "true"/"123" as string-typed values must not be coerced to bool/number;
    /// integer/boolean-typed params ARE coerced.
    #[test]
    fn test_minimax_schema_typed_coercion() {
        let content = "<minimax:tool_call>\n<invoke name=\"replace_str\">\n<parameter name=\"path\">x</parameter>\n<parameter name=\"old_str\">true</parameter>\n<parameter name=\"new_str\">123</parameter>\n<parameter name=\"count\">2</parameter>\n<parameter name=\"dry_run\">true</parameter>\n</invoke>\n</minimax:tool_call>";
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(content, ToolCallFormat::MiniMaxXml, &schemas);
        assert_eq!(parsed.calls.len(), 1);
        let args: serde_json::Value =
            serde_json::from_str(&parsed.calls[0].function.arguments).unwrap();
        assert_eq!(args["old_str"], "true");
        assert_eq!(args["new_str"], "123");
        assert_eq!(args["count"], 2);
        assert_eq!(args["dry_run"], true);
    }

    #[test]
    fn test_minimax_two_invokes_one_block() {
        let content = "<minimax:tool_call>\n<invoke name=\"read_file\">\n<parameter name=\"path\">a.rs</parameter>\n</invoke>\n<invoke name=\"read_file\">\n<parameter name=\"path\">b.rs</parameter>\n</invoke>\n</minimax:tool_call>";
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(content, ToolCallFormat::MiniMaxXml, &schemas);
        assert_eq!(parsed.calls.len(), 2);
        assert_eq!(parsed.calls[0].id, "call_0");
        assert_eq!(parsed.calls[1].id, "call_1");
    }

    /// Full round-trip (D2.4): history rendering via render_tool_call must
    /// parse back to identical arguments.
    #[test]
    fn test_minimax_render_parse_round_trip() {
        let args = serde_json::json!({
            "path": "src/lib.rs",
            "old_str": "fn main() {\n    println!(\"old\");\n}",
            "new_str": "fn main() {\n    println!(\"new\");\n}"
        });
        let rendered = super::super::tool_errors::render_tool_call(
            "replace_str",
            &args,
            ToolCallFormat::MiniMaxXml,
        );
        let tools = sample_tools();
        let schemas = build_schema_map(Some(&tools));
        let parsed = parse_tool_blocks_with(&rendered, ToolCallFormat::MiniMaxXml, &schemas);
        assert!(parsed.errors.is_empty(), "errors: {:?}", parsed.errors);
        assert_eq!(parsed.calls.len(), 1);
        let back: serde_json::Value =
            serde_json::from_str(&parsed.calls[0].function.arguments).unwrap();
        assert_eq!(back, args);
    }

    #[test]
    fn test_strip_think_tags() {
        assert_eq!(
            strip_think_tags("<think>reasoning here</think>Hello."),
            "Hello."
        );
        assert_eq!(
            strip_think_tags("A<think>x</think>B<think>y</think>C"),
            "ABC"
        );
        // Unterminated think drops the tail
        assert_eq!(strip_think_tags("Answer.<think>still going"), "Answer.");
        assert_eq!(strip_think_tags("No tags at all."), "No tags at all.");
    }

    // ─── P9a fixtures: native tool path ──────────────────────────────────────

    #[test]
    fn test_native_response_tool_calls_deserialize() {
        let body = r#"{
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        }"#;
        let parsed: ChatResponse = serde_json::from_str(body).unwrap();
        let msg = parsed.choices.unwrap().remove(0).message.unwrap();
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
    }

    /// D2.4: embedded MiniMax history re-renders past tool calls in the
    /// MiniMax dialect (not Hermes), and is byte-stable across identical calls.
    #[test]
    fn test_embedded_minimax_history_dialect() {
        use super::super::provider::{ChatMessage, MessageRole, ToolPassing};
        let provider = BedrockProvider::new("test-token".into(), None);
        let mut model = ModelConfig {
            provider: ProviderKind::Bedrock,
            model_id: "minimax.minimax-m2".into(),
            ..Default::default()
        };
        model.tool_call_format = ToolCallFormat::MiniMaxXml;
        model.tool_passing = ToolPassing::SystemPromptEmbed;

        let request = AiRequest {
            dynamic_tools: None,
            model,
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "You are an agent.".into(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: "".into(),
                    tool_call_id: None,
                    tool_calls: vec![ToolCallResponse {
                        id: "call_0".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"a.rs\"}".into(),
                        },
                    }],
                },
                ChatMessage {
                    role: MessageRole::Tool,
                    content: "contents".into(),
                    tool_call_id: Some("call_0".into()),
                    tool_calls: Vec::new(),
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "now edit it".into(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
            ],
            stop: None,
            tools: Some(sample_tools()),
            cache_breakpoints: Vec::new(),
        };

        let msgs = provider.build_messages(&request);
        // System prompt carries the MiniMax-dialect tool block
        assert!(msgs[0].content.contains("<tools>"));
        assert!(msgs[0].content.contains("<minimax:tool_call>"));
        // Assistant history re-rendered as MiniMax XML, not Hermes JSON
        assert!(msgs[1].content.contains("<invoke name=\"read_file\">"));
        assert!(msgs[1].content.contains("<parameter name=\"path\">a.rs</parameter>"));
        assert!(!msgs[1].content.contains("<tool_call>"));
        // Tool result flattened to user message
        assert!(msgs[2].content.starts_with("[tool_result id=call_0]"));
        assert_eq!(msgs[2].role, "user");
        // Byte-stable across identical calls (cache precondition)
        let again = provider.build_messages(&request);
        for (a, b) in msgs.iter().zip(again.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn test_chat_request_serializes_native_tools_and_history() {
        let req = ChatRequest {
            model: "eu.amazon.nova-pro-v1:0".into(),
            messages: vec![
                ChatMsg::text("system", "sys".into()),
                ChatMsg {
                    role: "assistant".into(),
                    content: "".into(),
                    tool_calls: Some(vec![ToolCallResponse {
                        id: "call_0".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"a.rs\"}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMsg {
                    role: "tool".into(),
                    content: "file contents".into(),
                    tool_calls: None,
                    tool_call_id: Some("call_0".into()),
                },
            ],
            max_tokens: 100,
            temperature: 0.2,
            stop: None,
            tools: Some(sample_tools()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tools"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][1]["tool_calls"][0]["id"], "call_0");
        assert_eq!(json["messages"][2]["role"], "tool");
        assert_eq!(json["messages"][2]["tool_call_id"], "call_0");
        // Text messages must not carry null tool fields
        assert!(json["messages"][0].get("tool_calls").is_none());
    }
}
