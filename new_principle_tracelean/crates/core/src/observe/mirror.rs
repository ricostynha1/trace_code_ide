//! Turning what a sandboxed workspace did into commands.
//!
//! The diff is a function from two tree snapshots to a list of mutations — no
//! filesystem, no watcher, no timing — and its law is that applying its output
//! to the first snapshot yields the second. A subsystem that looks inherently
//! effectful is therefore a direct differential test.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::history::command::{apply, Command, Workspace};

use super::policy::is_mirrored;

/// The commands that carry `before` to `after`, for the paths that are
/// mirrored at all.
///
/// Ordered so that applying them in sequence never depends on a state that does
/// not yet exist: a file is created before it is written to, and a file is
/// deleted only after nothing else refers to it.
///
/// @implements REQ-MIRROR.diff_is_pure
/// @implements REQ-MIRROR.minimal
/// @implements REQ-MIRROR.protected_excluded
/// @implements REQ-MIRROR.ordering_defined
/// @drt REQ-MIRROR.diff_is_pure
/// @drt REQ-MIRROR.minimal
/// @drt REQ-MIRROR.ordering_defined
/// @drt REQ-MIRROR.protected_excluded
pub fn mutations(before: Workspace, after: Workspace) -> Vec<Command> {
    let paths: BTreeSet<&String> = before
        .files
        .keys()
        .chain(after.files.keys())
        .filter(|path| is_mirrored(path))
        .collect();

    let mut removed = Vec::new();
    let mut created = Vec::new();
    let mut changed = Vec::new();

    // Sorted, because the mutation list is part of what the user reviews and a
    // diff that reorders itself between runs cannot be read.
    for path in paths {
        match (before.files.get(path), after.files.get(path)) {
            (Some(old), None) => {
                removed.push(Command::DeleteFile { path: path.clone(), content: old.clone() });
            }
            (None, Some(new)) => {
                created.push(Command::CreateFile { path: path.clone() });
                if !new.is_empty() {
                    created.push(Command::Insert {
                        file: path.clone(),
                        offset: 0,
                        text: new.clone(),
                    });
                }
            }
            (Some(old), Some(new)) if old != new => {
                // Replacement rather than a computed minimal edit: the mirror
                // reports what a tool left behind, and inventing a smaller edit
                // that reaches the same text would attribute to the tool an
                // intention it did not express.
                if !old.is_empty() {
                    changed.push(Command::Delete {
                        file: path.clone(),
                        offset: 0,
                        deleted: old.clone(),
                    });
                }
                if !new.is_empty() {
                    changed.push(Command::Insert {
                        file: path.clone(),
                        offset: 0,
                        text: new.clone(),
                    });
                }
            }
            // Unchanged, or absent from both: no mutation. An entry whose
            // removal still reproduces `after` must not be emitted.
            _ => {}
        }
    }

    let mut out = Vec::new();
    out.extend(removed);
    out.extend(created);
    out.extend(changed);
    out
}

/// The state reached by mirroring, in one call.
///
/// Exists so the law is a single function on both sides of a differential
/// test: applying the derived mutations to the original tree yields the
/// observed tree, for everything that is mirrored at all.
///
/// @implements REQ-MIRROR.apply_reproduces
/// @drt REQ-MIRROR.apply_reproduces
pub fn mirrored(before: Workspace, after: Workspace) -> Workspace {
    let mut workspace = before.clone();
    for command in mutations(before, after) {
        match apply(&workspace, &command) {
            Ok(next) => workspace = next,
            // A refusal here would mean the derived mutations do not fit the
            // state they were derived from, which is the law failing. Returning
            // what was reached lets the comparison show it rather than hiding
            // it behind a panic.
            Err(_) => return workspace,
        }
    }
    workspace
}

/// A write this system performed, to be ignored when it is observed coming
/// back.
///
/// @implements REQ-SELFWRITE.content_matched
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfWrite {
    pub path: String,
    /// What was written. Matching on the path alone would swallow a genuine
    /// external change to a file this system happened to touch.
    pub content: String,
    /// Observations this may still suppress. A suppression for a write that is
    /// never observed would otherwise block the next real change forever.
    ///
    /// @implements REQ-SELFWRITE.no_deadlock
    pub remaining: u32,
}

/// Whether an observation is this system's own write coming back, and the
/// pending set with that suppression consumed.
///
/// @implements REQ-SELFWRITE.own_writes_ignored
/// @implements REQ-SELFWRITE.suppression_is_consumed
/// @implements REQ-SELFWRITE.unmatched_is_external
/// @drt REQ-SELFWRITE.content_matched
/// @drt REQ-SELFWRITE.suppression_is_consumed
/// @drt REQ-SELFWRITE.unmatched_is_external
pub fn suppress(pending: Vec<SelfWrite>, path: String, content: String) -> Suppression {
    let mut rest = Vec::new();
    let mut suppressed = false;

    for mut write in pending {
        if !suppressed && write.path == path && write.content == content {
            suppressed = true;
            // Consumed: one suppression covers one observation, so a second
            // identical change from outside is still seen.
            write.remaining = write.remaining.saturating_sub(1);
            if write.remaining > 0 {
                rest.push(write);
            }
            continue;
        }
        rest.push(write);
    }

    Suppression { suppressed, pending: rest }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suppression {
    /// True when this observation was this system's own write.
    pub suppressed: bool,
    pub pending: Vec<SelfWrite>,
}

/// Age every pending suppression by one round, dropping those that expired.
///
/// @implements REQ-SELFWRITE.no_deadlock
/// @drt REQ-SELFWRITE.no_deadlock
pub fn expire(pending: Vec<SelfWrite>) -> Vec<SelfWrite> {
    pending
        .into_iter()
        .filter_map(|mut write| {
            write.remaining = write.remaining.saturating_sub(1);
            (write.remaining > 0).then_some(write)
        })
        .collect()
}

/// What a snapshot saw at one path.
///
/// Three states, not two. A file that exists and is not text — a binary, a file
/// with invalid UTF-8, anything the editor has no representation for — is
/// neither absent nor content, and collapsing it into either is how a mirror
/// either loses a change or writes rubbish over one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FileState {
    Absent,
    Text { content: String },
    /// Present, and not representable as text.
    Opaque,
}

/// What the mirror does about one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Change {
    /// A command the editor can apply.
    Mirror { command: Command },
    /// Something changed that cannot be carried across as text. Reported so a
    /// person can look, and never mirrored — writing a binary's bytes into a
    /// text buffer would corrupt the file on the way back.
    ///
    /// @implements REQ-MIRROR.binary_handled
    ReportOnly { path: String },
}

/// What the mirror does about one path, given what the two snapshots saw.
///
/// The opaque cases are the point. A change involving a file the editor cannot
/// represent is *reported* — it happened, and the user needs to know — but
/// never turned into commands. The alternative, silently skipping it, is a
/// change the user is told nothing about, which is worse than a change they
/// cannot review in place.
///
/// @implements REQ-MIRROR.binary_handled
/// @implements REQ-MIRROR.protected_excluded
/// @drt REQ-MIRROR.binary_handled
pub fn change_at(path: String, before: FileState, after: FileState) -> Vec<Change> {
    if !is_mirrored(&path) {
        return Vec::new();
    }
    match (&before, &after) {
        (FileState::Absent, FileState::Absent) => Vec::new(),
        // Either side opaque: report if anything changed, and never mirror.
        // Two opaque snapshots are indistinguishable to this layer, so the
        // honest answer is that nothing is known to have changed.
        (FileState::Opaque, FileState::Opaque) => Vec::new(),
        (FileState::Opaque, _) | (_, FileState::Opaque) => {
            vec![Change::ReportOnly { path }]
        }
        (FileState::Absent, FileState::Text { content }) => {
            let mut out = vec![Change::Mirror { command: Command::CreateFile { path: path.clone() } }];
            if !content.is_empty() {
                out.push(Change::Mirror {
                    command: Command::Insert { file: path, offset: 0, text: content.clone() },
                });
            }
            out
        }
        (FileState::Text { content }, FileState::Absent) => {
            vec![Change::Mirror {
                command: Command::DeleteFile { path, content: content.clone() },
            }]
        }
        (FileState::Text { content: old }, FileState::Text { content: new }) => {
            if old == new {
                return Vec::new();
            }
            let mut out = Vec::new();
            if !old.is_empty() {
                out.push(Change::Mirror {
                    command: Command::Delete { file: path.clone(), offset: 0, deleted: old.clone() },
                });
            }
            if !new.is_empty() {
                out.push(Change::Mirror {
                    command: Command::Insert { file: path, offset: 0, text: new.clone() },
                });
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(files: &[(&str, &str)]) -> Workspace {
        Workspace::with(files)
    }

    /// The law.
    ///
    /// @tests REQ-MIRROR.apply_reproduces
    #[test]
    fn applying_the_mutations_reproduces_what_was_observed() {
        let before = ws(&[("a.rs", "one"), ("b.rs", "two"), ("gone.rs", "x")]);
        let after = ws(&[("a.rs", "ONE"), ("b.rs", "two"), ("new.rs", "three")]);
        assert_eq!(mirrored(before, after.clone()), after);
    }

    /// @tests REQ-MIRROR.minimal
    #[test]
    fn an_unchanged_file_produces_nothing() {
        let same = ws(&[("a.rs", "one")]);
        assert!(mutations(same.clone(), same).is_empty());
    }

    /// Every emitted mutation must be load-bearing.
    #[test]
    fn no_mutation_can_be_dropped() {
        let before = ws(&[("a.rs", "one"), ("gone.rs", "x")]);
        let after = ws(&[("a.rs", "ONE"), ("new.rs", "n")]);
        let all = mutations(before.clone(), after.clone());
        for skip in 0..all.len() {
            let mut workspace = before.clone();
            for (index, command) in all.iter().enumerate() {
                if index == skip {
                    continue;
                }
                if let Ok(next) = apply(&workspace, command) {
                    workspace = next;
                }
            }
            assert_ne!(workspace, after, "mutation {skip} was not needed");
        }
    }

    /// @tests REQ-MIRROR.protected_excluded
    #[test]
    fn nothing_under_a_protected_path_is_mirrored() {
        let before = ws(&[(".git/HEAD", "old"), ("a.rs", "one")]);
        let after = ws(&[(".git/HEAD", "rewritten"), ("a.rs", "two")]);
        let commands = mutations(before, after);
        assert!(
            !commands.iter().any(|c| format!("{c:?}").contains(".git")),
            "{commands:?}"
        );
        assert_eq!(commands.len(), 2, "the ordinary file still changed");
    }

    /// @tests REQ-MIRROR.ordering_defined
    #[test]
    fn a_file_is_created_before_it_is_written_to() {
        let commands = mutations(ws(&[]), ws(&[("new.rs", "content")]));
        assert!(matches!(commands[0], Command::CreateFile { .. }));
        assert!(matches!(commands[1], Command::Insert { .. }));
        // And the sequence applies without refusal.
        assert_eq!(mirrored(ws(&[]), ws(&[("new.rs", "content")])).get("new.rs").unwrap(), "content");
    }

    #[test]
    fn build_output_is_not_mirrored() {
        let commands = mutations(ws(&[]), ws(&[("target/debug/x", "binary")]));
        assert!(commands.is_empty(), "{commands:?}");
    }

    fn pending(items: &[(&str, &str)]) -> Vec<SelfWrite> {
        items
            .iter()
            .map(|(p, c)| SelfWrite {
                path: p.to_string(),
                content: c.to_string(),
                remaining: 1,
            })
            .collect()
    }

    /// @tests REQ-SELFWRITE.own_writes_ignored
    /// @tests REQ-SELFWRITE.suppression_is_consumed
    #[test]
    fn a_self_write_is_ignored_once_and_then_seen() {
        let first = suppress(pending(&[("a.rs", "x")]), "a.rs".into(), "x".into());
        assert!(first.suppressed);
        assert!(first.pending.is_empty(), "the suppression was consumed");

        // The same change arriving again is external.
        let second = suppress(first.pending, "a.rs".into(), "x".into());
        assert!(!second.suppressed);
    }

    /// Matching on the path alone would swallow a real change.
    ///
    /// @tests REQ-SELFWRITE.content_matched
    #[test]
    fn a_different_change_to_the_same_path_is_external() {
        let result = suppress(pending(&[("a.rs", "ours")]), "a.rs".into(), "theirs".into());
        assert!(!result.suppressed);
        assert_eq!(result.pending.len(), 1, "the suppression is still pending");
    }

    /// @tests REQ-SELFWRITE.unmatched_is_external
    #[test]
    fn an_unrelated_change_is_external() {
        let result = suppress(pending(&[("a.rs", "x")]), "b.rs".into(), "x".into());
        assert!(!result.suppressed);
    }

    /// A write that is never observed must not block the next real change.
    ///
    /// @tests REQ-SELFWRITE.no_deadlock
    #[test]
    fn a_suppression_that_is_never_observed_expires() {
        let mut waiting = pending(&[("a.rs", "x")]);
        waiting = expire(waiting);
        assert!(waiting.is_empty());
        assert!(!suppress(waiting, "a.rs".into(), "x".into()).suppressed);
    }
}
