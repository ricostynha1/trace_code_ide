//! Claude Code's transcript, read into the events the sandbox shows.
//!
//! `transcript::read` knows records that carry their text as a string. Claude
//! Code writes its conversation as content blocks under `message` — text,
//! thinking, a tool call, a tool's result — so almost every record it writes
//! was unrecognised, and the sandbox showed no conversation at all. This reads
//! that shape. Usage and cost still come from `transcript::read`, which already
//! knows `message.usage`.
//!
//! Read only, and only to show: what the agent said decides nothing
//! (`REQ-TRANSCRIPT.no_interpretation`).
//!
//! @implements REQ-TRANSCRIPT.tool_format_read

use serde_json::Value;

use crate::observe::transcript::Event;

/// How much of one block a row shows. A tool's output can be a whole file.
const SHOWN: usize = 240;

fn shorten(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= SHOWN {
        one_line
    } else {
        format!("{}…", one_line.chars().take(SHOWN).collect::<String>())
    }
}

/// What a tool was called with, briefly: the argument a person recognises.
fn call(name: &str, input: &Value) -> String {
    let pick = ["file_path", "path", "command", "pattern", "url", "query", "description", "prompt"]
        .iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()));
    match pick {
        Some(arg) => format!("{name}({})", shorten(arg)),
        None => format!("{name}()"),
    }
}

/// The text of a tool result, which is a string or a list of text blocks.
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

/// Text a harness wrapped around a message rather than something a person said.
fn is_machinery(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty() || t.starts_with("<command-") || t.starts_with("<local-command") || t.starts_with("<system-reminder>")
}

fn push(out: &mut Vec<Event>, kind: &str, text: String) {
    out.push(Event { kind: kind.to_string(), text });
}

/// The events of one record, in the order its blocks appear.
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

/// The conversation in a Claude Code transcript, as far as it is written.
///
/// A last line without its newline is still being written and is left for the
/// next read, as `transcript::read` leaves it.
///
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
