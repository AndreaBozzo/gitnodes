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

//! Self-extracting embedded web assets for single-binary distribution.
//!
//! Built only with `--features embed-assets`, after a normal `cargo leptos
//! build` has produced `target/site`. That directory is compiled into the
//! binary; on startup it is written once to a per-version cache directory and
//! `LEPTOS_SITE_ROOT` is pointed at it, so the existing static-file handler
//! serves it unchanged. The result ships as a single self-contained file.

use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

static SITE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../target/site");

/// Extract the embedded site to a versioned cache directory and return its
/// path. Re-extraction is skipped when a complete copy already exists, so
/// repeated runs of the same version pay the cost only once.
pub fn extract_site() -> std::io::Result<PathBuf> {
    let build_id = option_env!("GITNODES_BUILD_ID").unwrap_or(env!("CARGO_PKG_VERSION"));
    let root = cache_root().join(format!("gitnodes-site-{build_id}"));
    let sentinel = root.join(".extracted");
    if sentinel.exists() {
        return Ok(root);
    }
    std::fs::create_dir_all(&root)?;
    SITE.extract(&root)?;
    reconcile_wasm_name(&root)?;
    std::fs::write(&sentinel, build_id)?;
    Ok(root)
}

/// cargo-leptos (0.3.x) emits the hydrate module as `{output_name}.wasm`, but
/// leptos 0.8's `HydrationScripts` loads it from `{output_name}_bg.wasm` when
/// file hashing is off. cargo-leptos's own dev server reconciles this; the
/// standalone binary must too, or the page renders but never hydrates. Provide
/// the `_bg` name the runtime references. No-op when the build already matches.
fn reconcile_wasm_name(root: &Path) -> std::io::Result<()> {
    let pkg = root.join("pkg");
    let Ok(entries) = std::fs::read_dir(&pkg) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wasm") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if stem.ends_with("_bg") {
            continue;
        }
        let bg = pkg.join(format!("{stem}_bg.wasm"));
        if !bg.exists() {
            std::fs::copy(&path, &bg)?;
        }
    }
    Ok(())
}

/// Best-effort OS cache directory, falling back to the temp dir.
fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(".cache");
    }
    std::env::temp_dir()
}
