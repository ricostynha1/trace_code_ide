//! @implements REQ-TRANSCRIPT.tool_format_read

use serde_json::Value;

use crate::observe::transcript::Event;

const SHOWN: usize = 240;

fn shorten(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= SHOWN {
        one_line
    } else {
        format!("{}…", one_line.chars().take(SHOWN).collect::<String>())
    }
}

fn call(name: &str, input: &Value) -> String {
    let pick = ["file_path", "path", "command", "pattern", "url", "query", "description", "prompt"]
        .iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()));
    match pick {
        Some(arg) => format!("{name}({})", shorten(arg)),
        None => format!("{name}()"),
    }
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn is_machinery(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty() || t.starts_with("<command-") || t.starts_with("<local-command") || t.starts_with("<system-reminder>")
}

fn push(out: &mut Vec<Event>, kind: &str, text: String) {
    out.push(Event { kind: kind.to_string(), text });
}

fn record(value: &Value, out: &mut Vec<Event>) {
    let Some(kind) = value.get("type").and_then(|k| k.as_str()) else { return };
    if value.get("isMeta").and_then(|m| m.as_bool()) == Some(true) {
        return;
    }
    let Some(message) = value.get("message") else { return };
    let content = message.get("content");
    match (kind, content) {
        ("user", Some(Value::String(text))) if !is_machinery(text) => push(out, "you", shorten(text)),
        ("user" | "assistant", Some(Value::Array(blocks))) => {
            for block in blocks {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !is_machinery(text) {
                            push(out, if kind == "user" { "you" } else { "agent" }, shorten(text));
                        }
                    }
                    Some("thinking") => {
                        let text = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                        if !text.is_empty() {
                            push(out, "thinking", shorten(text));
                        }
                    }
                    Some("tool_use") => {
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        push(out, "tool", call(name, &input));
                    }
                    Some("tool_result") => {
                        let failed = block.get("is_error").and_then(|e| e.as_bool()) == Some(true);
                        let text = result_text(block.get("content").unwrap_or(&Value::Null));
                        push(out, if failed { "failed" } else { "result" }, shorten(&text));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// @implements REQ-TRANSCRIPT.tool_format_read
/// @implements REQ-TRANSCRIPT.read_only
pub fn events(text: &str) -> Vec<Event> {
    let complete = match text.rfind('\n') {
        Some(at) => &text[..=at],
        None => "",
    };
    let mut out = Vec::new();
    for line in complete.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            record(&value, &mut out);
        }
    }
    out
}

/// @drt REQ-TRANSCRIPT.tool_format_read
pub fn events_owned(text: String) -> Vec<Event> {
    events(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-TRANSCRIPT.tool_format_read
    #[test]
    fn a_claude_code_session_reads_as_a_conversation() {
        let text = [
            r#"{"type":"mode","mode":"normal"}"#,
            r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"look first"},{"type":"text","text":"Reading it."},{"type":"tool_use","name":"Read","input":{"file_path":"/p/src/a.rs"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"fn a() {}","is_error":false}]}}"#,
            r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"caveat"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"half"#,
        ]
        .join("\n");
        let kinds: Vec<(String, String)> =
            events(&text).into_iter().map(|e| (e.kind, e.text)).collect();
        assert_eq!(
            kinds,
            vec![
                ("you".into(), "fix the bug".into()),
                ("thinking".into(), "look first".into()),
                ("agent".into(), "Reading it.".into()),
                ("tool".into(), "Read(/p/src/a.rs)".into()),
                ("result".into(), "fn a() {}".into()),
            ]
        );
    }

    #[test]
    fn a_long_result_is_shortened_to_one_line() {
        let long = "x\n".repeat(400);
        let shown = shorten(&long);
        assert!(shown.chars().count() <= SHOWN + 1 && !shown.contains('\n'));
    }
}
