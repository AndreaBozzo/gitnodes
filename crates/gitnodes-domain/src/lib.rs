// Copyright 2026 Andrea Bozzo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Pure domain types for gitnodes.
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

pub use config::{
    BrainConfig, BrandConfig, ConfigError, GithubClient, NodeTypeSpec, TargetConfig, TargetKey,
    TargetRef, TargetRefError, ViewSpec, decode_path_segment, encode_path_segment,
    slugify_view_name,
};
pub use error::{BrainError, ConflictKind};
pub use frontmatter::split_frontmatter;
pub use types::{BrainFilePayload, Edge, EdgeKind, EditMode, EditPrefill, Node, WriteIntent};
pub use work_items::{
    ExternalWorkItemBinding, ExternalWorkItemSystem, WorkItem, WorkItemKind, WorkItemState,
    WorkItemSystemOfRecord,
};
