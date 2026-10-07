//! Asking Lean whether a `@pins` theorem pins its clause.
//!
//! A shell around `trace::pinning`: copy the theorem's file with the check
//! appended into a scratch directory, run `lean` on it — through `lake env`
//! from the nearest Lake package, after building it, when there is one — and
//! keep the verdict under `.tracelean/pins`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::trace::pinning::{accepted, check_source, record_path, PinPlan, PinRecord};

/// The nearest directory at or above `from`, within `root`, holding a Lake
/// package.
fn lake_root(root: &Path, from: &Path) -> Option<PathBuf> {
    let mut at = from.parent();
    while let Some(dir) = at {
        if ["lakefile.lean", "lakefile.toml"].iter().any(|f| dir.join(f).is_file()) {
            return Some(dir.to_path_buf());
        }
        if dir == root {
            return None;
        }
        at = dir.parent();
    }
    None
}

/// The verdict kept for a clause, if any.
pub fn read_record(root: &Path, req_id: &str, clause: Option<&str>) -> Option<PinRecord> {
    let text = std::fs::read_to_string(record_path(root, req_id, clause)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Check one clause's theorem, write the verdict, and return it. `None` when
/// the clause has no `@pins` theorem to check.
pub fn check(root: &Path, plan: &PinPlan) -> Option<PinRecord> {
    let (theorem, file) = (plan.theorem.clone()?, plan.file.clone()?);
    let source = root.join(&file);
    let text = std::fs::read_to_string(&source).ok()? + &check_source(plan)?;
    let scratch = std::env::temp_dir().join(format!("tracelean-pins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&scratch);
    let copy = scratch.join("Check.lean");
    std::fs::write(&copy, text).ok()?;

    let output = match lake_root(root, &source) {
        Some(package) => {
            let _ = Command::new("lake").arg("build").current_dir(&package).output();
            Command::new("lake").args(["env", "lean"]).arg(&copy).current_dir(&package).output()
        }
        None => Command::new("lean").arg(&copy).current_dir(root).output(),
    };
    let _ = std::fs::remove_dir_all(&scratch);
    let (said, ok) = match output {
        Ok(out) => (
            format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
            out.status.success(),
        ),
        Err(error) => (format!("could not run Lean: {error}"), false),
    };
    let pinned = accepted(theorem.clone(), said.clone(), ok);
    let record = PinRecord {
        theorem_name: theorem.clone(),
        key: plan.key.clone(),
        pinned,
        said: if pinned { String::new() } else { said.replace(&copy.display().to_string(), &file) },
    };
    let path = record_path(root, &plan.req_id, plan.clause.as_deref());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&record).unwrap_or_default() + "\n");
    Some(record)
}
