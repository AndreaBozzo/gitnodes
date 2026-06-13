//! Backwards-compatible entry points for the pre-transaction rename API.
//!
//! The implementation now lives in [`crate::git_transaction`], where save,
//! delete, and rename all share the same Git Data API transaction pipeline.

pub use crate::git_transaction::{BackoffPolicy, RenameMutation, RenameOutcome, run};
