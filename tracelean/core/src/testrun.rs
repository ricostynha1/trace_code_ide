//! P12: project test runner — command execution + failure parsing.
//!
//! The IDE runs a configurable test command in the project root and parses
//! failures into clickable diagnostics. Parsers cover cargo test, pytest and
//! vitest output; unknown formats still show raw output.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result of running a project command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    /// Parsed failures (empty when everything passed or format unknown).
    pub failures: Vec<TestFailure>,
}

/// One parsed test failure, clickable when file/line are known.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    pub name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

/// Best-guess test command for a project (used until the user configures one
/// in {project}/.tracelean/test_command).
pub fn default_test_command(project_root: &Path) -> String {
    let has = |f: &str| project_root.join(f).exists();
    if has("Cargo.toml") {
        "cargo test".into()
    } else if has("pytest.ini") || has("setup.py") || has("pyproject.toml") {
        "pytest".into()
    } else if has("vitest.config.ts") || has("vitest.config.js") {
        "npx vitest run".into()
    } else if has("package.json") {
        "npm test".into()
    } else {
        "make test".into()
    }
}

/// The configured test command: {project}/.tracelean/test_command (one line),
/// falling back to auto-detection.
pub fn test_command(project_root: &Path) -> String {
    std::fs::read_to_string(project_root.join(".tracelean").join("test_command"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_test_command(project_root))
}

/// Persist the configured test command.
pub fn save_test_command(project_root: &Path, command: &str) -> Result<(), String> {
    let dir = project_root.join(".tracelean");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("test_command"), command.trim()).map_err(|e| e.to_string())
}

/// Parse test failures from combined output. Tries each known format.
pub fn parse_test_failures(output: &str) -> Vec<TestFailure> {
    let cargo = parse_cargo_failures(output);
    if !cargo.is_empty() {
        return cargo;
    }
    let pytest = parse_pytest_failures(output);
    if !pytest.is_empty() {
        return pytest;
    }
    parse_vitest_failures(output)
}

/// cargo test: `---- name stdout ----` blocks with
/// `thread 'name' panicked at src/file.rs:LINE:COL:` and the message below.
fn parse_cargo_failures(output: &str) -> Vec<TestFailure> {
    let mut failures = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("---- ") else {
            continue;
        };
        let Some(name) = rest.strip_suffix(" stdout ----") else {
            continue;
        };
        // Search the block for the panic location + message
        let mut file = None;
        let mut fline = None;
        let mut message = String::new();
        for l in lines.iter().skip(i + 1).take(20) {
            if l.starts_with("---- ") || l.starts_with("failures:") {
                break;
            }
            if let Some(at) = l.find("panicked at ") {
                let loc = l[at + "panicked at ".len()..].trim_end_matches(':');
                let mut parts = loc.rsplitn(3, ':');
                let _col = parts.next();
                let line_no = parts.next().and_then(|n| n.parse::<u32>().ok());
                let path = parts.next().map(|p| p.to_string());
                if path.is_some() {
                    file = path;
                    fline = line_no;
                }
            } else if !l.trim().is_empty() && message.len() < 500 {
                if !message.is_empty() {
                    message.push('\n');
                }
                message.push_str(l.trim());
            }
        }
        failures.push(TestFailure {
            name: name.to_string(),
            file,
            line: fline,
            message,
        });
    }
    failures
}

/// pytest: `FAILED tests/test_x.py::test_name - AssertionError: msg`
fn parse_pytest_failures(output: &str) -> Vec<TestFailure> {
    let mut failures = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("FAILED ") else {
            continue;
        };
        let (loc, msg) = match rest.split_once(" - ") {
            Some((l, m)) => (l, m.to_string()),
            None => (rest, String::new()),
        };
        let (file, name) = match loc.split_once("::") {
            Some((f, n)) => (Some(f.to_string()), n.to_string()),
            None => (None, loc.to_string()),
        };
        failures.push(TestFailure { name, file, line: None, message: msg });
    }
    failures
}

/// vitest: ` FAIL  src/x.test.ts > suite > name` or `❯ src/x.test.ts:LINE:COL`
fn parse_vitest_failures(output: &str) -> Vec<TestFailure> {
    let mut failures = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed
            .strip_prefix("FAIL ")
            .or_else(|| trimmed.strip_prefix("× "))
        else {
            continue;
        };
        let rest = rest.trim();
        let (file, name) = match rest.split_once(" > ") {
            Some((f, n)) => (Some(f.trim().to_string()), n.trim().to_string()),
            None => (None, rest.to_string()),
        };
        failures.push(TestFailure { name, file, line: None, message: String::new() });
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_failure_parsed_with_location() {
        let out = "\
running 2 tests
test math::works ... ok
test math::broken ... FAILED

failures:

---- math::broken stdout ----
thread 'math::broken' panicked at src/math.rs:42:9:
assertion `left == right` failed
  left: 4
 right: 5

failures:
    math::broken
";
        let f = parse_test_failures(out);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "math::broken");
        assert_eq!(f[0].file.as_deref(), Some("src/math.rs"));
        assert_eq!(f[0].line, Some(42));
        assert!(f[0].message.contains("assertion"));
    }

    #[test]
    fn pytest_failures_parsed() {
        let out = "\
==== FAILURES ====
FAILED tests/test_app.py::test_login - AssertionError: expected 200
FAILED tests/test_app.py::test_logout - KeyError: 'session'
==== 2 failed in 0.3s ====
";
        let f = parse_test_failures(out);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].file.as_deref(), Some("tests/test_app.py"));
        assert_eq!(f[0].name, "test_login");
        assert!(f[1].message.contains("KeyError"));
    }

    #[test]
    fn vitest_failures_parsed() {
        let out = " FAIL  src/sum.test.ts > sum > adds numbers\n";
        let f = parse_test_failures(out);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].file.as_deref(), Some("src/sum.test.ts"));
        assert_eq!(f[0].name, "sum > adds numbers");
    }

    #[test]
    fn clean_output_has_no_failures() {
        assert!(parse_test_failures("running 3 tests\ntest a ... ok\n").is_empty());
    }
}
