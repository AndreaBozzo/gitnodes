//! Pure domain types for brain_ui.
//!
//! This crate intentionally has zero runtime dependencies beyond `serde`. It
//! compiles on both `wasm32-unknown-unknown` (hydrate) and native (SSR) so the
//! same `Node`/`Edge`/`BrainFilePayload` flow through `#[server]` fn boundaries
//! without conditional cfg.

mod config;
mod error;
mod frontmatter;
mod types;
mod work_items;

pub use config::{BrainConfig, BrandConfig, ConfigError, GithubClient, NodeTypeSpec, TargetConfig};
pub use error::BrainError;
pub use frontmatter::split_frontmatter;
pub use types::{BrainFilePayload, Edge, EditMode, EditPrefill, Node};
pub use work_items::{
    ExternalWorkItemBinding, ExternalWorkItemSystem, WorkItem, WorkItemKind, WorkItemState,
    WorkItemSystemOfRecord,
};
