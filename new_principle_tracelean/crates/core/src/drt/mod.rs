//! Differential testing.

pub mod auto;
pub mod coverage;
pub mod config;
pub mod derive;
pub mod gen;
pub mod lean_runner;
pub mod pins;
pub mod protocol;
pub mod rust_runner;
pub mod run;
pub mod schema;
pub mod signature;
pub mod ts_runner;

pub use config::{qualified_op, Binding, CallSpec};
pub use protocol::{Case, Reply, RunnerError};
pub mod frontend;
