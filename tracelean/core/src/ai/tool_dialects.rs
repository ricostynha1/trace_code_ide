//! Text-based tool-calling dialects (Hermes JSON, Mistral brackets, MiniMax
//! XML) — rendering tool schemas into a system-prompt block and parsing tool
//! calls back out of model text.
//!
//! This is provider-*model* logic, not provider-*transport* logic: it only
//! matters for `ToolPassing::SystemPromptEmbed` models (Bedrock+MiniMax
//! today), but any provider routing to a model without native tool-call
//! support needs the same dialect support — kept here, parameterized purely
//! by `ToolCallFormat`, instead of living inside `bedrock.rs` where only
//! Bedrock could reuse it (ai_module_cleanup_plan.md finding 2).

use super::provider::{ToolCallFormat, ToolCallFunction, ToolCallResponse, ToolSchema};

// ---------------------------------------------------------------------------
// Tool embedding: render tool schemas as text for the system prompt
// ---------------------------------------------------------------------------

/// Render tool schemas as a text block to embed in system prompt (D2.1).
/// The `<tools>` section carries the full JSON schema per tool (matching the
/// MiniMax chat template); the invocation instructions follow the model's
/// trained dialect.
pub fn render_tools_as_text(tools: &[ToolSchema], format: ToolCallFormat) -> String {
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
pub struct ParsedToolCalls {
    pub calls: Vec<ToolCallResponse>,
    /// Error descriptions for malformed tool calls (fed back to model)
    pub errors: Vec<String>,
}

/// Map from tool name → its JSON-schema `parameters` object, for schema-driven
/// type coercion in the XML parser (D2.3).
pub type ToolSchemaMap<'a> = std::collections::HashMap<&'a str, &'a serde_json::Value>;

pub fn build_schema_map(tools: Option<&[ToolSchema]>) -> ToolSchemaMap<'_> {
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
pub fn parse_tool_blocks(content: &str) -> ParsedToolCalls {
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
pub fn parse_tool_blocks_with(
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

/// Strip tool call blocks from content (both <tool_call> tags and [TOOL_CALL] sections).
pub fn strip_tool_blocks(content: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::provider::ToolFunction;

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
}
