//! Traceability IPC commands: the project graph, requirements, coverage,
//! findings, evidence, and the differential-testing bindings.

use crate::requirements;
use crate::service;
use crate::AppStateWrapper;
use tauri::State;

// --- Requirements ---

#[tauri::command]
pub fn list_requirements(state: State<'_, AppStateWrapper>) -> Result<Vec<requirements::RequirementInfo>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::list_requirements(&s)
}

#[tauri::command]
pub fn update_requirement_status(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    new_status: String,
) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::update_requirement_status(&mut s, &req_id, &new_status)
}

#[tauri::command]
pub fn create_requirement(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    title: String,
) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::create_requirement(&mut s, &req_id, &title)
}

#[tauri::command]
pub fn check_lean_spec(state: State<'_, AppStateWrapper>, path: String) -> Result<requirements::LeanCheckResult, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::check_lean_spec(&s, &path)
}

#[tauri::command]
pub fn get_editor_mode(path: String) -> String {
    service::get_editor_mode(&path).to_string()
}

#[tauri::command]
pub fn navigate_trace_link(state: State<'_, AppStateWrapper>, from_path: String) -> Result<Option<String>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::navigate_trace_link(&s, &from_path)
}

// --- Annotation-based traceability ---

/// Rebuild the traceability index and write `.tracelean/trace.lock.json`.
#[tauri::command]
pub fn trace_scan(state: State<'_, AppStateWrapper>) -> Result<crate::trace::TraceIndex, String> {
    let s = state.0.lock().unwrap();
    service::trace_scan(&s)
}

/// Findings only, for the problems view and the CI gate.
#[tauri::command]
pub fn trace_findings(state: State<'_, AppStateWrapper>) -> Result<Vec<crate::trace::Finding>, String> {
    let s = state.0.lock().unwrap();
    service::trace_findings(&s)
}

/// One requirement with its clauses, links and per-bond assurance.
#[tauri::command]
pub fn trace_requirement(
    state: State<'_, AppStateWrapper>,
    req_id: String,
) -> Result<crate::trace::RequirementView, String> {
    let s = state.0.lock().unwrap();
    service::trace_requirement(&s, &req_id)
}

/// Every requirement, summarized for the panel.
#[tauri::command]
pub fn trace_overview(
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<crate::trace::RequirementSummary>, String> {
    let s = state.0.lock().unwrap();
    service::trace_overview(&s)
}

/// Run differential testing for a requirement clause.
#[tauri::command]
pub fn drt_run(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
    seed: Option<u64>,
    cases: Option<usize>,
) -> Result<crate::drt::DrtResult, String> {
    let s = state.0.lock().unwrap();
    service::drt_run(
        &s,
        &req_id,
        clause.as_deref(),
        seed.unwrap_or(1),
        cases.unwrap_or(200),
    )
}

/// Write a binding and adapter skeleton for a requirement that has none.
#[tauri::command]
pub fn drt_scaffold(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
    lang: Option<String>,
) -> Result<String, String> {
    let s = state.0.lock().unwrap();
    service::drt_scaffold(
        &s,
        &req_id,
        clause.as_deref(),
        lang.as_deref().unwrap_or("rust"),
    )
}

/// What binding this clause to a differential test would do. Writes nothing.
#[tauri::command]
pub fn drt_bind_preview(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
) -> Result<tracelean_core::drt::BindProposal, String> {
    let s = state.0.lock().unwrap();
    service::drt_bind_preview(&s, &req_id, clause.as_deref())
}

/// Carry out a binding proposal, through the undo tree.
///
/// The three writes — adapter file, `.tracelean/drt.json` entry, and the `@drt`
/// annotation that lives inside the generated adapter — go in as one batch, so
/// the project never sits in a state where a binding exists in two of its three
/// places. Undo removes all three.
#[tauri::command]
pub fn drt_bind_apply(
    app: tauri::AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, crate::UndoTreeCacheWrapper>,
    req_id: String,
    clause: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;

    let (proposal, command) = {
        let s = state.0.lock().unwrap();
        service::drt_bind_commands(&s, &req_id, clause.as_deref())?
    };

    {
        let mut s = state.0.lock().unwrap();
        // Files the batch edits must be in the buffer map first: `Replace`
        // verifies its `old` witness against the buffer, and a file nobody has
        // opened has no buffer.
        let root = s.project_root().cloned().ok_or("No project open")?;
        for path in [std::path::PathBuf::from(".tracelean/drt.json")] {
            if s.get_content(&path).is_none() {
                if let Ok(content) = std::fs::read_to_string(root.join(&path)) {
                    s.load_file(path, content);
                }
            }
        }
        service::apply_command(&mut s, command)?;
        super::editor::sync_file_operations_to_disk(&s);
    }

    crate::invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());
    let _ = app.emit("files-changed", ());

    serde_json::to_value(&proposal).map_err(|e| e.to_string())
}

/// Judge whether the model still formalizes a requirement clause.
#[tauri::command]
pub async fn judge_clause(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
) -> Result<serde_json::Value, String> {
    // The state guard is dropped before awaiting: holding a mutex across an
    // await would block every other IPC call for the duration of a model call.
    let snapshot = {
        let s = state.0.lock().unwrap();
        s.clone()
    };
    let judgement = service::judge_clause(&snapshot, &req_id, clause.as_deref()).await?;
    serde_json::to_value(JudgementView::from(&judgement)).map_err(|e| e.to_string())
}

/// The judge prompt as pasteable text, with no provider call.
#[tauri::command]
pub fn judge_prompt(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
) -> Result<service::JudgePrompt, String> {
    let s = state.0.lock().unwrap();
    service::judge_prompt(&s, &req_id, clause.as_deref())
}

/// Record a judge reply produced outside the app.
///
/// Same pipeline as the API path: parsed, and its witness executed. A pasted
/// verdict is worth what an API one is worth, and not a level more.
#[tauri::command]
pub fn judge_apply_reply(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
    reply: String,
) -> Result<serde_json::Value, String> {
    let s = state.0.lock().unwrap();
    let judgement = service::judge_apply_reply(&s, &req_id, clause.as_deref(), &reply)?;
    serde_json::to_value(JudgementView::from(&judgement)).map_err(|e| e.to_string())
}

/// Re-judge every stale requirement↔model bond, up to `limit`.
#[tauri::command]
pub async fn judge_stale(
    state: State<'_, AppStateWrapper>,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let snapshot = {
        let s = state.0.lock().unwrap();
        s.clone()
    };
    service::judge_stale(&snapshot, limit.unwrap_or(10)).await
}

/// The judge's answer, flattened for the frontend.
#[derive(serde::Serialize)]
pub struct JudgementView {
    pub verdict: String,
    pub explanation: String,
    pub confidence: String,
    /// Whether the witness was executed and behaved as claimed. `null` means
    /// there was nothing to check.
    pub witness_confirmed: Option<bool>,
    /// True when no model runner was available, so nothing could be checked.
    pub degraded: bool,
    pub unreliable: bool,
    pub cost_usd: f64,
    pub suggested_patch: Option<String>,
    pub witness_input: Option<serde_json::Value>,
    /// `"api"` or `"pasted"`.
    pub source: String,
}

impl From<&crate::judge::Judgement> for JudgementView {
    fn from(j: &crate::judge::Judgement) -> Self {
        Self {
            verdict: j.reply.verdict.as_str().to_string(),
            explanation: j.reply.explanation.clone(),
            confidence: format!("{:?}", j.reply.confidence).to_lowercase(),
            witness_confirmed: match &j.check {
                crate::judge::WitnessCheck::Confirmed { .. } => Some(true),
                crate::judge::WitnessCheck::Falsified { .. } => Some(false),
                crate::judge::WitnessCheck::Inexecutable(_) => None,
            },
            degraded: j.degraded,
            unreliable: j.unreliable,
            cost_usd: j.cost_usd,
            suggested_patch: j.reply.suggested_patch.clone(),
            witness_input: j.reply.witness.as_ref().map(|w| w.input.clone()),
            source: j.source.clone(),
        }
    }
}

/// Generate (and build, if a Lean toolchain is present) the model runner for a
/// requirement clause from its `@models` annotations.
#[tauri::command]
pub fn drt_generate_runner(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    clause: Option<String>,
) -> Result<String, String> {
    let s = state.0.lock().unwrap();
    service::drt_generate_runner(&s, &req_id, clause.as_deref())
}

/// The requirement DAG as a tree, collapsed to `depth` when given.
#[tauri::command]
pub fn trace_tree(
    state: State<'_, AppStateWrapper>,
    depth: Option<usize>,
) -> Result<Vec<crate::trace::TreeNode>, String> {
    let s = state.0.lock().unwrap();
    service::trace_tree(&s, depth)
}

/// The coverage map, including untraced files.
#[tauri::command]
pub fn trace_coverage_map(
    state: State<'_, AppStateWrapper>,
) -> Result<crate::trace::CoverageMap, String> {
    let s = state.0.lock().unwrap();
    service::trace_coverage_map(&s)
}

/// Import a coverage report produced by the language's own tool.
#[tauri::command]
pub fn trace_import_coverage(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<String, String> {
    let s = state.0.lock().unwrap();
    service::trace_import_coverage(&s, &path)
}

/// Import test results produced by the runner.
#[tauri::command]
pub fn trace_import_test_results(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<String, String> {
    let s = state.0.lock().unwrap();
    service::trace_import_test_results(&s, &path)
}

/// The role graph: what relates to what, as somebody wrote it down.
#[tauri::command]
pub fn trace_role_graph(
    state: State<'_, AppStateWrapper>,
) -> Result<crate::trace::graph::RoleGraph, String> {
    let s = state.0.lock().unwrap();
    service::trace_role_graph(&s)
}

/// Ask the toolchain whether the `@pins` obligations are finished.
#[tauri::command]
pub fn trace_strength_check(
    state: State<'_, AppStateWrapper>,
) -> Result<Vec<(String, Option<String>, crate::trace::strength::Strength)>, String> {
    let s = state.0.lock().unwrap();
    service::trace_strength_check(&s)
}

/// Write the spec-strength obligation scaffold.
#[tauri::command]
pub fn trace_strength_scaffold(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<String, String> {
    let s = state.0.lock().unwrap();
    service::trace_strength_scaffold(&s, &path)
}

/// The project graph: code nodes with their requirements, assurance and
/// findings overlaid.
#[tauri::command]
pub fn trace_project_graph(
    state: State<'_, AppStateWrapper>,
    declaration_cap: Option<usize>,
) -> Result<crate::trace::graph::ProjectGraph, String> {
    let s = state.0.lock().unwrap();
    service::trace_project_graph(&s, declaration_cap)
}

/// Coverage and assurance over recent commits, for the progress chart.
#[tauri::command]
pub fn trace_history(
    state: State<'_, AppStateWrapper>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let s = state.0.lock().unwrap();
    let (points, problems) = service::trace_history(&s, limit.unwrap_or(30))?;
    Ok(serde_json::json!({ "points": points, "problems": problems }))
}

/// Links anchored in one file, for gutter chips.
#[tauri::command]
pub fn trace_links_in_file(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<Vec<crate::trace::Link>, String> {
    let s = state.0.lock().unwrap();
    service::trace_links_in_file(&s, &path)
}
