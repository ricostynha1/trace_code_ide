//! Trimmed Rules Table — rule-based classification of which messages are
//! *allowed* to be cut at all.
//!
//! This answers one question per message, by type: "is this the kind of
//! thing that may ever be pruned?" (a user question never is; a stale tool
//! result may be, once it ages out of the protected recent-turn window).
//! It does not decide *whether* pruning any of them is worth it cost-wise,
//! nor *where* to cut among them — that's `cost_trimmed_summary_model`'s
//! job, operating only on the candidates this table marks eligible.
//!
//! Keeping the rule table separate from the cost math means a new rule
//! ("never prune X", "Y ages out after N turns") is a change in one place,
//! inspectable on its own, instead of a special case buried in the cost
//! scan.

use super::provider::{ChatMessage, MessageRole};
use super::retention::{EntryKind, RetentionAction, RetentionEntry};

/// Output of classifying a live message list against the rules table.
pub struct ClassifiedMessages {
    /// Every non-system message, oldest first, each carrying the
    /// Keep/Eligible verdict the rules table assigned it.
    pub entries: Vec<RetentionEntry>,
    /// `entries[i]` came from `messages[entry_to_msg_idx[i]]`.
    pub entry_to_msg_idx: Vec<usize>,
    /// Turn count inferred from assistant messages (one loop iteration each).
    pub current_turn: usize,
}

/// Classify a message list into retention entries per the rules table.
///
/// Rule: user messages are always `Keep` — they're never pruned (a summary
/// still captures them; pruning would just delete the question). Everything
/// else (assistant turns, tool results) is `Eligible`; whether an eligible
/// entry is old enough to actually be a candidate (the recent-turns
/// protected window) is enforced downstream by
/// `RetentionEngine::eligible_for_pruning`, not here — this table only
/// answers the type-based question.
pub fn classify_messages(messages: &[ChatMessage]) -> ClassifiedMessages {
    let mut entries: Vec<RetentionEntry> = Vec::new();
    let mut entry_to_msg_idx: Vec<usize> = Vec::new();
    let mut turn: usize = 0;

    for (msg_idx, msg) in messages.iter().enumerate().skip(1) {
        // skip system prompt at [0]
        let tokens = msg.content.len() / 4
            + msg.tool_calls.iter().map(|tc| tc.function.arguments.len() / 4 + 5).sum::<usize>();
        if tokens > 0 {
            let kind = if msg.role == MessageRole::Tool {
                EntryKind::ToolResult
            } else if msg.role == MessageRole::User {
                EntryKind::UserMsg
            } else {
                EntryKind::AssistantMsg
            };
            let action = if kind == EntryKind::UserMsg {
                RetentionAction::Keep
            } else {
                RetentionAction::Eligible
            };
            entries.push(RetentionEntry {
                id: entries.len() as u64,
                kind,
                action,
                content: msg.content.clone(),
                resources: Vec::new(),
                created_turn: turn,
                last_used_turn: turn,
                approx_tokens: tokens,
                ttl: None,
                invalidation_events: Vec::new(),
                args_hash: None,
                ephemeral: false,
                offloaded: false,
                offload_path: None,
            });
            entry_to_msg_idx.push(msg_idx);
        }
        if msg.role == MessageRole::Assistant {
            turn += 1;
        }
    }

    ClassifiedMessages { entries, entry_to_msg_idx, current_turn: turn }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage { role, content: content.to_string(), tool_call_id: None, tool_calls: Vec::new() }
    }

    #[test]
    fn user_messages_are_kept() {
        let messages = vec![
            msg(MessageRole::System, "sys"),
            msg(MessageRole::User, "hello there"),
            msg(MessageRole::Assistant, "hi, how can I help"),
        ];
        let classified = classify_messages(&messages);
        assert_eq!(classified.entries.len(), 2);
        assert_eq!(classified.entries[0].action, RetentionAction::Keep);
        assert_eq!(classified.entries[1].action, RetentionAction::Eligible);
        assert_eq!(classified.entry_to_msg_idx, vec![1, 2]);
    }

    #[test]
    fn turn_advances_on_assistant_messages() {
        let messages = vec![
            msg(MessageRole::System, "sys"),
            msg(MessageRole::User, "question one, long enough to count"),
            msg(MessageRole::Assistant, "answer one, long enough to count"),
            msg(MessageRole::User, "question two, long enough to count"),
            msg(MessageRole::Assistant, "answer two, long enough to count"),
        ];
        let classified = classify_messages(&messages);
        assert_eq!(classified.current_turn, 2);
        assert_eq!(classified.entries[0].created_turn, 0); // q1
        assert_eq!(classified.entries[1].created_turn, 0); // a1 closes turn 0
        assert_eq!(classified.entries[2].created_turn, 1); // q2
    }

    #[test]
    fn empty_messages_are_skipped() {
        let messages = vec![
            msg(MessageRole::System, "sys"),
            msg(MessageRole::Assistant, ""),
        ];
        let classified = classify_messages(&messages);
        assert!(classified.entries.is_empty());
    }
}
