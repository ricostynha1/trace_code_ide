//! Observing an external agent.
//!
//! The user runs a tool themselves, in a sandboxed copy of the project.
//! TraceLean watches that copy and mirrors what it finds. It never launches,
//! prompts, or drives the tool.

pub mod claude;
pub mod cost;
pub mod effects;
pub mod mirror;
pub mod policy;
pub mod sandbox;
pub mod transcript;
pub mod watch;
pub mod workcopy;
pub mod workspace;

pub use effects::{run_violations, RunWitness, Violation};
pub use policy::{classify, is_mirrored, Capability, Class};
