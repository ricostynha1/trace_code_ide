//! Trace graph and requirements IPC commands.

use crate::requirements;
use crate::service;
use crate::trace_graph::{self, RequirementTraceOwned, CodeElementTraceOwned};
use crate::{AppStateWrapper, SymbolTableWrapper, TraceGraphWrapper, TraceGraphStats};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn build_trace_graph(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
) -> Result<TraceGraphStats, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let mut g = graph_state.0.lock().map_err(|e| e.to_string())?;
    let (nodes, edges) = service::build_trace_graph(&s, &sym, &mut g)?;
    Ok(TraceGraphStats { nodes, edges })
}

#[tauri::command]
pub fn query_requirement_trace(
    graph_state: State<'_, TraceGraphWrapper>,
    req_id: String,
) -> Result<Option<RequirementTraceOwned>, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(g.query_requirement_owned(&req_id))
}

#[tauri::command]
pub fn query_code_trace(
    graph_state: State<'_, TraceGraphWrapper>,
    file: String,
    name: String,
) -> Result<Option<CodeElementTraceOwned>, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(g.query_code_element_owned(&PathBuf::from(&file), &name))
}

#[tauri::command]
pub fn update_trace_graph_file(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    path: String,
) -> Result<bool, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let mut g = graph_state.0.lock().map_err(|e| e.to_string())?;
    service::update_trace_graph_file(&s, &sym, &mut g, &path)
}

#[tauri::command]
pub fn get_trace_graph_stats(graph_state: State<'_, TraceGraphWrapper>) -> Result<TraceGraphStats, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(TraceGraphStats { nodes: g.node_count(), edges: g.edge_count() })
}

#[tauri::command]
pub fn get_full_trace_graph(
    graph_state: State<'_, TraceGraphWrapper>,
) -> Result<trace_graph::FullTraceGraphExport, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(g.export_full_graph())
}

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
