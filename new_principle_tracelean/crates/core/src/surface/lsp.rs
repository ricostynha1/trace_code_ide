use serde::{Deserialize, Serialize};

use crate::history::command::Command;

/// @implements REQ-LSP.encoding_declared
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// @implements REQ-LSP.column_to_offset
/// @drt REQ-LSP.column_to_offset
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
            return None;
        }
    }
    (counted == character).then_some(line_text.len())
}

/// @implements REQ-LSP.offset_to_column
/// @drt REQ-LSP.offset_to_column
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LowerError {
    Overlap { first: usize, second: usize },
    OutOfRange { start: usize, end: usize },
    Inverted { start: usize, end: usize },
}

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
        if pair[1].start < pair[0].end {
            return Err(LowerError::Overlap { first: pair[0].start, second: pair[1].start });
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lowered {
    Commands { commands: Vec<Command> },
    Refused { error: LowerError },
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Display<T> {
    Result { value: T },
    Unavailable { reason: String },
}

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

/// @implements REQ-LSP.absent_server_named
/// @drt REQ-LSP.absent_server_named
pub fn present_text(state: ServerState, answer: Option<String>) -> Display<String> {
    present(&state, answer)
}

/// @implements REQ-LSP.registry_per_language
/// @implements REQ-LSP.encoding_declared
/// @implements REQ-LSP.absent_server_named
/// @drt REQ-LSP.registry_per_language
/// @drt REQ-LSP.encoding_declared
pub fn encoding_for(registry: Vec<(String, ServerState)>, language: String) -> Display<Encoding> {
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

    /// @tests REQ-LSP.overlap_refused
    /// @tests REQ-LSP.edits_ordered
    #[test]
    fn a_lowering_is_commands_or_a_refusal() {
        let edit = |start, end| Edit { start, end, text: "x".into() };
        let lowered = lower_outcome("a.rs".into(), "abcd".into(), vec![edit(0, 1), edit(2, 3)]);
        let Lowered::Commands { commands } = lowered else { panic!("{lowered:?}") };
        let offsets: Vec<usize> = commands
            .iter()
            .map(|c| match c {
                Command::Delete { offset, .. } | Command::Insert { offset, .. } => *offset,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(offsets, vec![2, 2, 0, 0]);
        let refused = lower_outcome("a.rs".into(), "abcd".into(), vec![edit(0, 2), edit(1, 3)]);
        assert!(matches!(refused, Lowered::Refused { error: LowerError::Overlap { .. } }), "{refused:?}");
    }

    /// @tests REQ-LSP.offset_to_column
    /// @tests REQ-LSP.column_to_offset
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

    #[test]
    fn the_encodings_disagree_where_it_counts() {
        let line = "😀x".to_string();
        assert_eq!(to_character(line.clone(), 4, Encoding::Utf8), Some(4));
        assert_eq!(to_character(line.clone(), 4, Encoding::Utf16), Some(2));
        assert_eq!(to_character(line, 4, Encoding::Utf32), Some(1));
    }

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

    /// @tests REQ-LSP.absent_server_named
    #[test]
    fn no_server_is_never_rendered_as_an_empty_result() {
        let goals: Vec<String> = vec![];
        assert_eq!(
            present(&ServerState::NotStarted, Some(goals.clone())),
            Display::Unavailable { reason: "no server is running for this language".into() }
        );
        assert_eq!(
            present(&ServerState::Running { encoding: Encoding::Utf16 }, Some(goals.clone())),
            Display::Result { value: goals }
        );
    }
}
