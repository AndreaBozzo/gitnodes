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

//! Read-only local preview mode (`gitnodes preview [dir]`).
//!
//! Serves the existing graph UI over a local working tree with no GitHub and no
//! OAuth: the SQLite projection is seeded from the directory at boot (the same
//! pipeline the read-only MCP server uses), the read seams return read-only
//! permissions without a forge call, and every write capability is denied. It is
//! the read half of the roadmap's Local/Offline execution context, with no local
//! commit path.
//!
//! Security boundary: like PAT mode, preview exposes the whole brain to anyone
//! who can reach the HTTP port — but read-only and unauthenticated. Startup
//! therefore refuses a non-loopback bind unless `GITNODES_ALLOW_REMOTE_PREVIEW`
//! is explicitly set.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use gitnodes_domain::{BrainConfig, BrainError, TargetConfig};
use gitnodes_storage::RepositoryPermissions;

use super::{projection, working_tree};

/// The resolved local working tree when preview mode is active.
struct LocalContext {
    /// Canonicalized root of the working tree being served.
    root: PathBuf,
    /// Synthetic target the projection is keyed by (`_local/<dir>/working-tree`).
    target: TargetConfig,
    /// `.gitnodes.yml` parsed from disk, refreshed on each rebuild. Held behind
    /// a mutex so the manual-refresh path can pick up edits to the config file.
    config: Mutex<Arc<BrainConfig>>,
}

static LOCAL: OnceLock<LocalContext> = OnceLock::new();

/// True when GitNodes is serving a local working tree (`gitnodes preview`).
pub fn is_enabled() -> bool {
    LOCAL.get().is_some()
}

/// The synthetic target for the local working tree, or `None` outside preview.
pub fn target() -> Option<TargetConfig> {
    LOCAL.get().map(|ctx| ctx.target.clone())
}

/// Require a request to address the one synthetic target exposed by preview.
///
/// Preview reuses target-explicit server functions, so this check prevents an
/// arbitrary `TargetRef` from inheriting local read permissions merely because
/// the process happens to be in preview mode.
pub fn ensure_target(target: &TargetConfig) -> Result<(), BrainError> {
    let Some(active) = self::target() else {
        return Ok(());
    };
    if active == *target {
        Ok(())
    } else {
        Err(BrainError::permission_denied(format!(
            "local preview only serves {}/{}/{}",
            active.org, active.repo, active.branch
        )))
    }
}

/// Reject every mutation before it can reach a forge-backed write path.
pub fn ensure_writable() -> Result<(), BrainError> {
    if is_enabled() {
        Err(BrainError::permission_denied("local preview is read-only"))
    } else {
        Ok(())
    }
}

/// The `.gitnodes.yml` parsed from the working tree (default when absent).
pub fn config() -> Arc<BrainConfig> {
    LOCAL
        .get()
        .map(|ctx| {
            ctx.config
                .lock()
                .expect("local config mutex poisoned")
                .clone()
        })
        .unwrap_or_default()
}

/// Read one markdown file's content from the working tree, confined to the
/// brain root (path traversal rejected). Used by the detail panel/editor read
/// path, which otherwise fetches blob content from the forge.
pub fn read_file(path: &str) -> Result<String, String> {
    let ctx = LOCAL
        .get()
        .ok_or_else(|| "local preview not active".to_string())?;
    working_tree::read_confined_markdown(&ctx.root, path)
}

/// Read a configured template from `templates/`, confined to that directory.
pub fn read_template(filename: &str) -> Result<String, String> {
    let ctx = LOCAL
        .get()
        .ok_or_else(|| "local preview not active".to_string())?;
    let template_root = std::fs::canonicalize(ctx.root.join("templates"))
        .map_err(|error| format!("failed to open templates directory: {error}"))?;
    let candidate = std::fs::canonicalize(template_root.join(filename))
        .map_err(|error| format!("failed to open template {filename}: {error}"))?;
    if !candidate.starts_with(&template_root) {
        return Err("template path escapes the templates directory".to_string());
    }
    let metadata = std::fs::metadata(&candidate)
        .map_err(|error| format!("failed to inspect template {filename}: {error}"))?;
    if metadata.len() > working_tree::MAX_MARKDOWN_BYTES {
        return Err(format!(
            "template {filename} exceeds the {} byte limit",
            working_tree::MAX_MARKDOWN_BYTES
        ));
    }
    std::fs::read_to_string(&candidate)
        .map_err(|error| format!("failed to read template {filename} as UTF-8: {error}"))
}

/// Read raw bytes for a repo-relative asset path (e.g. `assets/foo.png`) from
/// the working tree, confined to the brain root. Lets the asset proxy serve
/// images from disk in preview mode instead of fetching them from the forge.
pub fn read_asset(repo_path: &str) -> Result<Vec<u8>, String> {
    let ctx = LOCAL
        .get()
        .ok_or_else(|| "local preview not active".to_string())?;
    let candidate = std::fs::canonicalize(ctx.root.join(repo_path))
        .map_err(|error| format!("failed to open {repo_path}: {error}"))?;
    if !candidate.starts_with(&ctx.root) {
        return Err("path escapes the knowledge directory".to_string());
    }
    std::fs::read(&candidate).map_err(|error| format!("failed to read {repo_path}: {error}"))
}

/// Read-only permissions handed to the read seams in preview mode: the brain is
/// browsable, every write/admin capability is denied.
pub fn read_only_permissions() -> RepositoryPermissions {
    RepositoryPermissions {
        pull: true,
        ..Default::default()
    }
}

fn remote_preview_allowed() -> bool {
    matches!(
        std::env::var("GITNODES_ALLOW_REMOTE_PREVIEW").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

/// Activate preview mode for `dir`. Canonicalizes the directory, builds the
/// synthetic target, and records the context. The projection is seeded
/// separately via [`rebuild_projection`] once the pool exists. Returns the
/// synthetic target so the caller can wire it into the request context.
pub fn activate(dir: &str) -> Result<TargetConfig, String> {
    let root = std::fs::canonicalize(dir)
        .map_err(|error| format!("failed to open local knowledge directory: {error}"))?;
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let repo = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("working-tree")
        .to_string();
    let target = TargetConfig {
        org: "_local".to_string(),
        repo,
        branch: "working-tree".to_string(),
    };
    LOCAL
        .set(LocalContext {
            root,
            target: target.clone(),
            config: Mutex::new(Arc::new(BrainConfig::default())),
        })
        .map_err(|_| "local preview already activated".to_string())?;
    Ok(target)
}

/// Enforce the loopback guardrail. Fatal when preview binds a non-loopback
/// address without the explicit opt-in, so remote exposure is deliberate.
pub fn enforce_loopback(addr: &SocketAddr) {
    if !is_enabled() {
        return;
    }
    if !addr.ip().is_loopback() && !remote_preview_allowed() {
        tracing::error!(
            %addr,
            "gitnodes preview binds a non-loopback address. Preview serves the \
             whole brain read-only with no authentication. Bind to 127.0.0.1, or \
             set GITNODES_ALLOW_REMOTE_PREVIEW=1 if it sits behind your own \
             auth/proxy."
        );
        std::process::exit(1);
    }
}

/// (Re)read the working tree and rebuild the projection from it. Used for the
/// initial boot seed and the manual refresh button. Also refreshes the cached
/// config so a `.gitnodes.yml` edit is reflected.
pub async fn rebuild_projection(reason: &str) -> Result<(), String> {
    let Some(ctx) = LOCAL.get() else {
        return Err("local preview not active".to_string());
    };
    let (config, files) = working_tree::read_working_tree(&ctx.root)?;
    projection::rebuild_from_raw_files(&ctx.target, &files, &config, reason)
        .await
        .map_err(|error| error.to_string())?;
    *ctx.config.lock().expect("local config mutex poisoned") = Arc::new(config);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_target, read_only_permissions};
    use gitnodes_domain::TargetConfig;

    #[test]
    fn read_only_permissions_allow_read_only() {
        let permissions = read_only_permissions();
        assert!(permissions.pull, "preview must allow reads");
        assert!(!permissions.push, "preview must deny direct writes");
        assert!(!permissions.admin, "preview must deny admin");
        assert!(!permissions.maintain, "preview must deny maintain");
    }

    #[test]
    fn target_check_is_a_noop_outside_preview() {
        let target = TargetConfig {
            org: "example".into(),
            repo: "brain".into(),
            branch: "main".into(),
        };
        ensure_target(&target).expect("normal server mode should not be constrained");
    }
}
