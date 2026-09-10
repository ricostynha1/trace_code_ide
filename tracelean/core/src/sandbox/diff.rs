//! Two-way filesystem diff between a session's working copy and the real
//! project tree — the session-copy analogue of
//! `ai::shell_sandbox::collect_upper_mutations`, which diffs an overlay
//! upper dir instead. No whiteout/opaque-xattr tricks are needed here: a
//! plain copy has no overlay semantics, so "deleted" is simply "present in
//! `real`, absent in `work`".

use crate::ai::shell_sandbox::{self, FsMutation, MutationKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Diff `work` (the session's writable copy) against `real` (the project the
/// session was copied from), skipping the allowlisted build dirs and
/// protected paths that are bind-mounted through rather than copied (they
/// never appear in the diff either way).
pub fn collect_tree_mutations(work: &Path, real: &Path) -> Vec<FsMutation> {
    let mut out = Vec::new();
    walk(work, real, PathBuf::new(), &mut out);
    out
}

fn walk(work_dir: &Path, real_dir: &Path, rel: PathBuf, out: &mut Vec<FsMutation>) {
    let mut names: HashSet<std::ffi::OsString> = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        names.extend(entries.flatten().map(|e| e.file_name()));
    }
    if let Ok(entries) = std::fs::read_dir(real_dir) {
        names.extend(entries.flatten().map(|e| e.file_name()));
    }

    for name in names {
        let child_rel = rel.join(&name);
        if shell_sandbox::is_protected(&child_rel) || shell_sandbox::is_allowlisted(&child_rel) {
            continue;
        }
        let work_path = work_dir.join(&name);
        let real_path = real_dir.join(&name);
        let work_meta = std::fs::symlink_metadata(&work_path).ok();
        let real_meta = std::fs::symlink_metadata(&real_path).ok();

        match (work_meta, real_meta) {
            (Some(wm), _) if wm.is_dir() => {
                walk(&work_path, &real_path, child_rel, out);
            }
            (Some(_), Some(rm)) if rm.is_dir() => {
                // `real` had a directory; `work` replaced it with a file.
                // Record the whole real subtree as deleted, then the new
                // file as created.
                record_tree_deleted(&real_path, &child_rel, out);
                if let Some(post) = shell_sandbox::read_capture(&work_path).ok().flatten() {
                    out.push(FsMutation {
                        path: child_rel,
                        kind: MutationKind::Created,
                        pre: None,
                        post: Some(post),
                    });
                }
            }
            (Some(_), _) => {
                // Plain file in `work`; compare against `real` (file or
                // absent). Binary/too-large files are skipped — best-effort
                // mirror, not a full audit trail (unlike the overlay path,
                // there is no `skipped` list here yet).
                let Some(post) = shell_sandbox::read_capture(&work_path).ok().flatten() else {
                    continue;
                };
                let pre = shell_sandbox::read_capture(&real_path).ok().flatten();
                if pre.as_deref() != Some(post.as_str()) {
                    let kind = if pre.is_some() { MutationKind::Modified } else { MutationKind::Created };
                    out.push(FsMutation { path: child_rel, kind, pre, post: Some(post) });
                }
            }
            (None, Some(rm)) if rm.is_dir() => {
                record_tree_deleted(&real_path, &child_rel, out);
            }
            (None, Some(_)) => {
                if let Some(pre) = shell_sandbox::read_capture(&real_path).ok().flatten() {
                    out.push(FsMutation { path: child_rel, kind: MutationKind::Deleted, pre: Some(pre), post: None });
                }
            }
            (None, None) => {}
        }
    }
}

fn record_tree_deleted(real_dir: &Path, rel: &Path, out: &mut Vec<FsMutation>) {
    let Ok(entries) = std::fs::read_dir(real_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let child_rel = rel.join(&name);
        if shell_sandbox::is_protected(&child_rel) || shell_sandbox::is_allowlisted(&child_rel) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            record_tree_deleted(&path, &child_rel, out);
        } else if let Some(pre) = shell_sandbox::read_capture(&path).ok().flatten() {
            out.push(FsMutation { path: child_rel, kind: MutationKind::Deleted, pre: Some(pre), post: None });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        let work = root.path().join("work");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(&work).unwrap();
        (root, work, real)
    }

    #[test]
    fn detects_created_file() {
        let (_root, work, real) = setup();
        fs::write(work.join("a.txt"), "hello").unwrap();
        let muts = collect_tree_mutations(&work, &real);
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].kind, MutationKind::Created);
        assert_eq!(muts[0].path, PathBuf::from("a.txt"));
        assert_eq!(muts[0].post.as_deref(), Some("hello"));
        assert_eq!(muts[0].pre, None);
    }

    #[test]
    fn detects_modified_file() {
        let (_root, work, real) = setup();
        fs::write(real.join("a.txt"), "old").unwrap();
        fs::write(work.join("a.txt"), "new").unwrap();
        let muts = collect_tree_mutations(&work, &real);
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].kind, MutationKind::Modified);
        assert_eq!(muts[0].pre.as_deref(), Some("old"));
        assert_eq!(muts[0].post.as_deref(), Some("new"));
    }

    #[test]
    fn detects_deleted_file() {
        let (_root, work, real) = setup();
        fs::write(real.join("a.txt"), "gone").unwrap();
        let muts = collect_tree_mutations(&work, &real);
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].kind, MutationKind::Deleted);
        assert_eq!(muts[0].pre.as_deref(), Some("gone"));
        assert_eq!(muts[0].post, None);
    }

    #[test]
    fn unchanged_file_produces_no_mutation() {
        let (_root, work, real) = setup();
        fs::write(real.join("a.txt"), "same").unwrap();
        fs::write(work.join("a.txt"), "same").unwrap();
        assert!(collect_tree_mutations(&work, &real).is_empty());
    }

    #[test]
    fn nested_dir_create_and_delete() {
        let (_root, work, real) = setup();
        fs::create_dir_all(real.join("keep")).unwrap();
        fs::write(real.join("keep/old.txt"), "x").unwrap();
        fs::create_dir_all(work.join("keep")).unwrap();
        fs::create_dir_all(work.join("newdir")).unwrap();
        fs::write(work.join("newdir/n.txt"), "n").unwrap();
        let mut muts = collect_tree_mutations(&work, &real);
        muts.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<_> = muts.iter().map(|m| (m.path.clone(), m.kind)).collect();
        assert_eq!(
            paths,
            vec![
                (PathBuf::from("keep/old.txt"), MutationKind::Deleted),
                (PathBuf::from("newdir/n.txt"), MutationKind::Created),
            ]
        );
    }

    #[test]
    fn allowlisted_and_protected_paths_are_skipped() {
        let (_root, work, real) = setup();
        fs::create_dir_all(work.join("target")).unwrap();
        fs::write(work.join("target/out.bin"), "x").unwrap();
        fs::create_dir_all(work.join(".git")).unwrap();
        fs::write(work.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        assert!(collect_tree_mutations(&work, &real).is_empty());
    }
}
