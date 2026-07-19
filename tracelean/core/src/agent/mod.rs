//! Headless agent runtime — the LLM tool loop decoupled from any UI framework.

pub mod context;
pub mod error;
pub mod runtime;
pub mod session;

pub use context::{AgentContext, PauseHandler};
pub use error::AgentError;
pub use runtime::{force_summarize, run_agent_turn, run_agent_turn_session, AgentTurnResult, ToolCallRecord};
pub use session::{ChatSession, ChatSessionInfo, ChatSessionSummary};
