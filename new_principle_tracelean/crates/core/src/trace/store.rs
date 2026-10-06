//! Where a backend leaves the record it earned.
//!
//! A directory of one file per record rather than one file holding all of them,
//! because the backends run as separate processes and often at the same time:
//! thirty suites appending to one document is a race, and the record that loses
//! it is evidence that silently never existed.
//!
//! Nothing here decides anything. The record arrives made, this writes it down,
//! and `lockfile` folds the directory into the committed index.

use std::path::{Path, PathBuf};

use super::record::Evidence;

/// Where a project keeps earned records.
pub fn dir_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("evidence")
}

/// The file one record belongs in.
///
/// Named from the slot it occupies — requirement, clause, bond — so a backend
/// re-running overwrites its own answer instead of accumulating answers, and so
/// the name is a function of the record rather than of when it was written.
///
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn file_of(root: &Path, record: &Evidence) -> PathBuf {
    let clause = record.key.clause.as_deref().unwrap_or("_");
    let bond = format!("{:?}", record.key.bond);
    dir_in(root).join(format!("{}.{clause}.{bond}.json", record.key.req_id))
}

/// Write one record where the lock builder will find it.
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

/// Every record a backend has left, sorted by the file it is in.
///
/// A file that does not parse is skipped rather than fatal: a half-written
/// record from a killed run must not stop the rest of the evidence being read.
/// It is also not silently equivalent to a valid one — it simply is not there,
/// and a missing record shows up as a clause at a lower level.
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

/// Remove one record, for a backend that has decided it no longer holds.
pub fn remove(root: &Path, record: &Evidence) -> std::io::Result<()> {
    match std::fs::remove_file(file_of(root, record)) {
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
    fn a_project_with_no_store_has_no_evidence_and_no_error() {
        assert!(read_all(&scratch("absent")).is_empty());
    }
}
