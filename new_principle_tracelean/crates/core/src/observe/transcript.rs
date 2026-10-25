use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Read {
    pub events: Vec<Event>,
    /// @implements REQ-TRANSCRIPT.unknown_preserved
    pub unrecognised: Vec<String>,
    /// @implements REQ-TRANSCRIPT.partial_line_held
    pub held: String,
    pub usage: Vec<Usage>,
}

fn nat_field(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

fn usage_in(value: &serde_json::Value) -> Option<Usage> {
    let holder = value.get("message").unwrap_or(value);
    let counts = holder.get("usage").filter(|held| held.is_object())?;
    let model = holder
        .get("model")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("model").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();
    Some(Usage {
        model,
        input: nat_field(counts, "input_tokens"),
        cached: nat_field(counts, "cache_read_input_tokens"),
        cache_write: nat_field(counts, "cache_creation_input_tokens"),
        output: nat_field(counts, "output_tokens"),
    })
}

/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.no_interpretation
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn read(text: String) -> Read {
    let mut out = Read::default();

    let (complete, held) = match text.rfind('\n') {
        Some(at) => (&text[..=at], &text[at + 1..]),
        None => ("", text.as_str()),
    };
    out.held = held.to_string();

    for line in complete.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                let kind = value.get("type").and_then(|v| v.as_str());
                let text = value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| value.get("content").and_then(|v| v.as_str()));
                let said = match (kind, text) {
                    (Some(kind), Some(text)) => {
                        out.events.push(Event {
                            kind: kind.to_string(),
                            text: text.to_string(),
                        });
                        true
                    }
                    _ => false,
                };
                let counted = match usage_in(&value) {
                    Some(usage) => {
                        out.usage.push(usage);
                        true
                    }
                    None => false,
                };
                let claimed = value.get("type").is_some();
                if !said && (claimed || !counted) {
                    out.unrecognised.push(line.to_string());
                }
            }
            Err(_) => out.unrecognised.push(line.to_string()),
        }
    }

    out
}

/// @implements REQ-TRANSCRIPT.partial_line_held
/// @drt REQ-TRANSCRIPT.partial_line_held
/// @drt REQ-TRANSCRIPT.unknown_preserved
/// @drt REQ-TRANSCRIPT.absent_is_fine
pub fn read_chunks(chunks: Vec<String>) -> Read {
    read(chunks.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-TRANSCRIPT.partial_line_held
    #[test]
    fn a_record_still_being_written_is_held() {
        let text = "{\"type\":\"say\",\"text\":\"one\"}\n{\"type\":\"say\",\"te";
        let result = read(text.to_string());
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.held, "{\"type\":\"say\",\"te");
        assert!(result.unrecognised.is_empty(), "a partial record is not an error");
    }

    #[test]
    fn a_complete_looking_last_line_is_still_held_until_its_newline() {
        let result = read("{\"type\":\"say\",\"text\":\"one\"}".to_string());
        assert!(result.events.is_empty());
        assert!(!result.held.is_empty());
    }

    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn an_unrecognised_record_does_not_abort_the_read() {
        let text = "{\"type\":\"say\",\"text\":\"one\"}\n{\"something\":\"else\"}\nnot json\n{\"type\":\"say\",\"text\":\"two\"}\n";
        let result = read(text.to_string());
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.unrecognised.len(), 2);
    }

    /// @tests REQ-TRANSCRIPT.absent_is_fine
    #[test]
    fn nothing_at_all_is_a_valid_transcript() {
        assert_eq!(read(String::new()), Read::default());
    }

    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn a_record_read_in_part_is_still_reported_as_unrecognised() {
        let text = "{\"type\":\"assistant\",\"message\":{\"model\":\"opus\",\
                    \"content\":[{\"type\":\"text\",\"text\":\"hello\"}],\
                    \"usage\":{\"output_tokens\":7}}}\n";
        let result = read(text.to_string());
        assert_eq!(result.usage.len(), 1, "the usage was not read");
        assert_eq!(result.usage[0].output, 7);
        assert_eq!(
            result.unrecognised.len(),
            1,
            "a record whose text could not be read said nothing about it"
        );
    }

    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn a_record_that_only_reports_usage_is_recognised() {
        let text = "{\"message\":{\"model\":\"opus\",\"usage\":{\"input_tokens\":3}}}\n";
        let result = read(text.to_string());
        assert_eq!(result.usage.len(), 1);
        assert_eq!(result.unrecognised, Vec::<String>::new());
    }

    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn a_usage_key_holding_something_else_is_not_a_usage_record() {
        let result = read("{\"note\":\"hello\",\"usage\":123}\n".to_string());
        assert_eq!(result.usage, vec![], "a number was read as a usage record");
        assert_eq!(result.unrecognised.len(), 1, "and it was not reported either");
    }

    #[test]
    fn a_held_remainder_completes_on_the_next_read() {
        let first = read("{\"type\":\"say\",\"text\":\"one\"}\n{\"type\":\"sa".to_string());
        let second = read(format!("{}y\",\"text\":\"two\"}}\n", first.held));
        assert_eq!(second.events, vec![Event { kind: "say".into(), text: "two".into() }]);
        assert!(second.held.is_empty());
    }
}
