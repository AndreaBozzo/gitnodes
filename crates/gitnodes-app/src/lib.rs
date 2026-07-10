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

//! The GitNodes application: a Leptos 0.8 fullstack web app (SSR binary + WASM
//! hydrate) over an Axum server. This is the deployable crate.
//!
//! Server-only code is gated behind `feature = "ssr"`, client-only behind
//! `hydrate`; the binary builds with `ssr`, the WASM lib with `hydrate`.
//! Anything touching tokio/reqwest/sqlx must be `#[cfg(feature = "ssr")]`.
//!
//! Holds the Axum router (OAuth, webhook, SSE, `/api` server fns, asset proxy),
//! the rebuildable SQLite projection, multi-target routing, the permission-aware
//! write orchestrator, and the Leptos UI. Git is the single source of truth; the
//! projection is a derived index, never a primary store.

#![recursion_limit = "512"]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

pub mod admin;
pub mod api;
pub mod app;
#[cfg(feature = "ssr")]
pub mod cli;
pub mod knowledge;
pub mod landing;
pub mod markdown;
#[cfg(feature = "ssr")]
pub mod mcp;
#[cfg(feature = "ssr")]
pub mod server;
#[cfg(feature = "ssr")]
pub mod validation;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
