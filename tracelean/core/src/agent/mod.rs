//! Headless agent runtime — the LLM tool loop decoupled from any UI framework.

pub mod context;
pub mod error;
pub mod runtime;

pub use context::{AgentContext, PauseHandler};
pub use error::AgentError;
pub use runtime::{run_agent_turn, AgentTurnResult, ToolCallRecord};
