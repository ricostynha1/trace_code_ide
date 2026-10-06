//! The language-server layer.
//!
//! A model in one language beside an implementation in another is the premise
//! of this project, so a registry keyed by language is the shape from the
//! start. What lives here is the part that decides: converting a position,
//! lowering an edit, and naming the state of a server.

use serde::{Deserialize, Serialize};

use crate::history::command::Command;

/// How a server counts columns.
///
/// Taken from what the server declared, never assumed: every mismatch shows up
/// as an off-by-some on exactly the lines containing non-ASCII text, reported
/// as "hover is wrong sometimes" and nearly unfindable from that description.
///
/// @implements REQ-LSP.encoding_declared
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

/// A position as the protocol states it: a line, and a column in the server's
/// units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// Byte offset within `line_text` for a column in `encoding`.
///
/// `None` when the column falls inside a character, which is a position no
/// buffer has and must not be rounded to a neighbouring one.
///
/// @implements REQ-LSP.encoding_round_trip
/// @drt REQ-LSP.encoding_round_trip
pub fn to_byte_offset(line_text: String, character: u32, encoding: Encoding) -> Option<usize> {
    let mut counted: u32 = 0;
    for (offset, ch) in line_text.char_indices() {
        if counted == character {
            return Some(offset);
        }
        counted += match encoding {
            Encoding::Utf8 => ch.len_utf8() as u32,
            Encoding::Utf16 => ch.len_utf16() as u32,
            Encoding::Utf32 => 1,
        };
        if counted > character {
            // The column landed inside this character.
            return None;
        }
    }
    (counted == character).then_some(line_text.len())
}

/// Column in `encoding` for a byte offset within `line_text`.
///
/// @implements REQ-LSP.encoding_round_trip
pub fn to_character(line_text: String, offset: usize, encoding: Encoding) -> Option<usize> {
    if offset > line_text.len() || !line_text.is_char_boundary(offset) {
        return None;
    }
    Some(
        line_text[..offset]
            .chars()
            .map(|ch| match encoding {
                Encoding::Utf8 => ch.len_utf8(),
                Encoding::Utf16 => ch.len_utf16(),
                Encoding::Utf32 => 1,
            })
            .sum(),
    )
}

/// One edit a server asked for, in byte offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LowerError {
    /// Two edits in one request cover the same text.
    Overlap { first: usize, second: usize },
    /// An edit's range is not inside the file, or splits a character.
    OutOfRange { start: usize, end: usize },
    Inverted { start: usize, end: usize },
}

/// Lower a server's edits into commands.
///
/// Applied from the end of the file backwards, so an earlier edit never
/// invalidates the offsets of a later one. Overlapping edits are refused rather
/// than applied in some order: the request describes a result, and two edits
/// over the same text describe two different results.
///
/// @implements REQ-LSP.edits_become_commands
/// @implements REQ-LSP.edits_ordered
/// @implements REQ-LSP.overlap_refused
pub fn lower(file: String, content: String, edits: Vec<Edit>) -> Result<Vec<Command>, LowerError> {
    for edit in &edits {
        if edit.start > edit.end {
            return Err(LowerError::Inverted { start: edit.start, end: edit.end });
        }
        if edit.end > content.len()
            || !content.is_char_boundary(edit.start)
            || !content.is_char_boundary(edit.end)
        {
            return Err(LowerError::OutOfRange { start: edit.start, end: edit.end });
        }
    }

    let mut ordered: Vec<&Edit> = edits.iter().collect();
    ordered.sort_by_key(|edit| (edit.start, edit.end));
    for pair in ordered.windows(2) {
        // Touching is not overlapping: two edits may meet at a boundary.
        if pair[1].start < pair[0].end {
            return Err(LowerError::Overlap { first: pair[0].start, second: pair[1].start });
        }
    }

    // Back to front, so every remaining offset still refers to the same text.
    let mut commands = Vec::new();
    for edit in ordered.into_iter().rev() {
        if edit.end > edit.start {
            commands.push(Command::Delete {
                file: file.clone(),
                offset: edit.start,
                deleted: content[edit.start..edit.end].to_string(),
            });
        }
        if !edit.text.is_empty() {
            commands.push(Command::Insert {
                file: file.clone(),
                offset: edit.start,
                text: edit.text.clone(),
            });
        }
    }
    Ok(commands)
}

/// What lowering produced: commands, or the reason it produced none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lowered {
    Commands { commands: Vec<Command> },
    Refused { error: LowerError },
}

/// `lower`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-LSP.edits_ordered
/// @drt REQ-LSP.edits_ordered
/// @drt REQ-LSP.edits_become_commands
/// @drt REQ-LSP.overlap_refused
pub fn lower_outcome(file: String, content: String, edits: Vec<Edit>) -> Lowered {
    match lower(file, content, edits) {
        Ok(commands) => Lowered::Commands { commands },
        Err(error) => Lowered::Refused { error },
    }
}

/// What is known about a server for a language.
///
/// The absence of a server is a named state. For a proof assistant this has a
/// sharp instance: an empty list of goals means the proof is complete, so
/// rendering "no server running" that way tells the user they have finished
/// when they have not started.
///
/// @implements REQ-LSP.absent_server_named
/// @implements REQ-LSP.registry_per_language
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerState {
    NotStarted,
    Starting,
    Running { encoding: Encoding },
    Failed { reason: String },
}

/// What to show where a result would go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Display<T> {
    Result { value: T },
    /// No server, and why. Never an empty result.
    Unavailable { reason: String },
}

/// Present a server's answer, or say why there is none.
///
/// @implements REQ-LSP.absent_server_named
pub fn present<T>(state: &ServerState, answer: Option<T>) -> Display<T> {
    match (state, answer) {
        (ServerState::Running { .. }, Some(value)) => Display::Result { value },
        (ServerState::Running { .. }, None) => {
            Display::Unavailable { reason: "the server returned nothing".into() }
        }
        (ServerState::NotStarted, _) => {
            Display::Unavailable { reason: "no server is running for this language".into() }
        }
        (ServerState::Starting, _) => Display::Unavailable { reason: "the server is starting".into() },
        (ServerState::Failed { reason }, _) => {
            Display::Unavailable { reason: format!("the server failed: {reason}") }
        }
    }
}

/// `present`, at the one type the conformance protocol can carry.
///
/// The function is generic because what a server answers with varies; a
/// binding needs one concrete type, and the branch being checked is the same
/// one for every `T`.
///
/// @implements REQ-LSP.absent_server_named
/// @drt REQ-LSP.absent_server_named
pub fn present_text(state: ServerState, answer: Option<String>) -> Display<String> {
    present(&state, answer)
}

/// The position encoding to use for a language, taken from what its server
/// declared.
///
/// A server per language, and the encoding read off the running one. Assuming
/// UTF-16 because the protocol's default is UTF-16 is the bug this exists to
/// prevent: a server that declared UTF-8 would then be sent positions it
/// cannot interpret, and the edits would land in the wrong place on exactly
/// the lines with non-ASCII text.
///
/// A language with no entry is named as absent rather than answered with a
/// default, because a default here is a wrong answer that looks like a right
/// one.
///
/// @implements REQ-LSP.registry_per_language
/// @implements REQ-LSP.encoding_declared
/// @implements REQ-LSP.absent_server_named
/// @drt REQ-LSP.registry_per_language
/// @drt REQ-LSP.encoding_declared
pub fn encoding_for(registry: Vec<(String, ServerState)>, language: String) -> Display<Encoding> {
    // First match, so a registry that names a language twice resolves the way
    // every other association list in this project does.
    match registry.into_iter().find(|(name, _)| *name == language) {
        None => Display::Unavailable {
            reason: format!("no server is registered for {language}"),
        },
        Some((_, state)) => match &state {
            ServerState::Running { encoding } => Display::Result { value: *encoding },
            _ => present(&state, None::<Encoding>),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class of bug this closes: an off-by-some on exactly the lines with
    /// non-ASCII text.
    ///
    /// @tests REQ-LSP.encoding_round_trip
    #[test]
    fn positions_round_trip_in_every_encoding() {
        for line in ["plain ascii", "héllo wörld", "emoji 😀 here", "日本語のテキスト", ""] {
            for encoding in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
                for (offset, _) in line.char_indices().chain(std::iter::once((line.len(), ' '))) {
                    let character = to_character(line.to_string(), offset, encoding)
                        .unwrap_or_else(|| panic!("{line:?} {offset} {encoding:?}"));
                    assert_eq!(
                        to_byte_offset(line.to_string(), character as u32, encoding),
                        Some(offset),
                        "{line:?} at {offset} in {encoding:?}"
                    );
                }
            }
        }
    }

    /// The encodings genuinely differ, so taking the server's word matters.
    #[test]
    fn the_encodings_disagree_where_it_counts() {
        let line = "😀x".to_string();
        assert_eq!(to_character(line.clone(), 4, Encoding::Utf8), Some(4));
        assert_eq!(to_character(line.clone(), 4, Encoding::Utf16), Some(2));
        assert_eq!(to_character(line, 4, Encoding::Utf32), Some(1));
    }

    /// A column inside a character is a position no buffer has.
    #[test]
    fn a_column_inside_a_character_is_refused() {
        assert_eq!(to_byte_offset("😀".into(), 1, Encoding::Utf16), None);
        assert_eq!(to_character("😀".into(), 2, Encoding::Utf8), None);
    }

    /// @tests REQ-LSP.edits_ordered
    #[test]
    fn edits_apply_back_to_front_so_offsets_stay_valid() {
        let content = "abcdefgh".to_string();
        let edits = vec![
            Edit { start: 0, end: 2, text: "XY".into() },
            Edit { start: 6, end: 8, text: "Z".into() },
        ];
        let commands = lower("f.rs".into(), content.clone(), edits).unwrap();

        let mut workspace = crate::history::command::Workspace::with(&[("f.rs", &content)]);
        for command in &commands {
            workspace = crate::history::command::apply(&workspace, command).unwrap();
        }
        assert_eq!(workspace.get("f.rs").unwrap(), "XYcdefZ");
    }

    /// @tests REQ-LSP.overlap_refused
    #[test]
    fn overlapping_edits_are_refused_not_ordered_arbitrarily() {
        let edits = vec![
            Edit { start: 0, end: 4, text: "X".into() },
            Edit { start: 2, end: 6, text: "Y".into() },
        ];
        assert!(matches!(
            lower("f.rs".into(), "abcdefgh".into(), edits),
            Err(LowerError::Overlap { .. })
        ));
    }

    /// Two edits meeting at a boundary describe one result, not two.
    #[test]
    fn touching_edits_are_allowed() {
        let edits = vec![
            Edit { start: 0, end: 2, text: "X".into() },
            Edit { start: 2, end: 4, text: "Y".into() },
        ];
        assert!(lower("f.rs".into(), "abcdefgh".into(), edits).is_ok());
    }

    #[test]
    fn an_edit_outside_the_file_is_refused() {
        let edits = vec![Edit { start: 0, end: 99, text: "X".into() }];
        assert!(matches!(
            lower("f.rs".into(), "abc".into(), edits),
            Err(LowerError::OutOfRange { .. })
        ));
    }

    /// For a proof assistant, an empty goal list means the proof is complete.
    ///
    /// @tests REQ-LSP.absent_server_named
    #[test]
    fn no_server_is_never_rendered_as_an_empty_result() {
        let goals: Vec<String> = vec![];
        assert_eq!(
            present(&ServerState::NotStarted, Some(goals.clone())),
            Display::Unavailable { reason: "no server is running for this language".into() }
        );
        // A running server with no goals is the real "proof complete".
        assert_eq!(
            present(&ServerState::Running { encoding: Encoding::Utf16 }, Some(goals.clone())),
            Display::Result { value: goals }
        );
    }
}
