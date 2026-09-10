//! Tails a Claude Code session's own JSONL transcript
//! (`~/.claude/projects/<slug>/<uuid>.jsonl`) and turns new lines into a
//! normalized event stream. No interception, no driving the tool — Claude
//! Code already writes this file for itself; tracelean only reads it.

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEvent {
    UserText { text: String },
    AssistantText { text: String },
    Thinking { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Usage {
        model: String,
        /// Fresh (non-cached) input tokens, billed at the plain input rate.
        input_tokens: u64,
        output_tokens: u64,
        /// Served from cache — billed at a discount (0.1x input, Anthropic).
        cache_read_tokens: u64,
        /// Newly written with a 5-minute TTL — billed at a premium (1.25x).
        cache_write_5m_tokens: u64,
        /// Newly written with a 1-hour TTL — billed at a higher premium (2x).
        cache_write_1h_tokens: u64,
    },
    SystemNote { subtype: String },
}

/// Reproduce Claude Code's project-directory slug and return the newest
/// transcript file under it, if any exist yet — a session may not have
/// started when the watcher first looks, so this is a lookup, not a hard
/// requirement. Verified against real `~/.claude/projects/*` directory
/// names: every character that isn't ASCII alphanumeric becomes `-`, not
/// just `/` — e.g. `/home/u/trace_code_ide` slugs to
/// `-home-u-trace-code-ide` (the `_` becomes `-` too, not just the `/`s).
///
/// Every tracelean sandbox session for the same project binds its work dir
/// at the identical real project path (so paths inside match paths
/// outside — see `sandbox::session`), which means Claude Code's own
/// path-based slug is identical across sessions too: two sandbox sessions
/// for the same project resolve to the *same* directory here, containing
/// one `.jsonl` per `claude` invocation across all of them. Picking
/// "newest" alone would let a second session's tail lock onto the first
/// session's still-live file. Use `transcript_path_excluding` with a
/// registry of already-claimed paths to avoid that.
pub fn transcript_path(claude_home: &Path, project_cwd: &Path) -> Option<PathBuf> {
    transcript_path_excluding(claude_home, project_cwd, &std::collections::HashSet::new())
}

/// Like `transcript_path`, but skips any path already in `exclude` — so a
/// session doesn't attach to a transcript file another session's tail has
/// already claimed for the same (path-aliased) project.
pub fn transcript_path_excluding(
    claude_home: &Path,
    project_cwd: &Path,
    exclude: &std::collections::HashSet<PathBuf>,
) -> Option<PathBuf> {
    let abs = project_cwd.canonicalize().ok()?;
    let slug: String = abs
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = claude_home.join("projects").join(slug);
    let entries = std::fs::read_dir(&dir).ok()?;
    entries
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        .filter(|e| !exclude.contains(&e.path()))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// Parse one JSONL line into zero or more transcript events (an assistant
/// line with several content blocks yields several events), tagged with
/// the uuid of the record they came from. Record types with no UI
/// representation (`mode`, `permission-mode`, `queue-operation`, ...)
/// produce nothing.
pub fn parse_line(line: &str) -> Vec<(String, TranscriptEvent)> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return Vec::new() };
    let uuid = v.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("user") => parse_message(&uuid, &v, true),
        Some("assistant") => {
            let mut events = parse_message(&uuid, &v, false);
            if let Some(u) = v.pointer("/message/usage") {
                let model = v.pointer("/message/model").and_then(|m| m.as_str()).unwrap_or("").to_string();
                events.push((
                    uuid,
                    {
                        // The nested breakdown (added alongside the aggregate
                        // cache_creation_input_tokens field) is what lets us
                        // price 5m vs 1h writes correctly (1.25x vs 2x). Fall
                        // back to treating the whole aggregate as 5m-TTL if
                        // an older transcript format lacks the breakdown.
                        let creation = u.get("cache_creation");
                        let aggregate = u.get("cache_creation_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                        let (write_5m, write_1h) = match creation {
                            Some(c) => (
                                c.get("ephemeral_5m_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                                c.get("ephemeral_1h_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                            ),
                            None => (aggregate, 0),
                        };
                        TranscriptEvent::Usage {
                            model,
                            input_tokens: u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                            output_tokens: u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                            cache_read_tokens: u.get("cache_read_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
                            cache_write_5m_tokens: write_5m,
                            cache_write_1h_tokens: write_1h,
                        }
                    },
                ));
            }
            events
        }
        Some("system") => {
            let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("").to_string();
            vec![(uuid, TranscriptEvent::SystemNote { subtype })]
        }
        _ => Vec::new(),
    }
}

fn parse_message(uuid: &str, record: &serde_json::Value, is_user: bool) -> Vec<(String, TranscriptEvent)> {
    let content = record.pointer("/message/content");
    let mut out = Vec::new();
    match content {
        Some(serde_json::Value::String(s)) => {
            out.push((
                uuid.to_string(),
                if is_user {
                    TranscriptEvent::UserText { text: s.clone() }
                } else {
                    TranscriptEvent::AssistantText { text: s.clone() }
                },
            ));
        }
        Some(serde_json::Value::Array(blocks)) => {
            for b in blocks {
                let Some(kind) = b.get("type").and_then(|t| t.as_str()) else { continue };
                let ev = match kind {
                    "text" => {
                        let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                        if is_user { TranscriptEvent::UserText { text } } else { TranscriptEvent::AssistantText { text } }
                    }
                    "thinking" => TranscriptEvent::Thinking {
                        text: b.get("thinking").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                    },
                    "tool_use" => TranscriptEvent::ToolUse {
                        id: b.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                        name: b.get("name").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                        input: b.get("input").cloned().unwrap_or(serde_json::Value::Null),
                    },
                    "tool_result" => TranscriptEvent::ToolResult {
                        tool_use_id: b.get("tool_use_id").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                        content: tool_result_text(b.get("content")),
                        is_error: b.get("is_error").and_then(|t| t.as_bool()).unwrap_or(false),
                    },
                    _ => continue,
                };
                out.push((uuid.to_string(), ev));
            }
        }
        _ => {}
    }
    out
}

fn tool_result_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Tracks a byte offset into one transcript file so repeated calls only
/// return newly appended lines.
pub struct TranscriptTail {
    path: PathBuf,
    offset: u64,
}

impl TranscriptTail {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    /// Read every full line appended since the last call and parse them.
    /// A trailing partial (not-yet-newline-terminated) line is left for
    /// next time.
    pub fn poll(&mut self) -> Vec<(String, TranscriptEvent)> {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return Vec::new() };
        let Ok(len) = f.metadata().map(|m| m.len()) else { return Vec::new() };
        if len < self.offset {
            self.offset = 0; // transcript was truncated/replaced
        }
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&buf);
        let Some(idx) = text.rfind('\n') else { return Vec::new() };
        let complete = &text[..idx];
        self.offset += (idx + 1) as u64;
        complete.lines().flat_map(parse_line).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Regression: two sandbox sessions for the same project bind at the
    /// identical real path, so Claude Code's own slug is identical across
    /// them too — a second session's tail must not lock onto the first
    /// session's still-live transcript file just because it's currently
    /// the newest.
    #[test]
    fn excludes_already_claimed_paths_from_the_newest_pick() {
        let claude_home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let project_path = project.path().join("shared-project");
        std::fs::create_dir_all(&project_path).unwrap();

        let abs = project_path.canonicalize().unwrap();
        let slug: String = abs.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let session_dir = claude_home.path().join("projects").join(&slug);
        std::fs::create_dir_all(&session_dir).unwrap();

        // Older session's transcript, already claimed by its tail.
        let old = session_dir.join("old-session.jsonl");
        std::fs::write(&old, "{}\n").unwrap();

        // Nothing new yet for a second sandbox session on the same project
        // — the only candidate is already claimed.
        let mut claimed = HashSet::new();
        claimed.insert(old.clone());
        assert_eq!(transcript_path_excluding(claude_home.path(), &project_path, &claimed), None);

        // Once `claude` actually starts in the second sandbox, its own new
        // file should be picked instead of the (newer-by-mtime, but
        // claimed) old one.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let new = session_dir.join("new-session.jsonl");
        std::fs::write(&new, "{}\n").unwrap();
        assert_eq!(transcript_path_excluding(claude_home.path(), &project_path, &claimed), Some(new));
    }

    #[test]
    fn parses_assistant_text_block() {
        let line = r#"{"type":"assistant","uuid":"u1","message":{"content":[{"type":"text","text":"hi"}],"model":"claude-opus-5","usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":4}}}"#;
        let events = parse_line(line);
        assert!(events.iter().any(|(_, e)| matches!(e, TranscriptEvent::AssistantText { text } if text == "hi")));
        assert!(events.iter().any(|(_, e)| matches!(e, TranscriptEvent::Usage { input_tokens: 1, output_tokens: 2, .. })));
    }

    #[test]
    fn parses_tool_use_and_result() {
        let use_line = r#"{"type":"assistant","uuid":"u2","message":{"content":[{"type":"tool_use","id":"t1","name":"read_file","input":{"path":"a.rs"}}]}}"#;
        let events = parse_line(use_line);
        assert!(events.iter().any(|(_, e)| matches!(e, TranscriptEvent::ToolUse { id, name, .. } if id == "t1" && name == "read_file")));

        let result_line = r#"{"type":"user","uuid":"u3","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}]}}"#;
        let events = parse_line(result_line);
        assert!(events.iter().any(|(_, e)| matches!(e, TranscriptEvent::ToolResult { tool_use_id, is_error: false, .. } if tool_use_id == "t1")));
    }

    #[test]
    fn usage_splits_cache_writes_by_ttl() {
        let line = r#"{"type":"assistant","uuid":"u5","message":{"content":[],"model":"claude-opus-5","usage":{"input_tokens":2,"output_tokens":280,"cache_read_input_tokens":18469,"cache_creation_input_tokens":8842,"cache_creation":{"ephemeral_1h_input_tokens":8842,"ephemeral_5m_input_tokens":0}}}}"#;
        let events = parse_line(line);
        let usage = events.iter().find_map(|(_, e)| match e {
            TranscriptEvent::Usage { cache_write_5m_tokens, cache_write_1h_tokens, .. } => Some((*cache_write_5m_tokens, *cache_write_1h_tokens)),
            _ => None,
        });
        assert_eq!(usage, Some((0, 8842)));
    }

    #[test]
    fn usage_falls_back_to_5m_when_no_ttl_breakdown_present() {
        let line = r#"{"type":"assistant","uuid":"u6","message":{"content":[],"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":500}}}"#;
        let events = parse_line(line);
        let usage = events.iter().find_map(|(_, e)| match e {
            TranscriptEvent::Usage { cache_write_5m_tokens, cache_write_1h_tokens, .. } => Some((*cache_write_5m_tokens, *cache_write_1h_tokens)),
            _ => None,
        });
        assert_eq!(usage, Some((500, 0)));
    }

    #[test]
    fn slug_replaces_underscores_not_just_slashes() {
        // Regression: verified against a real ~/.claude/projects entry —
        // `/home/u/trace_code_ide` slugs to `-home-u-trace-code-ide`, not
        // `-home-u-trace_code_ide`. A naive `.replace('/', "-")` misses this.
        let claude_home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let project_path = project.path().join("trace_code_ide");
        std::fs::create_dir_all(&project_path).unwrap();

        let abs = project_path.canonicalize().unwrap();
        let slug: String = abs.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let session_dir = claude_home.path().join("projects").join(&slug);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("s1.jsonl"), "{}\n").unwrap();

        let found = transcript_path(claude_home.path(), &project_path);
        assert_eq!(found, Some(session_dir.join("s1.jsonl")));
    }

    #[test]
    fn ignores_non_message_record_types() {
        assert!(parse_line(r#"{"type":"mode","mode":"default","sessionId":"s"}"#).is_empty());
        assert!(parse_line(r#"{"type":"queue-operation","operation":"push"}"#).is_empty());
    }

    #[test]
    fn tail_returns_only_new_complete_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "{\"type\":\"mode\"}\n").unwrap();
        let mut tail = TranscriptTail::new(path.clone());
        assert!(tail.poll().is_empty());

        std::fs::write(
            &path,
            "{\"type\":\"mode\"}\n{\"type\":\"assistant\",\"uuid\":\"u4\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"new\"}]}}\n",
        )
        .unwrap();
        let events = tail.poll();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0].1, TranscriptEvent::AssistantText { text } if text == "new"));
    }
}
