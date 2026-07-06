//! Error types for surgical editing operations.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SurgicalEditError {
    // --- AST Edit errors ---
    #[error("AST node not found: {0}")]
    NodeNotFound(String),

    #[error("Ambiguous AST node: found {count} matches for '{description}'")]
    AmbiguousNode { description: String, count: usize },

    #[error("Node type mismatch: expected '{expected}', got '{got}'")]
    NodeTypeMismatch { expected: String, got: String },

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Language not supported: {0}")]
    UnsupportedLanguage(String),

    // --- Patch Edit errors ---
    #[error("Patch does not apply cleanly: {0}")]
    PatchFailed(String),

    #[error("Invalid patch format: {0}")]
    InvalidPatch(String),

    // --- Search & Replace errors ---
    #[error("Search text not found in file")]
    SearchTextNotFound,

    #[error("Search text matches {0} locations (expected exactly 1)")]
    MultipleMatches(usize),

    // --- General ---
    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("IO error: {0}")]
    IoError(String),
}
