//! Reading an external tool's own session record.
//!
//! A convenience, never a dependency: the workspace diff is the truth about
//! what happened, and the transcript is the tool's account of it. A system that
//! acted on the account would be taking instruction from the tool.

use serde::{Deserialize, Serialize};

/// One thing the tool reported doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub kind: String,
    pub text: String,
}

/// What one exchange used, as the tool reported it.
///
/// Here rather than with the pricing, because it is a thing a transcript says.
/// What it costs is a separate question answered against a table (`REQ-COST`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

/// What a read of a transcript yielded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Read {
    pub events: Vec<Event>,
    /// Records whose shape was not recognised, kept rather than discarded.
    ///
    /// @implements REQ-TRANSCRIPT.unknown_preserved
    pub unrecognised: Vec<String>,
    /// Bytes not yet consumed, because the last record is still being written.
    ///
    /// A record read while it is still being appended parses as malformed, and
    /// treating that as an error makes the feature fail exactly when the tool
    /// is most active.
    ///
    /// @implements REQ-TRANSCRIPT.partial_line_held
    pub held: String,
    /// What the tool said it used, where it said so.
    pub usage: Vec<Usage>,
}

/// A natural field, or zero.
///
/// A count a tool did not report is a count of none, and refusing the whole
/// record over one absent field would lose the rest of it.
fn nat_field(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// The usage a record reports, if it reports any.
///
/// Two shapes, because tools write both: the counts at the top of the record,
/// or one level down under `message`, which is where a tool that wraps an API
/// response puts them. The names are the ones the API itself uses; renaming
/// them here would be this project inventing a vocabulary and then failing to
/// recognise the real one.
fn usage_in(value: &serde_json::Value) -> Option<Usage> {
    let holder = value.get("message").unwrap_or(value);
    // The key existing is not enough: `{"usage": 123}` is not a usage record,
    // and reading it as one invented a model named `unknown` that the estimate
    // then reported as unpriced — a count nobody spent, announced as a gap in
    // the price table.
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

/// Parse as much of a transcript as is complete.
///
/// An absent transcript is not a degraded mode: an empty string reads as an
/// empty result, and the workspace diff is the truth either way.
///
/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.no_interpretation
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn read(text: String) -> Read {
    let mut out = Read::default();

    // Only what precedes the final newline is complete. Everything after it is
    // held, whether or not it happens to parse — a record can be valid JSON and
    // still be half of what the tool intends to write.
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
                // A record that reports usage is recognised even when it
                // carries no text, which is the common case: a tool's
                // accounting records say what was spent and nothing a person
                // reads. They went to `unrecognised` while nothing here knew
                // what usage was, which was true then and misleading now.
                let counted = match usage_in(&value) {
                    Some(usage) => {
                        out.usage.push(usage);
                        true
                    }
                    None => false,
                };
                // Readable, but not a shape this knows. Reported rather than
                // dropped: an unrecognised record is information about a tool
                // this has not been taught yet.
                //
                // A record carrying a `type` claims to be a thing with a kind,
                // so one that claims it and yields no event is unrecognised
                // *even when it also carried usage* — the common case being
                // text written as an array of content blocks rather than as a
                // string. Reporting only the wholly unrecognised records
                // dropped exactly those, silently, which is
                // `unknown_preserved` failing where it matters most.
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

/// `read`, over a transcript given as the fragments it was written in.
///
/// The conformance protocol exchanges structured values, and a generator that
/// could only produce a flat string would never produce a newline — so the
/// interesting inputs, the ones with a half-written last record, would never
/// be generated at all. Joining fragments with newlines is how the shape of a
/// transcript gets into the generator; the function under test is still
/// `read`.
///
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

    /// A record can be valid JSON and still be half of what will be written.
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

    /// A record recognised in one part and not in another is reported.
    ///
    /// The shape a real agent transcript writes: text as an array of content
    /// blocks, with the usage for the same exchange beside it. The usage is
    /// read; the text is not; and the record must say so. It did not, because
    /// the usage suppressed the report — so the one thing this reader exists to
    /// prevent, silently losing what it has not been taught, was happening on
    /// the commonest record there is.
    ///
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

    /// An accounting record with no text is recognised, not reported.
    ///
    /// The other side of the rule above: a record that never claimed to say
    /// anything is not a shape this reader failed to understand.
    ///
    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn a_record_that_only_reports_usage_is_recognised() {
        let text = "{\"message\":{\"model\":\"opus\",\"usage\":{\"input_tokens\":3}}}\n";
        let result = read(text.to_string());
        assert_eq!(result.usage.len(), 1);
        assert_eq!(result.unrecognised, Vec::<String>::new());
    }

    /// A `usage` that is not an object is not usage.
    ///
    /// It used to be read as one, with the model defaulting to `unknown` — and
    /// the estimate then announced `unpriced: unknown` for a record that
    /// reported no usage at all. An honesty signal firing on nothing is worse
    /// than no signal.
    ///
    /// @tests REQ-TRANSCRIPT.unknown_preserved
    #[test]
    fn a_usage_key_holding_something_else_is_not_a_usage_record() {
        let result = read("{\"note\":\"hello\",\"usage\":123}\n".to_string());
        assert_eq!(result.usage, vec![], "a number was read as a usage record");
        assert_eq!(result.unrecognised.len(), 1, "and it was not reported either");
    }

    /// Holding is stable: feeding the held remainder back with its completion
    /// yields the record, and nothing is read twice.
    #[test]
    fn a_held_remainder_completes_on_the_next_read() {
        let first = read("{\"type\":\"say\",\"text\":\"one\"}\n{\"type\":\"sa".to_string());
        let second = read(format!("{}y\",\"text\":\"two\"}}\n", first.held));
        assert_eq!(second.events, vec![Event { kind: "say".into(), text: "two".into() }]);
        assert!(second.held.is_empty());
    }
}
