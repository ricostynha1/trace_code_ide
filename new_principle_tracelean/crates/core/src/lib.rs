//! TraceLean core.
//!
//! Every item here is linked to a requirement clause by an annotation in a
//! comment. Nothing is linked by its path: this file could be called anything
//! and every claim in it would survive.

pub mod drt;
pub mod evidence;
pub mod history;
pub mod judge;
pub mod observe;
pub mod surface;
pub mod trace;
pub mod wire;
