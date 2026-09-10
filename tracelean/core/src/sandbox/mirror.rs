//! Turns observed session-workspace mutations into the same invertible
//! `Command`s an edit tool would produce, and mirrors IDE-side saves back
//! into a session's work dir. Two directions, one shared echo-suppression
//! table (`SelfWrites`) so mirroring a write in one direction doesn't get
//! immediately re-observed and re-applied from the other.

use crate::ai::shell_sandbox::{FsMutation, MutationKind};
use crate::commands::Command;
use crate::state::AppState;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Content hashes of writes tracelean itself made into a session's work
/// dir (IDE saves mirrored out) or into the real tree (session mutations
/// mirrored in). The watcher / save path on the *other* side checks this
/// before treating a change as external, so echoes are dropped rather than
/// bouncing back and forth.
#[derive(Debug, Clone, Default)]
pub struct SelfWrites(Arc<Mutex<HashMap<PathBuf, u64>>>);

impl SelfWrites {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `content` was just written to `rel` by tracelean.
    pub fn mark(&self, rel: PathBuf, content: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(rel, hash_str(content));
        }
    }

    /// True (and clears the entry) if `content` matches the last self-write
    /// registered for `rel` — an echo, not an externally made change.
    pub fn is_echo(&self, rel: &Path, content: &str) -> bool {
        if let Ok(mut m) = self.0.lock() {
            if m.get(rel) == Some(&hash_str(content)) {
                m.remove(rel);
                return true;
            }
        }
        false
    }
}

/// Links a live `AppState` to an active session: the work dir IDE saves
/// should mirror into, and the echo table shared with its watcher.
#[derive(Debug, Clone)]
pub struct SandboxLink {
    pub work_dir: PathBuf,
    pub self_writes: SelfWrites,
}

/// Apply a batch of work-dir mutations to `state` and write the result to
/// the real tree (`project_root`). One `Command::Batch` per call, so a
/// quiescent tick of many writes becomes one undo-tree node, not one per
/// file. Mirrors `ai::tool_executor::materialize_sandbox_run`'s non-review
/// branch; sessions never route through `ReviewSink` — landing directly in
/// the buffer, file tree and undo tree is the point.
pub fn apply_mutations(
    mutations: &[FsMutation],
    project_root: &Path,
    state: &mut AppState,
) -> Result<Vec<PathBuf>, String> {
    if mutations.is_empty() {
        return Ok(Vec::new());
    }

    let mut batch: Vec<Command> = Vec::new();
    let mut to_save: Vec<PathBuf> = Vec::new();
    let mut to_remove: Vec<PathBuf> = Vec::new();
    let mut touched: Vec<PathBuf> = Vec::new();

    for m in mutations {
        touched.push(m.path.clone());
        match m.kind {
            MutationKind::Created | MutationKind::Modified => {
                let post = m.post.clone().unwrap_or_default();
                let old = if let Some(cur) = state.get_content(&m.path) {
                    cur.to_string()
                } else if let Some(pre) = &m.pre {
                    state.load_file(m.path.clone(), pre.clone());
                    pre.clone()
                } else {
                    batch.push(Command::CreateFile { path: m.path.clone() });
                    String::new()
                };
                if old != post {
                    batch.push(Command::Replace { file: m.path.clone(), at: 0, old, new: post });
                }
                to_save.push(m.path.clone());
            }
            MutationKind::Deleted => {
                let old = state
                    .get_content(&m.path)
                    .map(|c| c.to_string())
                    .or_else(|| m.pre.clone())
                    .unwrap_or_default();
                batch.push(Command::DeleteFile { path: m.path.clone(), content: old });
                to_remove.push(m.path.clone());
            }
        }
    }

    if !batch.is_empty() {
        let cmd = if batch.len() == 1 { batch.remove(0) } else { Command::Batch { commands: batch } };
        state.apply(cmd)?;
    }

    for rel in &to_save {
        if let Some(content) = state.get_content(rel) {
            let full = project_root.join(rel);
            if let Some(parent) = full.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&full, content).map_err(|e| format!("saving {}: {}", rel.display(), e))?;
        }
    }
    for rel in &to_remove {
        let _ = std::fs::remove_file(project_root.join(rel));
    }

    Ok(touched)
}
