use thiserror::Error;

/// Domain error for brain_ui operations.
///
/// Server functions convert this into `ServerFnError` at the edge; internal
/// code uses typed variants so matching on failure modes is possible.
#[derive(Debug, Error)]
pub enum BrainError {
    #[error("not authenticated")]
    Unauthenticated,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("github api: {0}")]
    GitHub(String),

    #[error("parse: {0}")]
    Parse(String),

    #[error("io: {0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}

impl BrainError {
    pub fn github(msg: impl Into<String>) -> Self {
        BrainError::GitHub(msg.into())
    }
    pub fn parse(msg: impl Into<String>) -> Self {
        BrainError::Parse(msg.into())
    }
    pub fn other(msg: impl Into<String>) -> Self {
        BrainError::Other(msg.into())
    }
}
