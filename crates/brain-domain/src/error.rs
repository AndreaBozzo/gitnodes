use thiserror::Error;

/// Domain error for brain_ui operations.
///
/// Server functions convert this into `ServerFnError` at the edge; internal
/// code uses typed variants so matching on failure modes is possible.
#[derive(Debug, Error)]
pub enum BrainError {
    #[error("not authenticated")]
    Unauthenticated,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("github api: {0}")]
    GitHub(String),

    #[error("parse: {0}")]
    Parse(String),

    #[error("io: {0}")]
    Io(String),

    /// Optimistic-concurrency conflict: a precondition declared by the caller
    /// (file expected to be absent, or expected to have a specific sha) did
    /// not hold against the current state of the target. Distinct from a
    /// `GitHub` 422 fast-forward retry path: this surfaces *to the user* as a
    /// "reload and retry" scenario, never as a transparent retry.
    #[error("conflict: {0}")]
    Conflict(String),

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
    pub fn conflict(msg: impl Into<String>) -> Self {
        BrainError::Conflict(msg.into())
    }
    pub fn other(msg: impl Into<String>) -> Self {
        BrainError::Other(msg.into())
    }
}
