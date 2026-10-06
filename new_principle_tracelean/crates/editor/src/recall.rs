//! What was open last time: the files in the tabs, the folders open in the
//! explorer, and the file in front — so reopening a project picks up where the
//! person left it rather than at the welcome page.
//!
//! A shell, like `theme`: it reads and writes one small JSON file. Only names
//! are kept, never text, so a file changed or removed since is shown as it now
//! is, or left out.

use std::path::Path;

use serde_json::{json, Value};

/// Where a project keeps it.
pub const FILE: &str = ".tracelean/editor.json";

/// The part of an editor worth having back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recalled {
    /// Files in the order their tabs were opened.
    pub files: Vec<String>,
    /// Folders open in the explorer.
    pub folders: Vec<String>,
    /// The file the document pane showed, if it showed one.
    pub shown: Option<String>,
}

/// What was kept, or nothing: a missing or unreadable file is a fresh start,
/// never a reason not to open.
pub fn load(root: &Path) -> Option<Recalled> {
    let text = std::fs::read_to_string(root.join(FILE)).ok()?;
    decode(&serde_json::from_str(&text).ok()?)
}

fn decode(value: &Value) -> Option<Recalled> {
    let names = |key: &str| -> Vec<String> {
        value[key]
            .as_array()
            .map(|all| all.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    Some(Recalled {
        files: names("files"),
        folders: names("folders"),
        shown: value["shown"].as_str().map(String::from),
    })
}

/// The folders opened before, newest first, kept for the person rather than
/// for a project: `$XDG_CONFIG_HOME/tracelean/recent.json`, else under
/// `~/.config`.
fn recent_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))?;
    Some(base.join("tracelean").join("recent.json"))
}

/// How many folders are remembered.
const RECENT: usize = 8;

/// A folder as the recent list names it: its full path.
pub fn named(root: &Path) -> String {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display().to_string()
}

/// Put `root` first among the folders opened before, and answer the list.
pub fn note_recent(root: &Path) -> Vec<String> {
    let Some(file) = recent_file() else { return Vec::new() };
    let shown = named(root);
    let mut list: Vec<String> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default();
    list.retain(|p| *p != shown && Path::new(p).is_dir());
    list.insert(0, shown);
    list.truncate(RECENT);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, serde_json::to_string_pretty(&list).unwrap_or_default());
    list
}

/// Keep it. A failure costs the next start its tabs, so it is not reported.
pub fn save(root: &Path, recalled: &Recalled) {
    let value = json!({ "files": recalled.files, "folders": recalled.folders, "shown": recalled.shown });
    let Ok(text) = serde_json::to_string_pretty(&value) else { return };
    let path = root.join(FILE);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}
