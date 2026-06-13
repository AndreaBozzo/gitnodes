pub mod brain_switcher;
pub mod components;
#[cfg(feature = "ssr")]
pub mod config_loader;
mod detail_bar;
mod detail_panel;
pub(crate) mod draft;
mod editor;
mod filter_panel;
mod graph_canvas;
pub mod live_sync;
#[cfg(feature = "hydrate")]
pub(crate) mod mermaid;
mod orphan_banner;
pub mod page;
pub mod pull_requests;
mod repo_structure;
#[cfg(feature = "ssr")]
pub mod runtime;
pub mod types;

pub use page::KnowledgePage;
