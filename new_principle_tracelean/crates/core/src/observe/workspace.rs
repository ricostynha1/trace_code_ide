//! The thin effectful shell: read a tree, write a copy, ask the host what it
//! can contain a tool with.
//!
//! Every decision this module could make has been taken out of it. Which paths
//! are mirrored is `policy`; whether a run obeyed its laws is `effects`. What
//! is left is filesystem calls and one `PATH` lookup, which is the most that
//! can be said for a module that cannot be differentially tested.
//!
//! Nothing here launches anything. `probe_containment` looks for an executable
//! and does not run it — the user runs the tool, in a shell they control
//! (`ARCH-NO-DRIVING`).
//!
//! @implements ARCH-CORE-SHELL.shell_thin
//! @implements ARCH-NO-DRIVING.no_launch

use std::path::{Path, PathBuf};

use crate::history::command::Workspace;

use super::effects::{CopyWitness, RunWitness};
use super::mirror::FileState;
use super::policy::{is_mirrored, Capability};

/// The containment mechanisms this project knows how to name, best first.
///
/// A fixed list rather than a probe of everything on the host: an unrecognised
/// mechanism is reported as unavailable, which is the safe direction to be
/// wrong in.
const MECHANISMS: &[(&str, &str)] = &[
    ("bwrap", "bubblewrap"),
    ("sandbox-exec", "seatbelt"),
    ("systemd-run", "systemd transient scope"),
];

/// Every mirrored file under `root`, as a workspace.
///
/// Unreadable files are skipped rather than failing the snapshot: a socket or a
/// dangling symlink in a project is not a reason to refuse to observe it, and
/// the file's absence from both snapshots makes it invisible rather than wrong.
///
/// @implements REQ-OBS.workspace_is_a_copy
pub fn snapshot(root: &Path) -> Workspace {
    survey(root).0
}

/// Every mirrored file under `root`, and the paths that exist and are not text.
///
/// The second list is what `snapshot` used to drop on the floor. A file the
/// editor cannot represent is a change the user still needs to hear about, and
/// silently omitting it means a tool can alter a binary and nothing says so.
///
/// @implements REQ-MIRROR.binary_handled
/// @implements REQ-OBS.workspace_is_a_copy
pub fn survey(root: &Path) -> (Workspace, Vec<String>) {
    let mut out = Workspace::new();
    let mut stack = vec![root.to_path_buf()];
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let Some(rel) = rel.to_str() else { continue };
            if path.is_dir() {
                // A directory whose whole subtree is excluded is not walked,
                // which is what keeps a snapshot of a built project cheap.
                if is_mirrored(rel) {
                    stack.push(path);
                }
            } else if is_mirrored(rel) {
                files.push(path);
            }
        }
    }
    // Sorted so that a snapshot is a function of the tree rather than of the
    // order the filesystem happened to return entries in.
    files.sort();
    let mut opaque = Vec::new();
    for path in files {
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let Some(rel) = rel.to_str() else { continue };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                out.files.insert(rel.to_string(), content);
            }
            // Present and unreadable as text. Recorded rather than skipped:
            // this is the case `REQ-MIRROR.binary_handled` exists for, and a
            // snapshot that omitted it would make a change to a binary
            // invisible rather than unmirrorable.
            Err(_) => opaque.push(rel.to_string()),
        }
    }
    (out, opaque)
}

/// What one path looked like in a survey.
///
/// @implements REQ-MIRROR.binary_handled
pub fn state_of(survey: &(Workspace, Vec<String>), path: &str) -> FileState {
    match survey.0.files.get(path) {
        Some(content) => FileState::Text { content: content.clone() },
        None if survey.1.iter().any(|p| p == path) => FileState::Opaque,
        None => FileState::Absent,
    }
}

/// Write a workspace into `dest`, creating directories as needed.
///
/// @implements REQ-OBS.workspace_is_a_copy
pub fn write_into(workspace: &Workspace, dest: &Path) -> std::io::Result<()> {
    for (rel, content) in &workspace.files {
        let target = dest.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, content)?;
    }
    Ok(())
}

/// What the host can contain a tool with, as a named state.
///
/// The executable is looked for and not run. An unavailable mechanism is
/// reported with a reason, because a sandbox that silently is not one is the
/// failure this clause exists to prevent.
///
/// @implements REQ-SBX.capability_reported
pub fn probe_containment() -> Capability {
    let Some(path) = std::env::var_os("PATH") else {
        return Capability::Unavailable { reason: "no PATH to search".to_string() };
    };
    for (binary, mechanism) in MECHANISMS {
        for dir in std::env::split_paths(&path) {
            if dir.join(binary).is_file() {
                return Capability::Contained { mechanism: (*mechanism).to_string() };
            }
        }
    }
    Capability::Unavailable {
        reason: format!(
            "none of {} is on PATH",
            MECHANISMS.iter().map(|(b, _)| *b).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Take the three snapshots a copy law is judged on.
///
/// The caller supplies the paths the run was observed writing to; this module
/// has no way to know them, and inventing them would make the escape law a
/// claim about nothing.
///
/// @implements ARCH-EFFECT-LAW.law_checked
pub fn witness(root: &Path, workspace_root: &Path, writes: Vec<String>) -> RunWitness {
    RunWitness {
        copy: CopyWitness {
            before: snapshot(root),
            workspace: snapshot(workspace_root),
            after: snapshot(root),
        },
        writes,
        containment: probe_containment(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The law, checked against the real thing: copy a real tree into a real
    /// directory and ask whether the laws hold of what happened.
    ///
    /// This is what `ARCH-EFFECT-LAW.law_checked` asks for. The model states
    /// the law; the differential test checks both sides compute it the same
    /// way; this checks it of an actual filesystem.
    ///
    /// @tests ARCH-EFFECT-LAW.law_checked
    /// @tests REQ-OBS.workspace_is_a_copy
    /// @tests REQ-SBX.real_tree_untouched
    #[test]
    fn a_real_copy_of_a_real_tree_obeys_the_law() {
        use crate::observe::effects::run_violations;

        let base = std::env::temp_dir().join(format!("tracelean-copy-{}", std::process::id()));
        let root = base.join("project");
        let sandbox = base.join("sandbox");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/a.rs").as_path(), "fn a() {}").unwrap();
        std::fs::write(root.join("README.md").as_path(), "# p").unwrap();
        std::fs::write(root.join(".git/HEAD").as_path(), "ref: main").unwrap();
        std::fs::write(root.join("target/out").as_path(), "binary").unwrap();

        let before = snapshot(&root);
        // The protected and regenerable files are not in the snapshot at all,
        // which is why they are neither copied nor compared.
        assert_eq!(before.files.len(), 2);
        write_into(&before, &sandbox).unwrap();

        let observed = witness(&root, &sandbox, vec![]);
        assert_eq!(
            run_violations(observed),
            vec![],
            "a faithful copy of a real tree broke a law"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// And it fails when it should: a copy that dropped a file is caught.
    ///
    /// @tests ARCH-EFFECT-LAW.law_checked
    #[test]
    fn a_copy_that_dropped_a_file_is_caught() {
        use crate::observe::effects::{run_violations, Violation};

        let base = std::env::temp_dir().join(format!("tracelean-copy-bad-{}", std::process::id()));
        let root = base.join("project");
        let sandbox = base.join("sandbox");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sandbox).unwrap();
        std::fs::write(root.join("a.rs").as_path(), "x").unwrap();
        std::fs::write(root.join("b.rs").as_path(), "y").unwrap();
        std::fs::write(sandbox.join("a.rs").as_path(), "x").unwrap();

        let violations = run_violations(witness(&root, &sandbox, vec!["/etc/passwd".into()]));
        assert!(violations.contains(&Violation::MissingFromCopy { path: "b.rs".into() }));
        assert!(violations.contains(&Violation::Escaped { path: "/etc/passwd".into() }));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A file the editor cannot represent is reported, not dropped.
    ///
    /// The whole point of `binary_handled`: a tool that alters a binary has
    /// altered something, and a snapshot that quietly omits it makes that
    /// invisible rather than merely unmirrorable.
    ///
    /// @tests REQ-MIRROR.binary_handled
    #[test]
    fn a_file_that_is_not_text_is_surveyed_rather_than_skipped() {
        use crate::observe::mirror::{change_at, Change, FileState};

        let base = std::env::temp_dir().join(format!("tracelean-opaque-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.rs"), "fn a() {}").unwrap();
        // Invalid UTF-8: a lone continuation byte is not a character.
        std::fs::write(base.join("logo.png"), [0x89u8, 0x50, 0xff, 0xfe, 0x00]).unwrap();

        let before = survey(&base);
        assert_eq!(before.0.files.len(), 1, "the text file was not surveyed");
        assert_eq!(before.1, vec!["logo.png".to_string()], "the binary was dropped");

        assert_eq!(
            state_of(&before, "logo.png"),
            FileState::Opaque,
            "a binary read as absent, which is how a change to it disappears"
        );
        assert_eq!(state_of(&before, "a.rs"), FileState::Text { content: "fn a() {}".into() });
        assert_eq!(state_of(&before, "nothing.rs"), FileState::Absent);

        // And a change to it is reported without being mirrored.
        let changes = change_at(
            "logo.png".into(),
            FileState::Opaque,
            FileState::Text { content: "text now".into() },
        );
        assert_eq!(changes, vec![Change::ReportOnly { path: "logo.png".into() }]);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Containment is named either way, and never empty.
    ///
    /// @tests REQ-SBX.capability_reported
    #[test]
    fn the_host_is_asked_and_the_answer_is_always_a_name() {
        use crate::observe::effects::capability_violations;
        assert_eq!(capability_violations(probe_containment()), vec![]);
    }
}
