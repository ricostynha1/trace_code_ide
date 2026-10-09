//! The explorer: a project's files as a tree of folders that open and close.
//!
//! A flat list of every path stops being readable at a few dozen files, so the
//! listing a person browses is a tree. Its rows are names, not paths — and a
//! name is not enough to open a file — so the producer also answers which path
//! a row stands for (`path_at`). The shell resolves a click by row through it,
//! as it resolves a tab by row; nothing works out a path from the text.
//!
//! Every row carries `file.open`. On a folder, opening it opens or closes it:
//! that is the shell's to do, since which folders are open is the session's
//! state, not the workspace's.
//!
//! @implements REQ-SHOW.listing_is_a_tree

use std::collections::{BTreeMap, BTreeSet};

use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// How many folders it is inside.
    pub depth: usize,
    /// The last segment of its path: what the row shows.
    pub name: String,
    /// The whole path, relative to the project: what the row is about.
    pub path: String,
    /// Whether it is a folder rather than a file.
    pub folder: bool,
    /// Whether, being a folder, it is open.
    pub open: bool,
}

/// The rows a set of files makes, with `open` folders showing what they hold.
///
/// Folders come before files at every level, each group in name order, and
/// what is inside a closed folder is not shown.
pub fn rows(files: &[String], open: &BTreeSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    level(files, "", 0, open, &mut out);
    out
}

fn level(files: &[String], prefix: &str, depth: usize, open: &BTreeSet<String>, out: &mut Vec<Row>) {
    let mut folders: BTreeSet<String> = BTreeSet::new();
    let mut leaves: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let Some(rest) = file.strip_prefix(prefix) else { continue };
        match rest.split_once('/') {
            Some((folder, _)) => folders.insert(folder.to_string()),
            None => leaves.insert(rest.to_string()),
        };
    }
    for folder in folders {
        let path = format!("{prefix}{folder}");
        let is_open = open.contains(&path);
        out.push(Row { depth, name: folder, path: path.clone(), folder: true, open: is_open });
        if is_open {
            level(files, &format!("{path}/"), depth + 1, open, out);
        }
    }
    for leaf in leaves {
        out.push(Row { depth, name: leaf.clone(), path: format!("{prefix}{leaf}"), folder: false, open: false });
    }
}

/// What comes before a row's name: its indentation, and an arrow on a folder.
fn lead(row: &Row) -> String {
    let marker = match (row.folder, row.open) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        (false, _) => "  ",
    };
    format!("{}{marker}", "  ".repeat(row.depth))
}

/// The mark a row carries when an agent's change waits on it (`●`, `+`, `✗`)
/// or it has edits not yet saved (`✎`, kind `unsaved`): on the file, and on
/// every folder above one, so a closed folder still says so.
fn badge(row: &Row, changed: &BTreeMap<String, String>) -> Option<(&'static str, Role)> {
    if row.folder {
        let inside = format!("{}/", row.path);
        let under: Vec<&str> =
            changed.iter().filter(|(p, _)| p.starts_with(&inside)).map(|(_, k)| k.as_str()).collect();
        return match under.as_slice() {
            [] => None,
            kinds if kinds.iter().all(|k| *k == "unsaved") => Some(("✎", Role::Entry)),
            _ => Some(("●", Role::Requirement)),
        };
    }
    match changed.get(&row.path).map(String::as_str) {
        Some("deleted") => Some(("✗", Role::Removed)),
        Some("created") => Some(("+", Role::Added)),
        Some("unsaved") => Some(("✎", Role::Entry)),
        Some(_) => Some(("●", Role::Requirement)),
        None => None,
    }
}

/// The tree as a buffer: one row a line, the name marked as a path that opens,
/// and a mark after it where an agent's change is waiting (`changed` maps a
/// path to `created`, `modified` or `deleted`). The file the document shows,
/// `current`, is a heading rather than a path, so it stands out.
///
/// A file that claims requirements carries a letter for each kind of claim
/// in it (`claimed`: I implements, T tests, M models, P proves, D drt), in
/// its chip's colour — where requirements are met is visible from the tree.
pub fn tree_buffer(
    title: String,
    rows: &[Row],
    changed: &BTreeMap<String, String>,
    claimed: &BTreeMap<String, BTreeSet<crate::trace::annotation::Role>>,
    current: Option<&str>,
) -> Buffer {
    // The first row names the project, so which tree this is stays in view.
    let name = title.trim_end_matches('/').rsplit('/').next().unwrap_or(&title).to_uppercase();
    let mut spans = vec![Span { start: 0, stop: name.chars().count(), role: Role::Heading, actions: Vec::new() }];
    let mut at = name.chars().count() + 1;
    let mut lines = vec![name];
    for row in rows {
        let lead = lead(row);
        let mut line = format!("{lead}{}", row.name);
        let length = line.chars().count();
        spans.push(Span {
            start: at + lead.chars().count(),
            stop: at + length,
            role: if current == Some(row.path.as_str()) { Role::Heading } else { Role::Path },
            actions: vec!["file.open".to_string()],
        });
        let mut end = length;
        if let Some(kinds) = claimed.get(&row.path).filter(|_| !row.folder) {
            line.push(' ');
            end += 1;
            // A model and a specification share a letter; it is shown once.
            let mut marked: Vec<char> = Vec::new();
            for kind in kinds {
                let Some(letter) = crate::surface::chips::letter(*kind) else { continue };
                if marked.contains(&letter) {
                    continue;
                }
                marked.push(letter);
                line.push(' ');
                line.push(letter);
                spans.push(Span { start: at + end + 1, stop: at + end + 2, role: Role::Claim { role: *kind }, actions: Vec::new() });
                end += 2;
            }
        }
        if let Some((mark, role)) = badge(row, changed) {
            line.push_str("  ");
            line.push_str(mark);
            spans.push(Span { start: at + end + 2, stop: at + end + 3, role, actions: Vec::new() });
            end += 3;
        }
        at += end + 1;
        lines.push(line);
    }
    // What the marks mean, under the tree: only those it shows, each in its
    // own colour.
    let mut legend: Vec<(String, Role, &str)> = Vec::new();
    let shown: BTreeSet<crate::trace::annotation::Role> =
        rows.iter().filter(|r| !r.folder).filter_map(|r| claimed.get(&r.path)).flatten().copied().collect();
    for kind in shown {
        if let Some(letter) = crate::surface::chips::letter(kind) {
            if legend.iter().any(|(said, _, _)| said.starts_with(letter)) {
                continue;
            }
            legend.push((letter.to_string(), Role::Claim { role: kind }, kind.as_str()));
        }
    }
    let marks: BTreeSet<&str> = changed.values().map(String::as_str).collect();
    for (kind, mark, role, said) in [
        ("modified", "●", Role::Requirement, "changed by the agent"),
        ("created", "+", Role::Added, "made by the agent"),
        ("deleted", "✗", Role::Removed, "deleted by the agent"),
        ("unsaved", "✎", Role::Entry, "not saved"),
    ] {
        if marks.contains(kind) {
            legend.push((mark.to_string(), role, said));
        }
    }
    if !legend.is_empty() {
        lines.push(String::new());
        at += 1;
        for (mark, role, said) in legend {
            let line = format!("{mark} {said}");
            spans.push(Span { start: at, stop: at + mark.chars().count(), role, actions: Vec::new() });
            at += line.chars().count() + 1;
            lines.push(line);
        }
    }
    Buffer {
        id: format!("dir:{title}"),
        kind: BufferKind::Directory { path: title },
        text: lines.join("\n"),
        spans,
    }
}

/// The row a position in the tree's text is on.
pub fn path_at<'a>(rows: &'a [Row], text: &str, offset: usize) -> Option<&'a Row> {
    // The first line is the project's name, not a row.
    let line = text.chars().take(offset).filter(|c| *c == '\n').count();
    line.checked_sub(1).and_then(|row| rows.get(row))
}

/// The folders that hold a path, so that revealing a file opens them.
pub fn holders(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len()).map(|n| parts[..n].join("/")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::faults;

    fn files() -> Vec<String> {
        ["README.md", "src/main.rs", "src/notes/a.md", "tests/t.rs"].iter().map(|s| s.to_string()).collect()
    }

    /// Closed, a folder is one row; open, it shows what it holds, indented,
    /// and every row still knows its whole path.
    ///
    /// @tests REQ-SHOW.listing_is_a_tree
    #[test]
    fn folders_open_and_close_and_rows_know_their_paths() {
        let closed = rows(&files(), &BTreeSet::new());
        let names: Vec<&str> = closed.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["src", "tests", "README.md"]);

        let open: BTreeSet<String> = ["src".to_string(), "src/notes".to_string()].into();
        let opened = rows(&files(), &open);
        let buffer = tree_buffer("demo".into(), &opened, &BTreeMap::new(), &BTreeMap::new(), Some("src/main.rs"));
        let main = buffer.text.find("main.rs").map(|b| buffer.text[..b].chars().count()).unwrap();
        assert!(buffer.spans.iter().any(|s| s.start == main && s.role == Role::Heading), "the shown file is not marked");
        assert_eq!(buffer.text, "DEMO\n▾ src\n  ▾ notes\n      a.md\n    main.rs\n▸ tests\n  README.md");
        assert!(faults(buffer.clone()).is_empty());
        let at = buffer.text.find("a.md").map(|b| buffer.text[..b].chars().count()).unwrap();
        assert_eq!(path_at(&opened, &buffer.text, at).map(|r| r.path.as_str()), Some("src/notes/a.md"));
    }

    /// A file an agent changed is marked, and so is every folder above it.
    #[test]
    fn waiting_changes_are_marked_on_the_file_and_its_folders() {
        let changed: BTreeMap<String, String> = [("src/notes/a.md".to_string(), "modified".to_string())].into();
        let buffer = tree_buffer("demo".into(), &rows(&files(), &BTreeSet::new()), &changed, &BTreeMap::new(), None);
        assert_eq!(buffer.text, "DEMO\n▸ src  ●\n▸ tests\n  README.md\n\n● changed by the agent");
        assert!(faults(buffer.clone()).is_empty());
        let open: BTreeSet<String> = ["src".to_string(), "src/notes".to_string()].into();
        let buffer = tree_buffer("demo".into(), &rows(&files(), &open), &changed, &BTreeMap::new(), None);
        assert!(buffer.text.contains("a.md  ●"), "{}", buffer.text);
        let rows = rows(&files(), &open);
        let at = buffer.text.find("main.rs").map(|b| buffer.text[..b].chars().count()).unwrap();
        assert_eq!(path_at(&rows, &buffer.text, at).map(|r| r.path.as_str()), Some("src/main.rs"));
    }

    /// An unsaved file is marked apart from an agent's change, and a folder
    /// holding both says the agent's.
    #[test]
    fn unsaved_edits_are_marked_apart() {
        let open: BTreeSet<String> = ["src".to_string()].into();
        let unsaved: BTreeMap<String, String> = [("src/main.rs".to_string(), "unsaved".to_string())].into();
        let buffer = tree_buffer("demo".into(), &rows(&files(), &open), &unsaved, &BTreeMap::new(), None);
        assert!(buffer.text.contains("▾ src  ✎") && buffer.text.contains("main.rs  ✎"), "{}", buffer.text);
        let mut both = unsaved.clone();
        both.insert("src/notes/a.md".into(), "modified".into());
        let buffer = tree_buffer("demo".into(), &rows(&files(), &open), &both, &BTreeMap::new(), None);
        assert!(buffer.text.contains("▾ src  ●"), "{}", buffer.text);
    }

    /// A file that claims requirements says which kinds of claim, each in its
    /// role, so it is coloured as its chips are.
    #[test]
    fn a_claiming_file_carries_its_claim_letters() {
        use crate::trace::annotation::Role as Claimed;
        let open: BTreeSet<String> = ["src".to_string()].into();
        let claimed: BTreeMap<String, BTreeSet<Claimed>> =
            [("src/main.rs".to_string(), [Claimed::Implements, Claimed::Tests].into())].into();
        let buffer = tree_buffer("demo".into(), &rows(&files(), &open), &BTreeMap::new(), &claimed, None);
        assert!(buffer.text.contains("main.rs  I T"), "{}", buffer.text);
        assert!(faults(buffer.clone()).is_empty());
        let at = buffer.text.find("I T").map(|b| buffer.text[..b].chars().count()).unwrap();
        assert!(buffer.spans.iter().any(|s| s.start == at && s.role == Role::Claim { role: Claimed::Implements }));
        // Under the tree, what the letters it shows mean — and only those.
        assert!(buffer.text.ends_with("\n\nI implements\nT tests"), "{}", buffer.text);
        let key = buffer.text.rfind("\nT tests").map(|b| buffer.text[..b + 1].chars().count()).unwrap();
        assert!(buffer.spans.iter().any(|s| s.start == key && s.role == Role::Claim { role: Claimed::Tests }));
        // A row past the tree is not a file.
        let rows = rows(&files(), &open);
        assert!(path_at(&rows, &buffer.text, buffer.text.chars().count() - 1).is_none());
    }

    #[test]
    fn a_file_is_held_by_every_folder_above_it() {
        assert_eq!(holders("src/notes/a.md"), vec!["src".to_string(), "src/notes".to_string()]);
        assert!(holders("README.md").is_empty());
    }
}
