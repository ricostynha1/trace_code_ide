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

/// Parse a requirement markdown file
pub fn parse_requirement(path: &Path, root: &Path) -> Option<RequirementInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;

    let heading = first_line.strip_prefix("# ")?;
    let (id, title) = heading.split_once(':')?;
    let id = id.trim().to_string();
    let title = title.trim().to_string();

    let status = content
        .lines()
        .find(|line| line.starts_with("Status:"))
        .map(|line| ReqStatus::from_str(line.strip_prefix("Status:").unwrap_or("draft")))
        .unwrap_or(ReqStatus::Draft);

    // Description is everything after status line
    let description = content
        .lines()
        .skip_while(|line| !line.starts_with("Status:"))
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    // Check if spec file exists
    let spec_path = root.join("specs").join(format!("{}.lean", id));
    let has_spec = spec_path.exists();

    Some(RequirementInfo {
        id,
        title,
        status,
        file: rel,
        description,
        has_spec,
    })
}

/// List all requirements from the reqs/ folder
pub fn list_requirements(root: &Path) -> Vec<RequirementInfo> {
    let reqs_dir = root.join("reqs");
    if !reqs_dir.is_dir() {
        return Vec::new();
    }

    let mut reqs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&reqs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(req) = parse_requirement(&path, root) {
                    reqs.push(req);
                }
            }
        }
    }
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
/// Requires `lean` binary in PATH.
pub fn check_lean_file(file_path: &Path) -> LeanCheckResult {
    let output = ProcessCommand::new("lean")
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
                message: format!("Failed to run lean: {}", e),
                severity: "error".into(),
            }],
            warnings: Vec::new(),
        },
    }
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
