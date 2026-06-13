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

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
