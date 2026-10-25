use std::path::{Path, PathBuf};

use super::record::{Evidence, Key};

pub fn dir_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("evidence")
}

/// @implements ARCH-DETERMINISM.stable_ordering
pub fn file_of(root: &Path, record: &Evidence) -> PathBuf {
    slot_file(root, &record.key)
}

fn slot_file(root: &Path, key: &Key) -> PathBuf {
    let clause = key.clause.as_deref().unwrap_or("_");
    let bond = format!("{:?}", key.bond);
    dir_in(root).join(format!("{}.{clause}.{bond}.json", key.req_id))
}

pub fn write(root: &Path, record: &Evidence) -> std::io::Result<PathBuf> {
    let path = file_of(root, record);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(record).unwrap_or_default();
    text.push('\n');
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn read_all(root: &Path) -> Vec<Evidence> {
    let Ok(entries) = std::fs::read_dir(dir_in(root)) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|text| serde_json::from_str::<Evidence>(&text).ok())
        .collect()
}

pub fn remove(root: &Path, record: &Evidence) -> std::io::Result<()> {
    remove_slot(root, &record.key)
}

pub fn remove_slot(root: &Path, key: &Key) -> std::io::Result<()> {
    match std::fs::remove_file(slot_file(root, key)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{Bond, Level};
    use crate::trace::record::{Detail, Key};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tracelean-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn record(clause: &str) -> Evidence {
        Evidence {
            key: Key {
                req_id: "REQ-A".into(),
                clause: Some(clause.into()),
                bond: Bond::ModelImpl,
            },
            level: Level::L3,
            detail: Detail::Drt { seed: 1, cases: 500, op: "REQ-A.one".into() },
            link_hash: "abc".into(),
            inputs: vec![("model".into(), "m".into()), ("implementation".into(), "i".into())],
        }
    }

    #[test]
    fn a_record_written_twice_is_one_record() {
        let dir = scratch("twice");
        write(&dir, &record("one")).unwrap();
        write(&dir, &record("one")).unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_come_back_in_the_same_order_every_time() {
        let dir = scratch("order");
        for clause in ["two", "one", "three"] {
            write(&dir, &record(clause)).unwrap();
        }
        let first: Vec<_> = read_all(&dir).iter().map(|r| r.key.clause.clone()).collect();
        let again: Vec<_> = read_all(&dir).iter().map(|r| r.key.clause.clone()).collect();
        assert_eq!(first, again);
        assert_eq!(first.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_does_not_parse_does_not_take_the_others_with_it() {
        let dir = scratch("broken");
        write(&dir, &record("one")).unwrap();
        std::fs::write(dir_in(&dir).join("REQ-B._.ModelImpl.json"), "{ not json").unwrap();
        assert_eq!(read_all(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emptying_a_slot_removes_only_its_record() {
        let dir = scratch("slot");
        write(&dir, &record("one")).unwrap();
        write(&dir, &record("two")).unwrap();
        remove_slot(&dir, &record("one").key).unwrap();
        remove_slot(&dir, &record("one").key).unwrap();
        let left: Vec<_> = read_all(&dir).iter().map(|r| r.key.clause.clone()).collect();
        assert_eq!(left, vec![Some("two".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_store_has_no_evidence_and_no_error() {
        assert!(read_all(&scratch("absent")).is_empty());
    }
}
