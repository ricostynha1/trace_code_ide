//! Agent runtime error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("No model selected")]
    NoModel,

    #[error("Provider not configured: {0}")]
    ProviderNotConfigured(String),

    #[error("Spend cap reached (${spent:.4} >= ${cap:.2})")]
    SpendCapReached { spent: f64, cap: f64 },

    #[error("Too many consecutive failures ({0})")]
    TooManyFailures(u32),

    #[error("Stopped by user after {0} tool calls")]
    StoppedByUser(usize),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Lock error: {0}")]
    Lock(String),
}

impl From<AgentError> for String {
    fn from(e: AgentError) -> String {
        e.to_string()
    }
}
