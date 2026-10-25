use serde::{Deserialize, Serialize};

use super::command::{Command, Workspace};
use super::tree::{NodeId, Tree};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Origin {
    Node { node: NodeId },
    /// @implements REQ-PROV.base_is_honest
    Base,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Moved(usize),
    WrittenHere,
    Renamed(String),
    FileCreatedHere,
}

/// @implements REQ-PROV.mapping_before
/// @implements REQ-PROV.mapping_after
/// @implements REQ-PROV.mapping_inside
/// @implements REQ-PROV.exact_not_heuristic
fn back(command: &Command, file: &str, position: usize) -> Step {
    match command {
        Command::Insert { file: target, offset, text } if target == file => {
            if position < *offset {
                Step::Moved(position)
            } else if position < offset + text.len() {
                Step::WrittenHere
            } else {
                Step::Moved(position - text.len())
            }
        }
        Command::Delete { file: target, offset, deleted } if target == file => {
            if position < *offset {
                Step::Moved(position)
            } else {
                Step::Moved(position + deleted.len())
            }
        }
        Command::CreateFile { path } if path == file => Step::FileCreatedHere,
        Command::DeleteFile { path, .. } if path == file => Step::FileCreatedHere,
        Command::RenameFile { from, to } if to == file => Step::Renamed(from.clone()),
        Command::Batch { commands } => {
            let mut current = position;
            let mut name = file.to_string();
            for member in commands.iter().rev() {
                match back(member, &name, current) {
                    Step::Moved(next) => current = next,
                    Step::WrittenHere => return Step::WrittenHere,
                    Step::FileCreatedHere => return Step::FileCreatedHere,
                    Step::Renamed(previous) => name = previous,
                }
            }
            if name == file {
                Step::Moved(current)
            } else {
                Step::Renamed(name)
            }
        }
        _ => Step::Moved(position),
    }
}

/// @implements REQ-PROV.position_question
pub fn origin(tree: &Tree, at: NodeId, file: &str, position: usize) -> Origin {
    let mut name = file.to_string();
    let mut current = position;

    for node_id in tree.ancestry(at).into_iter().rev() {
        let Some(node) = tree.node(node_id) else { break };
        match back(&node.command, &name, current) {
            Step::WrittenHere => return Origin::Node { node: node_id },
            Step::FileCreatedHere => return Origin::Node { node: node_id },
            Step::Moved(next) => current = next,
            Step::Renamed(previous) => name = previous,
        }
    }
    Origin::Base
}

/// @implements REQ-PROV.position_question
/// @drt REQ-PROV.position_question
/// @drt REQ-PROV.mapping_before
/// @drt REQ-PROV.mapping_after
/// @drt REQ-PROV.mapping_inside
/// @drt REQ-PROV.exact_not_heuristic
/// @drt REQ-PROV.base_is_honest
pub fn origins(
    base: Workspace,
    script: Vec<super::tree::Step>,
    file: String,
    position: usize,
) -> Vec<Origin> {
    let tree = super::tree::from_script(base, script);
    tree.ids().into_iter().map(|id| origin(&tree, id, &file, position)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::command::Workspace;

    fn insert(file: &str, offset: usize, text: &str) -> Command {
        Command::Insert { file: file.into(), offset, text: text.into() }
    }

    /// @tests REQ-PROV.position_question
    /// @tests REQ-PROV.mapping_inside
    #[test]
    fn a_position_lands_on_the_edit_that_wrote_it() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "")]));
        let first = t.push(insert("a.rs", 0, "hello")).unwrap();
        let second = t.push(insert("a.rs", 5, " world")).unwrap();
        let at = t.current().unwrap();

        assert_eq!(origin(&t, at, "a.rs", 0), Origin::Node { node: first });
        assert_eq!(origin(&t, at, "a.rs", 4), Origin::Node { node: first });
        assert_eq!(origin(&t, at, "a.rs", 5), Origin::Node { node: second });
        assert_eq!(origin(&t, at, "a.rs", 10), Origin::Node { node: second });
    }

    /// @tests REQ-PROV.mapping_after
    #[test]
    fn text_pushed_along_is_still_attributed_to_who_wrote_it() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "")]));
        let first = t.push(insert("a.rs", 0, "world")).unwrap();
        t.push(insert("a.rs", 0, "hello ")).unwrap();
        let at = t.current().unwrap();
        assert_eq!(origin(&t, at, "a.rs", 6), Origin::Node { node: first });
    }

    /// @tests REQ-PROV.base_is_honest
    #[test]
    fn text_that_arrived_with_the_file_reports_as_the_base() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "original")]));
        t.push(insert("a.rs", 8, " more")).unwrap();
        let at = t.current().unwrap();
        assert_eq!(origin(&t, at, "a.rs", 0), Origin::Base);
        assert_ne!(origin(&t, at, "a.rs", 8), Origin::Base);
    }

    #[test]
    fn a_deletion_is_never_the_origin_of_text() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "")]));
        let written = t.push(insert("a.rs", 0, "abcdef")).unwrap();
        t.push(Command::Delete {
            file: "a.rs".into(),
            offset: 1,
            deleted: "bc".into(),
        })
        .unwrap();
        let at = t.current().unwrap();
        assert_eq!(origin(&t, at, "a.rs", 1), Origin::Node { node: written });
    }

    #[test]
    fn a_rename_is_followed_backwards() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "")]));
        let written = t.push(insert("a.rs", 0, "text")).unwrap();
        t.push(Command::RenameFile { from: "a.rs".into(), to: "b.rs".into() }).unwrap();
        let at = t.current().unwrap();
        assert_eq!(origin(&t, at, "b.rs", 0), Origin::Node { node: written });
    }

    #[test]
    fn a_batch_is_read_in_reverse() {
        let mut t = Tree::new(Workspace::with(&[("a.rs", "")]));
        let batch = t
            .push(Command::Batch {
                commands: vec![insert("a.rs", 0, "one"), insert("a.rs", 3, "two")],
            })
            .unwrap();
        let at = t.current().unwrap();
        assert_eq!(origin(&t, at, "a.rs", 0), Origin::Node { node: batch });
        assert_eq!(origin(&t, at, "a.rs", 4), Origin::Node { node: batch });
    }
}
