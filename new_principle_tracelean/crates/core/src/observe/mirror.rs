//! Turning what a sandboxed workspace did into commands.
//!
//! The diff is a function from two tree snapshots to a list of mutations — no
//! filesystem, no watcher, no timing — and its law is that applying its output
//! to the first snapshot yields the second. A subsystem that looks inherently
//! effectful is therefore a direct differential test.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

/// The commands that carry the project to the agent's copy, on the paths the
/// agent changed: those whose content in the copy (its hash, or absence) is
/// not what `base` says the copy started with.
///
/// The copy is not the project. A project edited after its copy was made
/// differs from the copy on every path it touched, and a plain `mutations`
/// would offer each as the agent's work: accepting a weeks-old copy unwrote
/// the project. With no base, nothing can be told apart, so nothing is offered.
///
/// @implements REQ-OBS.only_what_the_tool_changed
pub fn changed_since(base: Option<BTreeMap<String, String>>, project: Workspace, agent: Workspace) -> Vec<Command> {
    let Some(base) = base else { return Vec::new() };
    let touched = |path: &String| {
        agent.files.get(path).map(|text| crate::trace::hash::text(text)).as_ref() != base.get(path)
    };
    mutations(project, agent.clone())
        .into_iter()
        .filter(|command| {
            let path = command_path(command);
            path.is_empty() || touched(&path)
        })
        .collect()
}

/// `changed_since` with the start as pairs, as the model reads it: a map does
/// not cross as a list, and a path listed twice is read at its last entry.
///
/// @drt REQ-OBS.only_what_the_tool_changed
pub fn changed_since_owned(base: Option<Vec<(String, String)>>, project: Workspace, agent: Workspace) -> Vec<Command> {
    changed_since(base.map(|pairs| pairs.into_iter().collect()), project, agent)
}

/// The file a command acts on. `mutations` emits neither a rename nor a batch;
/// were it to, it would be offered rather than silently dropped.
fn command_path(command: &Command) -> String {
    match command {
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.clone(),
        Command::Insert { file, .. } | Command::Delete { file, .. } => file.clone(),
        Command::RenameFile { .. } | Command::Batch { .. } => String::new(),
    }
}

/// What mirroring reached, and every derived command refused on the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirroring {
    pub reached: Workspace,
    /// A refusal means the derived mutations did not fit the state they were
    /// derived from — the law failing — and is named rather than skipped.
    pub refused: Vec<Command>,
}

/// The state reached by mirroring, and what was refused, in one call.
///
/// Exists so the law is a single function on both sides of a differential
/// test: applying the derived mutations to the original tree yields the
/// observed tree on every mirrored path, the original on every other, and no
/// refusal.
///
/// @implements REQ-MIRROR.apply_reproduces
/// @drt REQ-MIRROR.apply_reproduces
pub fn mirrored(before: Workspace, after: Workspace) -> Mirroring {
    let mut out = Mirroring { reached: before.clone(), refused: Vec::new() };
    for command in mutations(before, after) {
        match apply(&out.reached, &command) {
            Ok(next) => out.reached = next,
            // Recorded and passed over, so one refusal does not hide what the
            // rest of the mutations would have done.
            Err(_) => out.refused.push(command),
        }
    }
    out
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
    /// Rounds this may still wait to be observed before it expires. A
    /// suppression for a write that is never observed would otherwise block
    /// the next real change forever. Spent by `expire`, never by a match: a
    /// match consumes the whole write.
    ///
    /// @implements REQ-SELFWRITE.no_deadlock
    pub age: u32,
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

    for write in pending {
        if !suppressed && write.path == path && write.content == content {
            // Consumed whole, whatever its age: one write covers one
            // observation, so a second identical change from outside is seen.
            suppressed = true;
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
            write.age = write.age.saturating_sub(1);
            (write.age > 0).then_some(write)
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
    /// Present, and not representable as text: known by a hash of its bytes,
    /// so that a changed binary can be told from an unchanged one.
    Opaque { hash: String },
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
/// @drt REQ-MIRROR.binary_handled
pub fn change_at(path: String, before: FileState, after: FileState) -> Vec<Change> {
    if !is_mirrored(&path) {
        return Vec::new();
    }
    match (&before, &after) {
        (FileState::Absent, FileState::Absent) => Vec::new(),
        // Either side opaque: report if anything changed, and never mirror.
        // Two opaque snapshots are told apart by the hash of their bytes.
        (FileState::Opaque { hash: old }, FileState::Opaque { hash: new }) => {
            if old == new {
                Vec::new()
            } else {
                vec![Change::ReportOnly { path }]
            }
        }
        (FileState::Opaque { .. }, _) | (_, FileState::Opaque { .. }) => {
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

    fn base(files: &[(&str, &str)]) -> Option<BTreeMap<String, String>> {
        Some(files.iter().map(|(p, t)| (p.to_string(), crate::trace::hash::text(t))).collect())
    }

    /// The bug it is for: a copy made before the project grew a file and
    /// changed another offered to delete the one and unwrite the other.
    ///
    /// @tests REQ-OBS.only_what_the_tool_changed
    #[test]
    fn what_the_project_changed_since_the_copy_is_not_the_agents() {
        let copied = &[("a.rs", "one"), ("b.rs", "two")];
        let project = ws(&[("a.rs", "one, since edited"), ("b.rs", "two"), ("CLAUDE.md", "new")]);
        let agent = ws(&[("a.rs", "one"), ("b.rs", "TWO"), ("c.rs", "made")]);
        let offered = changed_since(base(copied), project, agent);
        let paths: Vec<String> = offered.iter().map(command_path).collect();
        assert!(paths.iter().all(|p| p == "b.rs" || p == "c.rs"), "{offered:?}");
        assert!(paths.contains(&"b.rs".to_string()) && paths.contains(&"c.rs".to_string()), "{offered:?}");
    }

    /// A file the agent deleted is offered; without a base, nothing is.
    ///
    /// @tests REQ-OBS.only_what_the_tool_changed
    #[test]
    fn a_deletion_is_the_agents_and_an_unknown_start_offers_nothing() {
        let project = ws(&[("a.rs", "one"), ("b.rs", "two")]);
        let agent = ws(&[("a.rs", "one")]);
        let offered = changed_since(base(&[("a.rs", "one"), ("b.rs", "two")]), project.clone(), agent.clone());
        assert_eq!(offered, vec![Command::DeleteFile { path: "b.rs".into(), content: "two".into() }]);
        assert!(changed_since(None, project, agent).is_empty());
    }

    /// The law.
    ///
    /// @tests REQ-MIRROR.apply_reproduces
    #[test]
    fn applying_the_mutations_reproduces_what_was_observed() {
        let before = ws(&[("a.rs", "one"), ("b.rs", "two"), ("gone.rs", "x")]);
        let after = ws(&[("a.rs", "ONE"), ("b.rs", "two"), ("new.rs", "three")]);
        let result = mirrored(before, after.clone());
        assert_eq!(result.reached, after);
        assert!(result.refused.is_empty());
    }

    /// On mirrored paths only: a protected or regenerated path keeps what the
    /// original tree had.
    ///
    /// @tests REQ-MIRROR.apply_reproduces
    #[test]
    fn an_unmirrored_path_keeps_the_original() {
        let before = ws(&[(".git/x", "a"), ("target/o", "1"), ("a.rs", "one")]);
        let after = ws(&[(".git/x", "b"), ("target/o", "2"), ("a.rs", "two")]);
        let result = mirrored(before, after);
        assert_eq!(result.reached, ws(&[(".git/x", "a"), ("target/o", "1"), ("a.rs", "two")]));
    }

    /// A binary whose bytes changed is reported; one whose bytes did not is
    /// not. Before the opaque state carried a hash, both were silent.
    ///
    /// @tests REQ-MIRROR.binary_handled
    #[test]
    fn a_changed_binary_is_reported_and_an_unchanged_one_is_not() {
        let state = |hash: &str| FileState::Opaque { hash: hash.into() };
        assert_eq!(
            change_at("logo.png".into(), state("1"), state("2")),
            vec![Change::ReportOnly { path: "logo.png".into() }]
        );
        assert!(change_at("logo.png".into(), state("1"), state("1")).is_empty());
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
        assert_eq!(mirrored(ws(&[]), ws(&[("new.rs", "content")])).reached.get("new.rs").unwrap(), "content");
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
                age: 1,
            })
            .collect()
    }

    /// A write with rounds to spare still suppresses one observation, not one
    /// per round: with a single counter for both, it suppressed three.
    ///
    /// @tests REQ-SELFWRITE.suppression_is_consumed
    #[test]
    fn a_write_with_time_left_is_still_consumed_by_one_observation() {
        let waiting = vec![SelfWrite { path: "a.rs".into(), content: "x".into(), age: 3 }];
        let first = suppress(waiting, "a.rs".into(), "x".into());
        assert!(first.suppressed);
        assert!(first.pending.is_empty(), "the write outlived its one observation");
        assert!(!suppress(first.pending, "a.rs".into(), "x".into()).suppressed);
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
