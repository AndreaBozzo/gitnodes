// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

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
