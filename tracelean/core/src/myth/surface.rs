//! Surfaces (Myth phase 5): a UI surface as a structured document.
//!
//! A surface is content + a parser producing nodes with captures + the
//! binding map attaching actions to captures. Tree-sitter is one parser
//! choice; regular surfaces (file tree, menus) use a cheap line parser that
//! implements the same node/capture interface. Frontends render the node
//! tree however they like (rich rows in the GUI, styled text in the TUI) —
//! behavior and structure come from the surface either way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One parsed node of a surface.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceNode {
    /// Capture name ("dir" / "file" for the tree; highlight captures for code).
    pub capture: String,
    /// 0-indexed line in the surface content.
    pub line: u32,
    /// Char range within the content.
    pub from: usize,
    pub to: usize,
    /// Node text (without indentation).
    pub text: String,
    /// Surface-specific metadata (file tree: {"path": …, "depth": …}).
    pub meta: serde_json::Value,
}

/// A surface snapshot shipped to frontends: content + nodes + the actions
/// each capture carries.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceView {
    pub id: String,
    pub content: String,
    pub nodes: Vec<SurfaceNode>,
    /// capture name → actions (from the binding map).
    pub bindings: HashMap<String, Vec<String>>,
}

/// The file tree as a surface: the workspace rendered as indented text,
/// line-parsed into `@dir` / `@file` nodes carrying the target path.
pub struct FileTreeSurface;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    ".venv",
    "__pycache__",
];

impl FileTreeSurface {
    /// Build the surface for a project root.
    pub fn build(root: &Path) -> SurfaceView {
        let mut lines: Vec<String> = Vec::new();
        let mut nodes: Vec<SurfaceNode> = Vec::new();
        let mut char_offset = 0usize;
        Self::walk(root, root, 0, &mut lines, &mut nodes, &mut char_offset);

        let mut bindings = HashMap::new();
        for capture in ["dir", "file"] {
            let actions = super::bindings::actions_for_capture(capture);
            if !actions.is_empty() {
                bindings.insert(capture.to_string(), actions);
            }
        }

        SurfaceView {
            id: "file_tree".into(),
            content: lines.join("\n"),
            nodes,
            bindings,
        }
    }

    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        lines: &mut Vec<String>,
        nodes: &mut Vec<SurfaceNode>,
        char_offset: &mut usize,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<(bool, String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && name != ".tracelean" {
                    return None;
                }
                let is_dir = e.file_type().ok()?.is_dir();
                if is_dir && SKIP_DIRS.contains(&name.as_str()) {
                    return None;
                }
                Some((is_dir, name, e.path()))
            })
            .collect();
        // Directories first, then files, both alphabetical.
        items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        for (is_dir, name, path) in items {
            let indent = "  ".repeat(depth);
            let display = if is_dir { format!("{}/", name) } else { name.clone() };
            let line_text = format!("{}{}", indent, display);
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();

            let from = *char_offset + indent.chars().count();
            let to = *char_offset + line_text.chars().count();
            nodes.push(SurfaceNode {
                capture: if is_dir { "dir".into() } else { "file".into() },
                line: lines.len() as u32,
                from,
                to,
                text: display,
                meta: serde_json::json!({
                    "path": rel.to_string_lossy(),
                    "depth": depth,
                }),
            });

            // +1 for the joining newline
            *char_offset += line_text.chars().count() + 1;
            lines.push(line_text);

            if is_dir {
                Self::walk(root, &path, depth + 1, lines, nodes, char_offset);
            }
        }
    }
}
