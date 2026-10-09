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
use super::policy::{is_mirrored, Capability, PROTECTED};

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

/// A short digest of a file's bytes (FNV-1a): enough to tell a changed binary
/// from an unchanged one, which is all it is for.
fn bytes_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

/// Every mirrored file under `root`, and the paths that exist and are not
/// text, each with a hash of its bytes.
///
/// The second list is what `snapshot` used to drop on the floor. A file the
/// editor cannot represent is a change the user still needs to hear about, and
/// silently omitting it means a tool can alter a binary and nothing says so;
/// without its hash, two versions of it were indistinguishable.
///
/// @implements REQ-MIRROR.binary_handled
/// @implements REQ-OBS.workspace_is_a_copy
pub fn survey(root: &Path) -> (Workspace, Vec<(String, String)>) {
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
            Err(_) => {
                let hash = std::fs::read(&path).map(|bytes| bytes_hash(&bytes)).unwrap_or_default();
                opaque.push((rel.to_string(), hash));
            }
        }
    }
    (out, opaque)
}

/// What one path looked like in a survey.
///
/// @implements REQ-MIRROR.binary_handled
pub fn state_of(survey: &(Workspace, Vec<(String, String)>), path: &str) -> FileState {
    match survey.0.files.get(path) {
        Some(content) => FileState::Text { content: content.clone() },
        None => match survey.1.iter().find(|(p, _)| p == path) {
            Some((_, hash)) => FileState::Opaque { hash: hash.clone() },
            None => FileState::Absent,
        },
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

/// The protected roots (`.git`, `.tracelean`) present under `dir`.
pub fn protected_roots(dir: &Path) -> Vec<String> {
    PROTECTED.iter().filter(|root| dir.join(root).exists()).map(|root| root.to_string()).collect()
}

/// A tree as the copy law sees it: its mirrored files, and each of `roots` as
/// one entry whose content is not read — whether a tool can see version
/// control is the copy's business, what it does inside is its own.
pub fn view(dir: &Path, roots: &[String]) -> Workspace {
    let mut out = snapshot(dir);
    for root in roots {
        out.files.insert(root.clone(), String::new());
    }
    out
}

/// Take the three snapshots a copy law is judged on.
///
/// The caller supplies the paths the run was observed writing to, and the
/// protected roots the sandbox shows the tool from the real tree
/// (`sandbox::bound`); this module has no way to know either, and inventing
/// them would make the laws claims about nothing.
///
/// @implements ARCH-EFFECT-LAW.law_checked
pub fn witness(root: &Path, workspace_root: &Path, bound: &[String], writes: Vec<String>) -> RunWitness {
    let mut seen = protected_roots(workspace_root);
    // A bind of a root the real tree does not have shows the tool nothing.
    seen.extend(bound.iter().filter(|name| root.join(name).exists()).cloned());
    RunWitness {
        copy: CopyWitness {
            before: view(root, &protected_roots(root)),
            workspace: view(workspace_root, &seen),
            after: view(root, &protected_roots(root)),
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
        // which is why they are not copied as files.
        assert_eq!(before.files.len(), 2);
        write_into(&before, &sandbox).unwrap();

        // The sandbox shows the tool the real `.git`, read-only.
        let bound = vec![".git".to_string()];
        let observed = witness(&root, &sandbox, &bound, vec![]);
        assert_eq!(
            run_violations(observed),
            vec![],
            "a faithful copy of a real tree broke a law"
        );

        // Without that, the tool cannot see the project's version control, and
        // the copy is not one.
        use crate::observe::effects::Violation;
        assert_eq!(
            run_violations(witness(&root, &sandbox, &[], vec![])),
            vec![Violation::MissingFromCopy { path: ".git".into() }]
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

        let violations = run_violations(witness(&root, &sandbox, &[], vec!["/etc/passwd".into()]));
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
        assert_eq!(before.1.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), vec!["logo.png"], "the binary was dropped");

        let was = state_of(&before, "logo.png");
        assert!(
            matches!(was, FileState::Opaque { .. }),
            "a binary read as absent, which is how a change to it disappears"
        );
        assert_eq!(state_of(&before, "a.rs"), FileState::Text { content: "fn a() {}".into() });
        assert_eq!(state_of(&before, "nothing.rs"), FileState::Absent);

        // And a change to it is reported without being mirrored: to text, and
        // to other bytes, which is the change a hashless state could not see.
        let changes = change_at("logo.png".into(), was.clone(), FileState::Text { content: "text now".into() });
        assert_eq!(changes, vec![Change::ReportOnly { path: "logo.png".into() }]);
        std::fs::write(base.join("logo.png"), [0x89u8, 0x50, 0xff, 0xfe, 0x01]).unwrap();
        let now = state_of(&survey(&base), "logo.png");
        assert_eq!(change_at("logo.png".into(), was.clone(), now), vec![Change::ReportOnly { path: "logo.png".into() }]);
        assert!(change_at("logo.png".into(), was.clone(), was).is_empty());

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
