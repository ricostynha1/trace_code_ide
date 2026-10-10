//! Running a Rust project's tests one at a time under coverage.
//!
//! The shell for `trace::lines`: build the tests with `-C instrument-coverage`
//! into a target directory outside the project, list each test binary's tests,
//! run each test alone with its own profile, and ask the toolchain's
//! `llvm-cov` for its line counts. Slower than one run of everything, and the
//! only way to know which test reached a line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::trace::lines::{lcov, merged, Coverage, TestLines};

/// The toolchain's own LLVM tools (`rustup component add llvm-tools`).
fn llvm_tool(name: &str) -> Result<PathBuf, String> {
    let sysroot = Command::new("rustc").args(["--print", "sysroot"]).output().map_err(|e| e.to_string())?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim());
    let lib = sysroot.join("lib").join("rustlib");
    for entry in std::fs::read_dir(&lib).map_err(|e| e.to_string())?.flatten() {
        let candidate = entry.path().join("bin").join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("no `{name}` in {}: run `rustup component add llvm-tools`", lib.display()))
}

/// The test executables `cargo test --no-run` built, with the package
/// directory each runs from.
fn test_binaries(root: &Path, target: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let built = Command::new("cargo")
        .args(["test", "--no-run", "--message-format=json", "--quiet"])
        .env("RUSTFLAGS", "-C instrument-coverage")
        .env("CARGO_TARGET_DIR", target)
        .env("LLVM_PROFILE_FILE", target.join("build-%p.profraw"))
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !built.status.success() {
        return Err(format!("the tests did not build:\n{}", String::from_utf8_lossy(&built.stderr)));
    }
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&built.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        let (Some(exe), Some(manifest)) = (message["executable"].as_str(), message["manifest_path"].as_str()) else {
            continue;
        };
        let package = Path::new(manifest).parent().unwrap_or(root).to_path_buf();
        out.push((PathBuf::from(exe), package));
    }
    Ok(out)
}

/// The line counts an instrumented `binary` left in `profiles` (every
/// `.profraw` there, merged), per file of the project at `root`, by path
/// relative to it.
pub fn profiled_lines(root: &Path, binary: &Path, profiles: &Path) -> Result<Vec<(String, Vec<(u32, u64)>)>, String> {
    let (profdata, cov) = (llvm_tool("llvm-profdata")?, llvm_tool("llvm-cov")?);
    let raws: Vec<PathBuf> = std::fs::read_dir(profiles)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "profraw"))
        .collect();
    if raws.is_empty() {
        return Err(format!("no profile in {}: was the runner built instrumented?", profiles.display()));
    }
    let data = profiles.join("merged.profdata");
    let merged_ok =
        Command::new(&profdata).args(["merge", "-sparse"]).args(&raws).arg("-o").arg(&data).status().map_err(|e| e.to_string())?;
    if !merged_ok.success() {
        return Err("llvm-profdata could not merge the profiles".into());
    }
    let exported = Command::new(&cov)
        .args(["export", "-format=lcov"])
        .arg(format!("-instr-profile={}", data.display()))
        .arg(binary)
        .output()
        .map_err(|e| e.to_string())?;
    let prefix = format!("{}/", root.display());
    Ok(lcov(&String::from_utf8_lossy(&exported.stdout))
        .into_iter()
        .filter_map(|(file, lines)| file.strip_prefix(&prefix).map(|rel| (rel.to_string(), lines)))
        .collect())
}

/// Measure every test of the project at `root`, and the coverage of each of
/// its files that a test reached or could have.
pub fn measure(root: &Path, files: &BTreeMap<String, String>) -> Result<(Coverage, usize), String> {
    let (profdata, cov) = (llvm_tool("llvm-profdata")?, llvm_tool("llvm-cov")?);
    let scratch = std::env::temp_dir().join(format!("tracelean-lines-{}", crate::trace::hash::text(&root.display().to_string())));
    let target = scratch.join("target");
    let profiles = scratch.join("profiles");
    let _ = std::fs::remove_dir_all(&profiles);
    std::fs::create_dir_all(&profiles).map_err(|e| e.to_string())?;
    let prefix = format!("{}/", root.display());

    let mut tests: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for (binary, package) in test_binaries(root, &target)? {
        let listed = Command::new(&binary)
            .args(["--list", "--format", "terse"])
            .env("LLVM_PROFILE_FILE", profiles.join("list.profraw"))
            .current_dir(&package)
            .output()
            .map_err(|e| e.to_string())?;
        for l in String::from_utf8_lossy(&listed.stdout).lines() {
            if let Some(name) = l.strip_suffix(": test") {
                tests.push((binary.clone(), package.clone(), name.to_string()));
            }
        }
    }

    // Each test alone in its own process, several processes at once, each
    // leaving its profile under its own number.
    let numbered: Vec<(usize, &(PathBuf, PathBuf, String))> = tests.iter().enumerate().collect();
    let measured = super::run::par_map(&numbered, |(i, (binary, package, name))| -> Option<Vec<TestLines>> {
        let raw = profiles.join(format!("{i}.profraw"));
        let data = profiles.join(format!("{i}.profdata"));
        let ran = Command::new(binary)
            .args(["--exact", name, "--test-threads", "1", "--quiet"])
            .env("LLVM_PROFILE_FILE", &raw)
            .current_dir(package)
            .output()
            .ok()?;
        // A test that did not run left nothing to read; one that failed
        // still ran the lines it ran.
        if !raw.is_file() || ran.stdout.windows(9).any(|w| w == b"1 ignored") {
            return None;
        }
        let merged_ok = Command::new(&profdata).args(["merge", "-sparse"]).arg(&raw).arg("-o").arg(&data).status().ok()?;
        let exported = merged_ok
            .success()
            .then(|| {
                Command::new(&cov)
                    .args(["export", "-format=lcov"])
                    .arg(format!("-instr-profile={}", data.display()))
                    .arg(binary)
                    .output()
                    .ok()
            })
            .flatten();
        let _ = (std::fs::remove_file(&raw), std::fs::remove_file(&data));
        let Some(exported) = exported else { return Some(Vec::new()) };
        Some(
            lcov(&String::from_utf8_lossy(&exported.stdout))
                .into_iter()
                .filter_map(|(file, lines)| {
                    let rel = file.strip_prefix(&prefix)?;
                    files.contains_key(rel).then(|| TestLines { test: name.clone(), file: rel.to_string(), lines })
                })
                .collect(),
        )
    });
    let count = measured.iter().flatten().count();
    let runs: Vec<TestLines> = measured.into_iter().flatten().flatten().collect();

    let mut coverage = Coverage::default();
    let mut paths: Vec<String> = runs.iter().map(|r| r.file.clone()).collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        let hash = crate::trace::hash::text(files.get(&path).map(String::as_str).unwrap_or(""));
        coverage.files.insert(path.clone(), (hash, merged(runs.clone(), path)));
    }
    Ok((coverage, count))
}

/// Where a project keeps its measured coverage.
pub fn path_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("coverage.json")
}

pub fn write(root: &Path, coverage: &Coverage) -> Result<(), String> {
    let path = path_in(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string(coverage).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())
}

pub fn read(root: &Path) -> Coverage {
    std::fs::read_to_string(path_in(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The kept coverage, when it was written after `since`: what an editor asks
/// each frame without reading the file each frame.
pub fn read_if_newer(root: &Path, since: Option<std::time::SystemTime>) -> Option<(std::time::SystemTime, Coverage)> {
    let written = std::fs::metadata(path_in(root)).and_then(|m| m.modified()).ok()?;
    if since.is_some_and(|seen| seen >= written) {
        return None;
    }
    Some((written, read(root)))
}
