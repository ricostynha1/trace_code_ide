//! Requirements management — MVP 3
//!
//! Handles requirement status workflow, auto-creation of spec files,
//! and Lean compiler integration.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

/// Requirement status workflow: Draft → Approved → Linked
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReqStatus {
    Draft,
    Approved,
    Linked,
}

impl ReqStatus {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "approved" => ReqStatus::Approved,
            "linked" => ReqStatus::Linked,
            _ => ReqStatus::Draft,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ReqStatus::Draft => "draft",
            ReqStatus::Approved => "approved",
            ReqStatus::Linked => "linked",
        }
    }

    /// Valid transitions: Draft→Approved, Approved→Linked
    pub fn can_transition_to(&self, target: &ReqStatus) -> bool {
        matches!(
            (self, target),
            (ReqStatus::Draft, ReqStatus::Approved)
                | (ReqStatus::Approved, ReqStatus::Linked)
                // Allow going back
                | (ReqStatus::Linked, ReqStatus::Approved)
                | (ReqStatus::Approved, ReqStatus::Draft)
        )
    }
}

/// Parsed requirement from markdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementInfo {
    pub id: String,
    pub title: String,
    pub status: ReqStatus,
    pub file: PathBuf,
    pub description: String,
    pub has_spec: bool,
}

impl RequirementInfo {
    fn from_trace(req: &crate::trace::Requirement, modeled: bool) -> Self {
        Self {
            id: req.id.clone(),
            title: req.title.clone(),
            status: match req.status {
                crate::trace::ReqStatus::Draft => ReqStatus::Draft,
                crate::trace::ReqStatus::Approved => ReqStatus::Approved,
                crate::trace::ReqStatus::Linked => ReqStatus::Linked,
            },
            file: req.file.clone(),
            description: req.body.clone(),
            has_spec: modeled,
        }
    }
}

/// Parse a requirement markdown file.
///
/// Delegates to `crate::trace::requirement`, which reads frontmatter (and
/// falls back to the older `# REQ-01: Title` heading form). Identity comes
/// from the document's `id`, never from where the file lives.
pub fn parse_requirement(path: &Path, root: &Path) -> Option<RequirementInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    match crate::trace::requirement::parse_markdown(path, root, &content) {
        crate::trace::ParseOutcome::Requirement(req, _) => Some(RequirementInfo::from_trace(&req, false)),
        crate::trace::ParseOutcome::NotARequirement => None,
    }
}

/// List every requirement in the project.
///
/// Walks the whole tree rather than a `reqs/` directory: TraceLean imposes no
/// layout on the projects it traces. `has_spec` now means "something carries an
/// `@models` annotation for this requirement", not "a file named after it
/// exists in `specs/`".
pub fn list_requirements(root: &Path) -> Vec<RequirementInfo> {
    let index = crate::trace::build(root);
    let mut reqs: Vec<RequirementInfo> = index
        .requirements
        .values()
        .map(|req| {
            let modeled = index
                .links_for(&req.id)
                .iter()
                .any(|l| l.role == crate::trace::Role::Models);
            RequirementInfo::from_trace(req, modeled)
        })
        .collect();
    reqs.sort_by(|a, b| a.id.cmp(&b.id));
    reqs
}

/// Update status line in a requirement file. Returns new file content.
pub fn update_requirement_status(content: &str, new_status: &ReqStatus) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let status_idx = lines.iter().position(|l| l.starts_with("Status:"));

    let status_line = format!("Status: {}", new_status.as_str());

    if let Some(idx) = status_idx {
        lines[idx] = status_line;
    } else {
        // Insert after first line (heading)
        if lines.len() >= 1 {
            lines.insert(1, status_line);
        } else {
            lines.push(status_line);
        }
    }

    lines.join("\n")
}

/// Generate initial Lean spec content for a requirement
pub fn generate_spec_template(req: &RequirementInfo) -> String {
    format!(
        r#"-- Formal specification for {}: {}
-- Define datatypes and function signatures here.
-- The implementation must match these types exactly.

-- TODO: Define domain types (enums, structures)
-- TODO: Define function contracts (input → output types)
-- TODO: Add properties (optional correctness theorems)

/-- Placeholder type — replace with actual domain model. -/
structure Placeholder where
  field : String
"#,
        req.id, req.title
    )
}

/// Result from Lean compiler invocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeanCheckResult {
    pub success: bool,
    pub errors: Vec<LeanDiagnostic>,
    pub warnings: Vec<LeanDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeanDiagnostic {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub message: String,
    pub severity: String,
}

/// Run Lean 4 type checker on a file.
/// Requires `lean` binary in PATH or installed via elan.
pub fn check_lean_file(file_path: &Path) -> LeanCheckResult {
    let lean_bin = find_lean_binary();

    let output = ProcessCommand::new(&lean_bin)
        .arg(file_path)
        .arg("--run")
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();

            if out.status.success() {
                LeanCheckResult {
                    success: true,
                    errors: Vec::new(),
                    warnings: parse_lean_diagnostics(&stdout, "warning"),
                }
            } else {
                LeanCheckResult {
                    success: false,
                    errors: parse_lean_diagnostics(&stderr, "error"),
                    warnings: parse_lean_diagnostics(&stderr, "warning"),
                }
            }
        }
        Err(e) => LeanCheckResult {
            success: false,
            errors: vec![LeanDiagnostic {
                file: file_path.to_string_lossy().to_string(),
                line: 0,
                col: 0,
                message: format!("Failed to run lean ({}): {}", lean_bin, e),
                severity: "error".into(),
            }],
            warnings: Vec::new(),
        },
    }
}

/// Find the lean binary: check elan paths, then fall back to PATH.
fn find_lean_binary() -> String {
    // Check common elan install locations
    let candidates = [
        // Linux/macOS standard elan
        dirs_home().map(|h| format!("{}/.elan/bin/lean", h)),
        // Also try /root for Docker containers
        Some("/root/.elan/bin/lean".to_string()),
    ];

    for candidate in candidates.into_iter().flatten() {
        if std::path::Path::new(&candidate).exists() {
            return candidate;
        }
    }

    // Fall back to bare name (relies on PATH)
    "lean".to_string()
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

/// Parse Lean compiler output into diagnostics.
/// Format: "file:line:col: severity: message"
fn parse_lean_diagnostics(output: &str, filter_severity: &str) -> Vec<LeanDiagnostic> {
    let mut diagnostics = Vec::new();

    for line in output.lines() {
        // Try to parse "file.lean:line:col: error/warning: message"
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() >= 4 {
            let file = parts[0].trim().to_string();
            let line_num = parts[1].trim().parse::<u32>().unwrap_or(0);
            let col = parts[2].trim().parse::<u32>().unwrap_or(0);
            let rest = parts[3].trim();

            let (severity, message) = if let Some(msg) = rest.strip_prefix("error:") {
                ("error", msg.trim())
            } else if let Some(msg) = rest.strip_prefix("warning:") {
                ("warning", msg.trim())
            } else {
                ("error", rest)
            };

            if severity == filter_severity || filter_severity.is_empty() {
                diagnostics.push(LeanDiagnostic {
                    file,
                    line: line_num,
                    col,
                    message: message.to_string(),
                    severity: severity.to_string(),
                });
            }
        }
    }

    diagnostics
}
