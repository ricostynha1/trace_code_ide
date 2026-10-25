//! @implements ARCH-CORE-SHELL.shell_thin
//! @implements ARCH-NO-DRIVING.no_launch

use std::path::{Path, PathBuf};

use crate::history::command::Workspace;

use super::effects::{CopyWitness, RunWitness};
use super::mirror::FileState;
use super::policy::{is_mirrored, Capability, PROTECTED};

const MECHANISMS: &[(&str, &str)] = &[
    ("bwrap", "bubblewrap"),
    ("sandbox-exec", "seatbelt"),
    ("systemd-run", "systemd transient scope"),
];

/// @implements REQ-OBS.workspace_is_a_copy
pub fn snapshot(root: &Path) -> Workspace {
    survey(root).0
}

fn bytes_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

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
                if is_mirrored(rel) {
                    stack.push(path);
                }
            } else if is_mirrored(rel) {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut opaque = Vec::new();
    for path in files {
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let Some(rel) = rel.to_str() else { continue };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                out.files.insert(rel.to_string(), content);
            }
            Err(_) => {
                let hash = std::fs::read(&path).map(|bytes| bytes_hash(&bytes)).unwrap_or_default();
                opaque.push((rel.to_string(), hash));
            }
        }
    }
    (out, opaque)
}

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

pub fn protected_roots(dir: &Path) -> Vec<String> {
    PROTECTED.iter().filter(|root| dir.join(root).exists()).map(|root| root.to_string()).collect()
}

pub fn view(dir: &Path, roots: &[String]) -> Workspace {
    let mut out = snapshot(dir);
    for root in roots {
        out.files.insert(root.clone(), String::new());
    }
    out
}

/// @implements ARCH-EFFECT-LAW.law_checked
pub fn witness(root: &Path, workspace_root: &Path, bound: &[String], writes: Vec<String>) -> RunWitness {
    let mut seen = protected_roots(workspace_root);
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
        assert_eq!(before.files.len(), 2);
        write_into(&before, &sandbox).unwrap();

        let bound = vec![".git".to_string()];
        let observed = witness(&root, &sandbox, &bound, vec![]);
        assert_eq!(
            run_violations(observed),
            vec![],
            "a faithful copy of a real tree broke a law"
        );

        use crate::observe::effects::Violation;
        assert_eq!(
            run_violations(witness(&root, &sandbox, &[], vec![])),
            vec![Violation::MissingFromCopy { path: ".git".into() }]
        );

        let _ = std::fs::remove_dir_all(&base);
    }

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

    /// @tests REQ-MIRROR.binary_handled
    #[test]
    fn a_file_that_is_not_text_is_surveyed_rather_than_skipped() {
        use crate::observe::mirror::{change_at, Change, FileState};

        let base = std::env::temp_dir().join(format!("tracelean-opaque-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.rs"), "fn a() {}").unwrap();
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

        let changes = change_at("logo.png".into(), was.clone(), FileState::Text { content: "text now".into() });
        assert_eq!(changes, vec![Change::ReportOnly { path: "logo.png".into() }]);
        std::fs::write(base.join("logo.png"), [0x89u8, 0x50, 0xff, 0xfe, 0x01]).unwrap();
        let now = state_of(&survey(&base), "logo.png");
        assert_eq!(change_at("logo.png".into(), was.clone(), now), vec![Change::ReportOnly { path: "logo.png".into() }]);
        assert!(change_at("logo.png".into(), was.clone(), was).is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// @tests REQ-SBX.capability_reported
    #[test]
    fn the_host_is_asked_and_the_answer_is_always_a_name() {
        use crate::observe::effects::capability_violations;
        assert_eq!(capability_violations(probe_containment()), vec![]);
    }
}
