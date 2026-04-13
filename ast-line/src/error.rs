//! Error types for ast-line.

use thiserror::Error;

/// All errors that can arise during indexing.
#[derive(Debug, Error)]
pub enum SnifferError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("JSON error: {0}")]
    Json(String),
}
