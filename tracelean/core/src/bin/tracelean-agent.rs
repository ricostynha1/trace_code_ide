//! tracelean-agent — standalone ACP agent binary.
//!
//! Speaks Agent Client Protocol over stdio.
//! Reads API keys from environment. Tool calls are handled by the IDE client.
//!
//! Usage:
//!   OPENROUTER_API_KEY=sk-... cargo run --bin tracelean-agent
//!   AWS_BEARER_TOKEN_BEDROCK=... cargo run --bin tracelean-agent
//!   TRACELEAN_DISABLE_CONTEXT_TRIMMING=1 cargo run --bin tracelean-agent  # bugs.md Bug 2

use tokio::sync::watch;
use tracelean_core::acp::builtin_agent::{run_builtin_agent, SteeringCommand};

#[tokio::main]
async fn main() {
    // Steering channel — parent process can send abort/redirect via signals
    let (_steering_tx, steering_rx) = watch::channel(SteeringCommand::None);

    eprintln!("[tracelean-agent] starting ACP agent over stdio...");

    if let Err(e) = run_builtin_agent(steering_rx).await {
        eprintln!("[tracelean-agent] fatal: {}", e);
        std::process::exit(1);
    }
}
