//! "Who wrote this line?" — answered from the undo tree rather than from git.
//!
//! TraceLean's claim is that the undo tree is one history of everything, which
//! is only useful if you can ask it a question about a *position* and not just
//! about a node. `t u` in Trace mode puts the cursor on a line and lands on the
//! moment that line was written.
//!
//! The method is exact for the command model we have. Walking one `Replace`
//! backwards maps a char position in the post-edit buffer to the pre-edit one:
//!
//! - before the edit → unchanged
//! - after the replacement → shifted by `old.len() - new.len()`
//! - *inside* the replacement → this is the edit that wrote it; stop here
//!
//! No replay, no heuristics, and no dependence on the buffer's current
//! contents. The one thing it cannot see is text that came in with the file
//! rather than being typed: that reports as the file's base node, which is the
//! honest answer ("this is how the file arrived") rather than a wrong one.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::commands::Command;
use crate::undo_tree::{NodeId, UndoTree};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub node_id: NodeId,
    pub timestamp: DateTime<Utc>,
    /// Name of the commit point this edit belongs to, when it has one.
    pub commit_point: Option<String>,
    /// The file as it was named at that point in history (renames are followed).
    pub file: PathBuf,
    /// Char offset of the edit in the buffer as it was *after* that edit.
    pub at: usize,
    /// What the edit put there.
    pub inserted: String,
    /// What it replaced (empty for a pure insert).
    pub removed: String,
    /// True when the answer is "the file arrived like this", not "someone
    /// typed it" — distinguished so the UI never claims authorship it lacks.
    pub is_origin: bool,
}

/// Flatten `Batch` so a grouped edit is searched command by command.
fn flatten<'a>(cmd: &'a Command, out: &mut Vec<&'a Command>) {
    match cmd {
        Command::Batch { commands } => {
            for c in commands {
                flatten(c, out);
            }
        }
        other => out.push(other),
    }
}

/// Find the edit that produced the character at `char_pos` in `file`.
///
/// `char_pos` is an offset into the buffer as it stands now; the walk maps it
/// backwards through history. Returns `None` when the tree holds no edit that
/// covers the position (an empty history, or a position past the end).
pub fn provenance_at(tree: &UndoTree, file: &Path, char_pos: usize) -> Option<Provenance> {
    let mut path = file.to_path_buf();
    let mut pos = char_pos;
    let mut node = tree.current_node()?;

    loop {
        let mut commands = Vec::new();
        flatten(&node.command, &mut commands);
        // Within a node the commands were applied front to back, so walking
        // backwards means visiting them back to front.
        for cmd in commands.iter().rev() {
            match cmd {
                Command::Replace { file: f, at, old, new } if *f == path => {
                    let new_len = new.chars().count();
                    let old_len = old.chars().count();
                    if pos < *at {
                        // Before the edit: untouched by it.
                    } else if pos < at + new_len {
                        return Some(Provenance {
                            node_id: node.id,
                            timestamp: node.timestamp,
                            commit_point: node.commit_point.as_ref().map(|c| c.name.clone()),
                            file: path.clone(),
                            at: *at,
                            inserted: new.clone(),
                            removed: old.clone(),
                            is_origin: node.parent.is_none(),
                        });
                    } else {
                        pos = pos - new_len + old_len;
                    }
                }
                Command::CreateFile { path: p } if *p == path => {
                    // The file begins here and nothing earlier can own the
                    // position: report the creation as the origin.
                    return Some(Provenance {
                        node_id: node.id,
                        timestamp: node.timestamp,
                        commit_point: node.commit_point.as_ref().map(|c| c.name.clone()),
                        file: path.clone(),
                        at: 0,
                        inserted: String::new(),
                        removed: String::new(),
                        is_origin: true,
                    });
                }
                Command::RenameFile { from, to } if *to == path => {
                    // Walking backwards past a rename: the file was called
                    // `from` before this point.
                    path = from.clone();
                }
                _ => {}
            }
        }
        match node.parent.and_then(|id| tree.get_node(id)) {
            Some(parent) => node = parent,
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn edit(state: &mut AppState, file: &str, at: usize, old: &str, new: &str) {
        state
            .apply(Command::Replace {
                file: PathBuf::from(file),
                at,
                old: old.into(),
                new: new.into(),
            })
            .unwrap();
    }

    fn setup() -> AppState {
        let mut state = AppState::new();
        state.set_coalescing(false);
        state.load_file(PathBuf::from("a.rs"), String::new());
        state
    }

    #[test]
    fn a_position_is_attributed_to_the_edit_that_typed_it() {
        let mut state = setup();
        edit(&mut state, "a.rs", 0, "", "hello ");
        edit(&mut state, "a.rs", 6, "", "world");
        let p = provenance_at(state.undo_tree(), Path::new("a.rs"), 8).unwrap();
        assert_eq!(p.inserted, "world");
    }

    #[test]
    fn earlier_text_is_still_found_after_later_inserts_shift_it() {
        let mut state = setup();
        edit(&mut state, "a.rs", 0, "", "world");
        // Insert in front: "world" now starts at 6, not 0.
        edit(&mut state, "a.rs", 0, "", "hello ");
        let p = provenance_at(state.undo_tree(), Path::new("a.rs"), 7).unwrap();
        assert_eq!(
            p.inserted, "world",
            "the walk must undo the offset shift the later insert caused"
        );
    }

    #[test]
    fn a_replacement_owns_the_text_it_left_behind() {
        let mut state = setup();
        edit(&mut state, "a.rs", 0, "", "let x = 1;");
        edit(&mut state, "a.rs", 8, "1", "42");
        let p = provenance_at(state.undo_tree(), Path::new("a.rs"), 9).unwrap();
        assert_eq!(p.inserted, "42");
        assert_eq!(p.removed, "1");
    }

    #[test]
    fn renames_are_followed_backwards() {
        let mut state = setup();
        edit(&mut state, "a.rs", 0, "", "fn main() {}");
        state
            .apply(Command::RenameFile {
                from: PathBuf::from("a.rs"),
                to: PathBuf::from("b.rs"),
            })
            .unwrap();
        let p = provenance_at(state.undo_tree(), Path::new("b.rs"), 3).unwrap();
        assert_eq!(p.inserted, "fn main() {}");
        assert_eq!(
            p.file,
            PathBuf::from("a.rs"),
            "the answer names the file as it was called when the text was written"
        );
    }

    #[test]
    fn a_position_nobody_wrote_has_no_provenance() {
        let mut state = setup();
        edit(&mut state, "a.rs", 0, "", "hi");
        assert!(provenance_at(state.undo_tree(), Path::new("a.rs"), 99).is_none());
        assert!(provenance_at(state.undo_tree(), Path::new("other.rs"), 0).is_none());
    }
}
