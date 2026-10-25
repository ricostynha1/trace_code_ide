use std::path::Path;

use serde_json::{json, Value};

pub const FILE: &str = ".tracelean/editor.json";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recalled {
    pub files: Vec<String>,
    pub folders: Vec<String>,
    pub shown: Option<String>,
}

/// @implements REQ-NAV.session_recalled
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

fn recent_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))?;
    Some(base.join("tracelean").join("recent.json"))
}

const RECENT: usize = 8;

pub fn named(root: &Path) -> String {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display().to_string()
}

/// @implements REQ-SHOW.recent_reopens
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

pub fn save(root: &Path, recalled: &Recalled) {
    let value = json!({ "files": recalled.files, "folders": recalled.folders, "shown": recalled.shown });
    let Ok(text) = serde_json::to_string_pretty(&value) else { return };
    let path = root.join(FILE);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}
