//! Differential testing: the bond between the Lean reference model and the
//! implementation.
//!
//! This is the bond that makes the whole design language-independent. The
//! model and the implementation are compared by *behaviour* — same inputs,
//! same outputs — rather than by translating one into the other, so the
//! implementation may be Rust, Python or anything else, and may use `unsafe`,
//! generics, FFI and third-party crates freely.
//!
//! It buys falsification, not proof: a run that finds nothing is evidence, not
//! a guarantee, which is why coverage is reported next to the case count and
//! why the resulting evidence sits at L3 rather than L4.

pub mod bind;
pub mod config;
pub mod gen;
pub mod infer;
pub mod lean_runner;
pub mod protocol;
pub mod run;
pub mod schema;

pub use bind::BindProposal;
pub use config::{Binding, CallSpec, DrtConfig};
pub use infer::{infer_input, InferError};
pub use protocol::{Case, Reply, Runner, RunnerError, RunnerSpec};
pub use run::{Coverage, CoverageFloor, Divergence, DrtResult, RunOptions, Triage};
pub use schema::Schema;
