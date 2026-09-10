//! Tauri IPC command modules — split by domain.
//! Each module contains thin dispatch functions (no business logic).

pub mod editor;
pub mod trace;
pub mod ai_commands;
pub mod mcp_commands;
pub mod acp_commands;
pub mod myth_commands;
pub mod terminal_commands;
pub mod sandbox_commands;
