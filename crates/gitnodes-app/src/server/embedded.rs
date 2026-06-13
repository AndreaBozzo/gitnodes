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
    std::fs::write(&sentinel, build_id)?;
    Ok(root)
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
