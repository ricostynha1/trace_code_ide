//! Actionable tool-call error messages (P3).
//!
//! One canonical builder used by every parse/validation failure path so the
//! model always gets: which tool failed, why, and a correct example rendered
//! in the dialect it is expected to speak.

use super::provider::ToolCallFormat;
use super::tool_registry::ToolRegistry;

/// Extract a tool name from a broken/partial payload with a lenient scan.
/// Works on malformed JSON (`"name":"replace_str"` fragments) and on
/// MiniMax XML (`<invoke name="replace_str">`).
pub fn lenient_tool_name(raw: &str) -> Option<String> {
    // JSON-ish: "name" : "identifier"
    if let Some(pos) = raw.find("\"name\"") {
        let rest = &raw[pos + 6..];
        let rest = rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
        if let Some(stripped) = rest.strip_prefix('"') {
            let name: String = stripped
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    // XML-ish: <invoke name="identifier">
    if let Some(pos) = raw.find("<invoke name=\"") {
        let rest = &raw[pos + 14..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Render a tool call in the given dialect. Used for error-message examples
/// and for re-rendering past tool calls into embedded-tool history (D2.4).
pub fn render_tool_call(tool_name: &str, args: &serde_json::Value, format: ToolCallFormat) -> String {
    match format {
        ToolCallFormat::MiniMaxXml => {
            let mut out = String::from("<minimax:tool_call>\n");
            out.push_str(&format!("<invoke name=\"{}\">\n", tool_name));
            if let Some(obj) = args.as_object() {
                for (key, value) in obj {
                    let text = match value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    out.push_str(&format!(
                        "<parameter name=\"{}\">{}</parameter>\n",
                        key, text
                    ));
                }
            }
            out.push_str("</invoke>\n</minimax:tool_call>");
            out
        }
        ToolCallFormat::HermesJson => format!(
            "<tool_call>\n{}\n</tool_call>",
            serde_json::json!({ "name": tool_name, "arguments": args })
        ),
        ToolCallFormat::MistralBrackets => format!(
            "[TOOL_CALLS] [{}]",
            serde_json::json!({ "name": tool_name, "arguments": args })
        ),
    }
}

/// Build the canonical error message for a malformed tool call.
///
/// `tool_name`: pass it if the caller knows it; otherwise it is recovered
/// leniently from `raw`. `raw` is the offending payload (truncated in output).
pub fn format_tool_call_error(
    tool_name: Option<&str>,
    raw: &str,
    parse_err: &str,
    registry: Option<&ToolRegistry>,
    format: ToolCallFormat,
) -> String {
    let recovered;
    let name = match tool_name {
        Some(n) => Some(n),
        None => {
            recovered = lenient_tool_name(raw);
            recovered.as_deref()
        }
    };

    let raw_preview: String = if raw.chars().count() > 200 {
        format!("{}…", raw.chars().take(200).collect::<String>())
    } else {
        raw.to_string()
    };

    let mut msg = String::from("[TOOL_CALL_ERROR]\n");
    match name {
        Some(n) => msg.push_str(&format!("- Tool: {}\n", n)),
        None => msg.push_str("- Tool: (could not be determined)\n"),
    }
    msg.push_str(&format!("- Problem: {}\n", parse_err));
    msg.push_str(&format!("- Your call (truncated): {}\n", raw_preview));

    if let (Some(n), Some(reg)) = (name, registry) {
        if let Some(example) = reg.example_for(n) {
            msg.push_str(&format!(
                "\nCorrect invocation format for `{}` — copy this structure exactly:\n{}\n",
                n,
                render_tool_call(n, example, format)
            ));
        } else if let Some(schema) = reg.schema_for(n) {
            msg.push_str(&format!(
                "\nParameter schema for `{}`:\n{}\n",
                n,
                serde_json::to_string(schema).unwrap_or_default()
            ));
        }
    }
    msg.push_str("\nRetry the call in exactly that format.");
    msg
}

/// A schema-validation failure: which parameter caused it (`None` for
/// whole-arguments-object failures) plus the human-readable reason, so
/// callers can look up that parameter's own `error_hint` (finding 5).
pub struct ValidationError {
    pub param: Option<String>,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Validate parsed arguments against a tool's JSON schema (small hand-rolled
/// checker: required fields present, no unknown fields, primitive types match).
/// Returns Err on the first violation.
pub fn validate_args(args: &serde_json::Value, schema: &serde_json::Value) -> Result<(), ValidationError> {
    let Some(obj) = args.as_object() else {
        return Err(ValidationError {
            param: None,
            message: format!("arguments must be a JSON object, got {}", type_name(args)),
        });
    };
    let properties = schema.get("properties").and_then(|p| p.as_object());

    // Required fields present
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for req in required {
            if let Some(key) = req.as_str() {
                if !obj.contains_key(key) {
                    return Err(ValidationError {
                        param: Some(key.to_string()),
                        message: format!("missing required parameter `{}`", key),
                    });
                }
            }
        }
    }

    if let Some(props) = properties {
        // Unknown fields (only when the schema forbids them or lists properties)
        let forbid_additional = schema
            .get("additionalProperties")
            .and_then(|a| a.as_bool())
            .map(|b| !b)
            .unwrap_or(false);
        if forbid_additional {
            for key in obj.keys() {
                if !props.contains_key(key) {
                    return Err(ValidationError {
                        param: None,
                        message: format!("unknown parameter `{}`", key),
                    });
                }
            }
        }
        // Primitive type checks
        for (key, value) in obj {
            let Some(prop_schema) = props.get(key) else { continue };
            let Some(expected) = prop_schema.get("type").and_then(|t| t.as_str()) else {
                continue;
            };
            let ok = match expected {
                "string" => value.is_string(),
                "integer" => value.is_i64() || value.is_u64(),
                "number" => value.is_number(),
                "boolean" => value.is_boolean(),
                "array" => value.is_array(),
                "object" => value.is_object(),
                _ => true,
            };
            if !ok {
                return Err(ValidationError {
                    param: Some(key.to_string()),
                    message: format!(
                        "parameter `{}` must be {}, got {}",
                        key,
                        expected,
                        type_name(value)
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Look up a parameter's `error_hint` from a tool's JSON schema
/// (`input_schema.properties.<param>.error_hint` — not a standard
/// JSON-Schema keyword, read only by this validator, harmless to the
/// model-facing schema otherwise).
fn error_hint_for_param<'a>(schema: &'a serde_json::Value, param: &str) -> Option<&'a str> {
    schema
        .get("properties")
        .and_then(|p| p.get(param))
        .and_then(|p| p.get("error_hint"))
        .and_then(|h| h.as_str())
}

/// Build the error message shown to a model that called a tool with
/// arguments failing schema validation (missing/wrong-type params). Sourced
/// entirely from tools.json (`short_help`/`example`) rather than a per-tool
/// hardcoded string, so every tool's error text stays in sync with its
/// schema automatically (P1: no hardcoded tool text in the executor).
pub fn format_arg_validation_error(
    tool_name: &str,
    error: &ValidationError,
    registry: Option<&ToolRegistry>,
) -> String {
    let mut msg = format!("Invalid arguments for `{}`: {}.", tool_name, error.message);
    if let Some(reg) = registry {
        if let (Some(param), Some(schema)) = (&error.param, reg.schema_for(tool_name)) {
            if let Some(hint) = error_hint_for_param(schema, param) {
                msg.push_str(&format!("\n{}", hint));
            }
        }
        if let Some(sig) = reg.short_help_for(tool_name) {
            msg.push_str(&format!("\nUsage: {}", sig));
        }
        if let Some(example) = reg.example_for(tool_name) {
            msg.push_str(&format!(
                "\nExample arguments: {}",
                serde_json::to_string(example).unwrap_or_default()
            ));
        }
    }
    msg
}

fn type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_name_from_broken_json() {
        // The exact failure class from todo.md: truncated JSON with a huge old_str
        let raw = r#"{"name":"replace_str","arguments":{"path":"tracelean/core/src/ai/mod.rs","old_str":"/// Canonical system prompt for all ..."#;
        assert_eq!(lenient_tool_name(raw).as_deref(), Some("replace_str"));
    }

    #[test]
    fn lenient_name_from_xml() {
        let raw = "<minimax:tool_call>\n<invoke name=\"read_file\">\n<parameter";
        assert_eq!(lenient_tool_name(raw).as_deref(), Some("read_file"));
    }

    #[test]
    fn error_message_names_tool_and_shows_example() {
        let registry = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        let raw = r#"{"name":"replace_str","arguments":{"path":"x.rs","old_str":"unterminated"#;
        let msg = format_tool_call_error(
            None,
            raw,
            "invalid JSON: EOF while parsing an object",
            Some(&registry),
            ToolCallFormat::MiniMaxXml,
        );
        assert!(msg.contains("replace_str"));
        assert!(msg.contains("<minimax:tool_call>"));
        assert!(msg.contains("<parameter name=\"old_str\">"));
    }

    #[test]
    fn error_message_hermes_dialect() {
        let registry = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        let msg = format_tool_call_error(
            Some("read_file"),
            "{broken",
            "invalid JSON",
            Some(&registry),
            ToolCallFormat::HermesJson,
        );
        assert!(msg.contains("<tool_call>"));
        assert!(msg.contains("\"read_file\""));
    }

    #[test]
    fn validate_missing_required() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        });
        let err = validate_args(&serde_json::json!({}), &schema).unwrap_err();
        assert!(err.message.contains("path"));
        assert_eq!(err.param.as_deref(), Some("path"));
    }

    #[test]
    fn validate_wrong_type_and_unknown_field() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"count": {"type": "integer"}},
            "required": [],
            "additionalProperties": false
        });
        assert!(validate_args(&serde_json::json!({"count": "five"}), &schema).is_err());
        assert!(validate_args(&serde_json::json!({"bogus": 1}), &schema).is_err());
        assert!(validate_args(&serde_json::json!({"count": 5}), &schema).is_ok());
    }

    #[test]
    fn arg_validation_error_pulls_usage_and_example_from_registry() {
        let registry = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        let error = ValidationError {
            param: Some("path".to_string()),
            message: "missing required parameter `path`".to_string(),
        };
        let msg = format_arg_validation_error("read_file", &error, Some(&registry));
        assert!(msg.contains("read_file"));
        assert!(msg.contains("missing required parameter `path`"));
        assert!(msg.contains("Usage:"));
        assert!(msg.contains("Example arguments:"));
    }

    #[test]
    fn arg_validation_error_surfaces_param_error_hint_when_present() {
        let registry = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        let error = ValidationError {
            param: Some("old_str".to_string()),
            message: "missing required parameter `old_str`".to_string(),
        };
        let msg = format_arg_validation_error("replace_str", &error, Some(&registry));
        assert!(msg.contains("byte-for-byte"), "expected old_str's error_hint in: {}", msg);
    }

    #[test]
    fn all_tool_examples_validate_against_their_schemas() {
        let registry = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        for entry in registry.all_tool_entries() {
            let example = entry
                .example
                .as_ref()
                .unwrap_or_else(|| panic!("tool `{}` has no example in tools.json", entry.name));
            validate_args(example, &entry.input_schema)
                .unwrap_or_else(|e| panic!("example for `{}` invalid: {}", entry.name, e));
        }
    }
}
